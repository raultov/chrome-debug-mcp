pub mod restart_chrome;
pub mod stop_chrome;

use async_trait::async_trait;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[async_trait]
pub trait ChromeManager: Send + Sync {
    async fn ensure_instance(&mut self) -> anyhow::Result<()>;
    fn stop_instance(&mut self) -> anyhow::Result<()>;
    fn get_port(&self) -> u16;
    #[allow(dead_code)]
    fn set_port(&mut self, port: u16);
    fn set_proxy(&mut self, proxy: Option<String>);
}

pub struct ChromeInstanceManager {
    child: Option<Child>,
    host: String,
    port: u16,
    user_data_dir: std::path::PathBuf,
    proxy_server: Option<String>,
    enable_automation: bool,
    headless: bool,
    user_profile: bool,
}

#[async_trait]
impl ChromeManager for ChromeInstanceManager {
    async fn ensure_instance(&mut self) -> anyhow::Result<()> {
        self.ensure_instance_impl().await
    }

    fn stop_instance(&mut self) -> anyhow::Result<()> {
        self.stop_instance_impl()
    }

    fn get_port(&self) -> u16 {
        self.port
    }

    fn set_port(&mut self, port: u16) {
        self.port = port;
    }

    fn set_proxy(&mut self, proxy: Option<String>) {
        self.proxy_server = proxy;
    }
}

impl ChromeInstanceManager {
    fn get_chrome_path() -> String {
        if let Ok(path) = std::env::var("CHROME_PATH") {
            return path;
        }

        #[cfg(target_os = "macos")]
        {
            return "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string();
        }

        #[cfg(target_os = "windows")]
        {
            return "chrome".to_string();
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            "google-chrome".to_string()
        }
    }

    pub fn new(
        host: String,
        port: u16,
        enable_automation: bool,
        headless: bool,
        user_profile: bool,
    ) -> Self {
        let user_data_dir = std::env::temp_dir().join(format!("chrome-mcp-profile-{}", port));
        Self {
            child: None,
            host,
            port,
            user_data_dir,
            proxy_server: None,
            enable_automation,
            headless,
            user_profile,
        }
    }

    fn log(&self, msg: &str) -> anyhow::Result<()> {
        self.log_to(std::path::Path::new("logs"), msg)
    }

    fn log_to(&self, base: &std::path::Path, msg: &str) -> anyhow::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(base)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(base.join("debug.log"))?;
        writeln!(file, "[ChromeManager:{}] {}", self.port, msg)?;
        Ok(())
    }

    async fn is_port_open(&self) -> bool {
        let addr = format!("{}:{}", self.host, self.port);
        let Ok(parsed_addr) = addr.parse() else {
            return false;
        };
        let result = TcpStream::connect_timeout(&parsed_addr, Duration::from_millis(500)).is_ok();
        let _ = self.log(&format!("is_port_open: {} -> {}", addr, result));
        result
    }

    fn is_managed_profile_active(&self) -> bool {
        if self.user_profile {
            return false;
        }
        let lock_file = std::env::temp_dir()
            .join(format!("chrome-mcp-profile-{}", self.port))
            .join("SingletonLock");
        lock_file.exists()
    }

    async fn is_chrome_cdp(&self) -> bool {
        let addr = format!("{}:{}", self.host, self.port);

        let probe = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let mut stream = tokio::net::TcpStream::connect(&addr).await.ok()?;

            let request = format!(
                "GET /json/version HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                addr
            );
            stream.write_all(request.as_bytes()).await.ok()?;

            // Chrome >= 148 ignores `Connection: close` and keeps the socket open,
            // so we cannot read until EOF. Instead, we stop as soon as the
            // identifying markers appear or the global timeout fires.
            let mut response = String::new();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break, // EOF (older Chrome does close)
                    Ok(n) => {
                        response.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if response.contains("Browser") || response.contains("WebKit-Version") {
                            return Some(true);
                        }
                        if response.len() > 64 * 1024 {
                            break; // absurdly large response: not DevTools
                        }
                    }
                    Err(_) => break,
                }
            }
            Some(response.contains("Browser") || response.contains("WebKit-Version"))
        };

        // Global timeout: covers connect + write + read altogether.
        matches!(
            tokio::time::timeout(Duration::from_secs(2), probe).await,
            Ok(Some(true))
        )
    }

    async fn find_new_port(&mut self) -> anyhow::Result<()> {
        let original_port = self.port;
        for port in (original_port + 1)..(original_port + 100) {
            let addr = format!("{}:{}", self.host, port);
            let Ok(parsed_addr) = addr.parse() else {
                continue;
            };

            let is_open =
                TcpStream::connect_timeout(&parsed_addr, Duration::from_millis(200)).is_ok();
            if !is_open {
                let lock_file = std::env::temp_dir()
                    .join(format!("chrome-mcp-profile-{}", port))
                    .join("SingletonLock");
                if !lock_file.exists() {
                    self.port = port;
                    self.user_data_dir =
                        std::env::temp_dir().join(format!("chrome-mcp-profile-{}", port));
                    let _ = self.log(&format!("Found available dynamic port: {}", port));
                    return Ok(());
                }
            }
        }
        self.port = original_port;
        Err(anyhow::anyhow!(
            "Could not find an available port after 100 attempts"
        ))
    }

    async fn ensure_instance_impl(&mut self) -> anyhow::Result<()> {
        let _ = self.log(&format!(
            "ensure_instance started for {}:{}",
            self.host, self.port
        ));

        // 1. Check if our own child is already running
        if let Some(ref mut child) = self.child
            && child.try_wait()?.is_none()
            && self.is_port_open().await
        {
            return Ok(());
        }

        // 2. Port is open by someone else
        if self.is_port_open().await {
            if self.is_chrome_cdp().await {
                if self.is_managed_profile_active() {
                    let _ = self.log(&format!(
                        "Port {} is used by another managed instance",
                        self.port
                    ));
                    if self.host == "127.0.0.1" || self.host == "localhost" {
                        self.find_new_port().await?;
                        // Port changed to a closed one, proceed to start_instance
                    } else {
                        // Remote, just use it
                        return Ok(());
                    }
                } else {
                    // Port open but not managed. Assume user-started Chrome (the "distinguish" use case)
                    let _ = self.log(&format!(
                        "Port {} is open and not managed. Attaching to existing Chrome...",
                        self.port
                    ));
                    return Ok(());
                }
            } else {
                // Port open but NOT Chrome!
                if self.host == "127.0.0.1" || self.host == "localhost" {
                    let _ = self.log(&format!(
                        "Port {} is taken by a non-Chrome process. Finding new port...",
                        self.port
                    ));
                    self.find_new_port().await?;
                } else {
                    return Err(anyhow::anyhow!(
                        "Port {} is taken by a non-Chrome process",
                        self.port
                    ));
                }
            }
        }

        // 3. If port is still not open, we start it (if local)
        if !self.is_port_open().await {
            if self.host != "127.0.0.1" && self.host != "localhost" {
                return Err(anyhow::anyhow!(
                    "Chrome instance not found at {}:{}. Cannot start remote instance.",
                    self.host,
                    self.port
                ));
            }

            self.start_instance().await?;
        }

        Ok(())
    }

    async fn start_instance(&mut self) -> anyhow::Result<()> {
        let _ = self.log("Starting new instance...");

        if !self.user_profile {
            // Ensure user data dir exists
            if !self.user_data_dir.exists() {
                std::fs::create_dir_all(&self.user_data_dir)?;
            } else {
                // Patch preferences to avoid crash bubble
                let _ = self.patch_preferences();
            }
        }

        let chrome_path = Self::get_chrome_path();
        let mut cmd = Command::new(&chrome_path);
        cmd.arg(format!("--remote-debugging-port={}", self.port))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-session-crashed-bubble")
            .arg("--noerrdialogs")
            .arg("--disable-dev-shm-usage");

        if !self.user_profile {
            cmd.arg(format!("--user-data-dir={}", self.user_data_dir.display()));
        }

        if self.enable_automation {
            cmd.arg("--enable-automation");
        } else {
            cmd.arg("--disable-infobars");
        }

        if self.headless {
            cmd.arg("--headless=new");
        }

        if self.headless || std::env::var("CHROME_NO_SANDBOX").is_ok() {
            cmd.arg("--no-sandbox");
        }

        if let Some(proxy) = &self.proxy_server {
            cmd.arg(format!("--proxy-server={}", proxy));
        }

        let mut child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start Chrome using path '{}'. If Chrome is not installed in the default location, please set the CHROME_PATH environment variable to point to the executable. OS Error: {}",
                    chrome_path,
                    e
                )
            })?;

        // Wait for port to open
        let mut attempts = 0;
        while attempts < 50 {
            if self.is_port_open().await {
                let _ = self.log("Chrome started successfully.");
                self.child = Some(child);
                return Ok(());
            }

            // Check if the process exited early (e.g. delegated to existing session)
            if let Ok(Some(_status)) = child.try_wait() {
                let err_msg = if self.user_profile {
                    "Chrome process exited immediately. When using --user-profile, Chrome cannot open a debugging port if there is already a running Chrome instance. You must either CLOSE ALL existing Chrome windows before starting the MCP, OR launch your main Chrome browser manually with the --remote-debugging-port flag."
                } else {
                    "Chrome process exited unexpectedly before opening the debugging port."
                };
                let err = anyhow::anyhow!(err_msg);
                let _ = self.log(&err.to_string());
                return Err(err);
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
            attempts += 1;
        }

        self.child = Some(child);
        let err = anyhow::anyhow!("Chrome failed to start after multiple attempts");
        let _ = self.log(&format!("Error: {}", err));
        Err(err)
    }

    fn patch_preferences(&self) -> anyhow::Result<()> {
        if self.user_profile {
            return Ok(());
        }
        let prefs_path = self.user_data_dir.join("Default").join("Preferences");
        if !prefs_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&prefs_path)?;
        let mut json: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(profile) = json.get_mut("profile")
            && let Some(profile_obj) = profile.as_object_mut()
        {
            profile_obj.insert("exit_type".to_string(), serde_json::json!("Normal"));
            profile_obj.insert("exited_cleanly".to_string(), serde_json::json!(true));
        }

        std::fs::write(&prefs_path, serde_json::to_string(&json)?)?;
        let _ = self.log("Patched Preferences to avoid crash bubble.");
        Ok(())
    }

    fn stop_instance_impl(&mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            {
                // Try SIGTERM first
                let pid = child.id();
                let _ = Command::new("kill")
                    .arg("-15")
                    .arg(pid.to_string())
                    .status();

                // Wait a bit
                std::thread::sleep(Duration::from_millis(500));

                // If still alive, kill it
                if let Ok(None) = child.try_wait() {
                    let _ = child.kill();
                }
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let _ = child.wait();
        }

        if !self.user_profile {
            // Clean up SingletonLock
            let lock_file = self.user_data_dir.join("SingletonLock");
            if lock_file.exists() {
                let _ = std::fs::remove_file(lock_file);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
pub struct MockChromeManager {
    port: u16,
}

#[cfg(test)]
impl MockChromeManager {
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

#[cfg(test)]
#[async_trait]
impl ChromeManager for MockChromeManager {
    async fn ensure_instance(&mut self) -> anyhow::Result<()> {
        // Mock: do nothing
        Ok(())
    }

    fn stop_instance(&mut self) -> anyhow::Result<()> {
        // Mock: do nothing
        Ok(())
    }

    fn get_port(&self) -> u16 {
        self.port
    }

    fn set_port(&mut self, port: u16) {
        self.port = port;
    }

    fn set_proxy(&mut self, _proxy: Option<String>) {
        // Mock: do nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_chrome_path_with_env() {
        // Set environment variable
        unsafe {
            std::env::set_var("CHROME_PATH", "/custom/path/to/chrome");
        }

        let path = ChromeInstanceManager::get_chrome_path();
        assert_eq!(path, "/custom/path/to/chrome");

        // Cleanup to not affect other tests
        unsafe {
            std::env::remove_var("CHROME_PATH");
        }
    }

    #[test]
    fn test_get_chrome_path_without_env() {
        // Ensure env var is not set
        unsafe {
            std::env::remove_var("CHROME_PATH");
        }

        let path = ChromeInstanceManager::get_chrome_path();
        // The default path depends on the OS
        #[cfg(target_os = "macos")]
        assert_eq!(
            path,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        );

        #[cfg(target_os = "windows")]
        assert_eq!(path, "chrome");

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(path, "google-chrome");
    }

    #[tokio::test]
    async fn test_find_new_port() {
        use std::net::TcpListener;
        let base_port = 12000;
        // Occupy the base port
        let _listener = TcpListener::bind(format!("127.0.0.1:{}", base_port)).unwrap();

        let mut manager =
            ChromeInstanceManager::new("127.0.0.1".into(), base_port, true, true, false);
        manager.find_new_port().await.unwrap();

        assert!(manager.port > base_port);
        assert!(manager.port < base_port + 100);
        // Verify user_data_dir updated
        assert!(
            manager
                .user_data_dir
                .to_string_lossy()
                .contains(&manager.port.to_string())
        );
    }

    #[test]
    fn test_is_managed_profile_active() {
        let port = 12001;
        let manager = ChromeInstanceManager::new("127.0.0.1".into(), port, true, true, false);

        // Initially inactive
        assert!(!manager.is_managed_profile_active());

        // Create the lock file
        let profile_dir = std::env::temp_dir().join(format!("chrome-mcp-profile-{}", port));
        std::fs::create_dir_all(&profile_dir).unwrap();
        let lock_file = profile_dir.join("SingletonLock");
        std::fs::write(&lock_file, "lock").unwrap();

        assert!(manager.is_managed_profile_active());

        // Cleanup
        let _ = std::fs::remove_file(lock_file);
        let _ = std::fs::remove_dir(profile_dir);
    }

    // ── Issue #4 regression tests ────────────────────────────────────
    // Test infrastructure: mock DevTools HTTP server

    #[derive(Clone, Copy)]
    enum MockBehavior {
        /// Chrome ≥148: responds with Content-Length and keeps the socket alive
        /// (never closes). Reproduces the hang reported in issue #4.
        KeepAlive,
        /// Chrome that honors Connection: close (closes socket after response).
        CloseAfterResponse,
        /// Responds and keeps alive, then closes after the given duration.
        KeepAliveThenCloseAfter(Duration),
        /// Accepts connection but never sends anything.
        SilentPeer,
        /// HTTP 200 with non-Chrome body, then closes.
        NotChrome,
    }

    fn chrome_version_json() -> String {
        r#"{"Browser":"Chrome/148.0.7778.216","Protocol-Version":"1.3","WebKit-Version":"537.36","webSocketDebuggerUrl":"ws://127.0.0.1:9224/devtools/browser/abc"}"#.to_string()
    }

    fn chrome_http_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Security-Policy: frame-ancestors 'none'\r\n\
             Content-Length: {}\r\n\
             Content-Type: application/json; charset=UTF-8\r\n\
             \r\n\
             {}",
            body.len(),
            body
        )
    }

    /// Starts a mock DevTools HTTP server on an ephemeral port.
    /// Returns `(port, JoinHandle)`. The handle should be aborted at test end.
    async fn mock_devtools_server(behavior: MockBehavior) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };

                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};

                    // Consume the HTTP request (read until \r\n\r\n)
                    let mut req_buf = vec![0u8; 4096];
                    let mut total = 0;
                    loop {
                        match stream.read(&mut req_buf[total..]).await {
                            Ok(0) => return, // Connection dropped early (e.g. is_port_open)
                            Ok(n) => {
                                total += n;
                                if String::from_utf8_lossy(&req_buf[..total]).contains("\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }

                    match behavior {
                        MockBehavior::KeepAlive => {
                            let body = chrome_version_json();
                            let resp = chrome_http_response(&body);
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                            // Keep socket open forever (Chrome ≥148 ignoring Connection: close)
                            std::future::pending::<()>().await;
                        }
                        MockBehavior::CloseAfterResponse => {
                            let body = chrome_version_json();
                            let resp = chrome_http_response(&body);
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                            // Drop stream → close socket
                        }
                        MockBehavior::KeepAliveThenCloseAfter(delay) => {
                            let body = chrome_version_json();
                            let resp = chrome_http_response(&body);
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                            tokio::time::sleep(delay).await;
                            // Drop stream → close socket
                        }
                        MockBehavior::SilentPeer => {
                            // Never send anything; keep socket alive
                            std::future::pending::<()>().await;
                        }
                        MockBehavior::NotChrome => {
                            let body = r#"{"status":"ok","service":"nginx"}"#;
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.flush().await;
                            // Drop stream → close socket
                        }
                    }
                });
            }
        });

        (port, handle)
    }

    fn test_manager(port: u16) -> ChromeInstanceManager {
        ChromeInstanceManager::new("127.0.0.1".into(), port, false, true, false)
    }

    // ── T1: Regression for B1 (main bug from issue #4) ──
    // With the old code, read_to_string() blocks forever when Chrome keeps the
    // connection alive. The probe must detect markers and return quickly.

    #[tokio::test]
    async fn given_keepalive_devtools_endpoint_when_probing_cdp_then_returns_true_within_3s() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::KeepAlive).await;
        let mgr = test_manager(port);

        let result = tokio::time::timeout(Duration::from_secs(3), mgr.is_chrome_cdp()).await;

        assert!(
            matches!(result, Ok(true)),
            "Expected Ok(true) within 3s, got {:?}. \
             The probe must detect Chrome markers and return quickly even when \
             the server keeps the connection alive (Chrome >=148 behavior).",
            result
        );

        server_handle.abort();
    }

    // ── T2: Regression for B2 (blocking I/O on async runtime) ──
    // On a current_thread runtime, blocking I/O prevents the mock server task
    // from being polled, causing a deadlock. With truly async I/O both the
    // probe and the mock interleave cooperatively.

    #[tokio::test(flavor = "current_thread")]
    async fn given_slow_endpoint_when_probing_cdp_then_other_tasks_keep_progressing() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::KeepAliveThenCloseAfter(
            Duration::from_millis(500),
        ))
        .await;
        let mgr = test_manager(port);

        let result = tokio::time::timeout(Duration::from_secs(3), mgr.is_chrome_cdp()).await;

        assert!(
            matches!(result, Ok(true)),
            "Expected Ok(true) on current_thread runtime, got {:?}. \
             If this test hangs/times out, it indicates the probe uses blocking I/O \
             that prevents cooperative scheduling with the mock server.",
            result
        );

        server_handle.abort();
    }

    // ── T3: Timeout behavior on silent peer ──
    // A peer that accepts but never responds must be handled by the internal
    // timeout (~2s), not hang forever.

    #[tokio::test]
    async fn given_silent_peer_when_probing_cdp_then_returns_false_within_bounded_time() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::SilentPeer).await;
        let mgr = test_manager(port);

        let start = std::time::Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(4), mgr.is_chrome_cdp()).await;
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Ok(false)),
            "Expected Ok(false) within 4s, got {:?}. \
             A silent peer should trigger the internal timeout.",
            result
        );
        assert!(
            elapsed <= Duration::from_millis(2500),
            "Internal timeout should fire at ~2s, but took {:?}",
            elapsed
        );

        server_handle.abort();
    }

    // ── T4: Non-regression — non-Chrome HTTP server ──

    #[tokio::test]
    async fn given_non_chrome_http_server_when_probing_cdp_then_returns_false() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::NotChrome).await;
        let mgr = test_manager(port);

        let result = tokio::time::timeout(Duration::from_secs(3), mgr.is_chrome_cdp()).await;

        assert!(
            matches!(result, Ok(false)),
            "Expected Ok(false) for non-Chrome server, got {:?}",
            result
        );

        server_handle.abort();
    }

    // ── T5: Non-regression — closed port ──

    #[tokio::test]
    async fn given_closed_port_when_probing_cdp_then_returns_false_quickly() {
        // Bind to get an ephemeral port, then drop to ensure nothing listens
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mgr = test_manager(port);
        let start = std::time::Instant::now();
        let result = mgr.is_chrome_cdp().await;
        let elapsed = start.elapsed();

        assert!(!result, "Should return false for closed port");
        assert!(
            elapsed < Duration::from_millis(2500),
            "Should fail within the 2s internal timeout, took {:?}",
            elapsed
        );
    }

    // ── T6: Non-regression — Chrome that honors Connection: close ──

    #[tokio::test]
    async fn given_closing_devtools_endpoint_when_probing_cdp_then_returns_true() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::CloseAfterResponse).await;
        let mgr = test_manager(port);

        let result = tokio::time::timeout(Duration::from_secs(3), mgr.is_chrome_cdp()).await;

        assert!(
            matches!(result, Ok(true)),
            "Expected Ok(true) for a server that closes after response, got {:?}",
            result
        );

        server_handle.abort();
    }

    // ── T7: BDD E2E — attaching to user-started Chrome ──
    // This is the exact scenario from issue #4: a user-started Chrome on a
    // port, no managed profile lock, ensure_instance should attach without
    // spawning a child process.

    #[tokio::test]
    async fn given_user_started_chrome_when_ensure_instance_then_attaches_without_spawning() {
        let (port, server_handle) = mock_devtools_server(MockBehavior::KeepAlive).await;
        let mut mgr = test_manager(port);

        // Precondition: no managed profile lock file for this ephemeral port
        let lock_file = std::env::temp_dir()
            .join(format!("chrome-mcp-profile-{}", port))
            .join("SingletonLock");
        assert!(
            !lock_file.exists(),
            "Precondition failed: SingletonLock should not exist for ephemeral port {}",
            port
        );

        let result = tokio::time::timeout(Duration::from_secs(5), mgr.ensure_instance()).await;

        assert!(
            matches!(result, Ok(Ok(()))),
            "Expected Ok(Ok(())) — ensure_instance should attach to existing Chrome. Got {:?}",
            result
        );
        assert!(
            mgr.child.is_none(),
            "Should not have spawned a child Chrome process when attaching to existing instance"
        );

        server_handle.abort();
    }

    // ── T8: Regression for B3 — log directory creation ──
    // The old log() failed silently when logs/ didn't exist. The fix creates
    // the directory with create_dir_all before writing.

    #[test]
    fn given_missing_logs_dir_when_logging_then_creates_dir_and_writes() {
        let tmp = std::env::temp_dir().join(format!("chrome-mcp-logtest-{}", std::process::id()));
        // Ensure starting from a clean state
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(!tmp.exists(), "Precondition: temp log dir should not exist");

        let mgr = test_manager(0);
        let result = mgr.log_to(&tmp, "hello from test");

        assert!(
            result.is_ok(),
            "log_to should succeed even when directory doesn't exist: {:?}",
            result.err()
        );

        let log_path = tmp.join("debug.log");
        assert!(log_path.exists(), "debug.log should have been created");

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("hello from test"),
            "Log should contain the message, got: {}",
            content
        );
        assert!(
            content.contains("[ChromeManager:0]"),
            "Log should contain the port prefix, got: {}",
            content
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
