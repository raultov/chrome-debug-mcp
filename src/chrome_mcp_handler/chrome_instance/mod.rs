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
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/debug.log")?;
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
        let Ok(parsed_addr) = addr.parse() else {
            return false;
        };
        let Ok(mut stream) = TcpStream::connect_timeout(&parsed_addr, Duration::from_secs(1))
        else {
            return false;
        };

        let request = format!(
            "GET /json/version HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            addr
        );

        use std::io::{Read, Write};
        if stream.write_all(request.as_bytes()).is_err() {
            return false;
        }

        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);

        response.contains("Browser") || response.contains("WebKit-Version")
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
}
