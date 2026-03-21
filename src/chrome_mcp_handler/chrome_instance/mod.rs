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
    port: u16,
    user_data_dir: std::path::PathBuf,
    proxy_server: Option<String>,
    enable_automation: bool,
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

    pub fn new(port: u16, enable_automation: bool) -> Self {
        let user_data_dir = std::env::temp_dir().join(format!("chrome-mcp-profile-{}", port));
        Self {
            child: None,
            port,
            user_data_dir,
            proxy_server: None,
            enable_automation,
        }
    }

    fn log(&self, msg: &str) -> anyhow::Result<()> {
        eprintln!("[ChromeManager:{}] {}", self.port, msg);
        Ok(())
    }

    async fn is_port_open(&self) -> bool {
        let addr = format!("127.0.0.1:{}", self.port);
        TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(200)).is_ok()
    }

    async fn ensure_instance_impl(&mut self) -> anyhow::Result<()> {
        let _ = self.log("ensure_instance started");
        if self.is_port_open().await {
            // Already running
            return Ok(());
        }
        self.start_instance().await
    }

    async fn start_instance(&mut self) -> anyhow::Result<()> {
        let _ = self.log("Starting new instance...");

        // Ensure user data dir exists
        if !self.user_data_dir.exists() {
            std::fs::create_dir_all(&self.user_data_dir)?;
        } else {
            // Patch preferences to avoid crash bubble
            let _ = self.patch_preferences();
        }

        let chrome_path = Self::get_chrome_path();
        let mut cmd = Command::new(&chrome_path);
        cmd.arg(format!("--remote-debugging-port={}", self.port))
            .arg(format!("--user-data-dir={}", self.user_data_dir.display()))
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-session-crashed-bubble")
            .arg("--noerrdialogs");

        if self.enable_automation {
            cmd.arg("--enable-automation");
        } else {
            cmd.arg("--disable-infobars");
        }

        if let Some(proxy) = &self.proxy_server {
            cmd.arg(format!("--proxy-server={}", proxy));
        }

        let child = cmd
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

        self.child = Some(child);

        // Wait for port to open
        let mut attempts = 0;
        while attempts < 50 {
            if self.is_port_open().await {
                let _ = self.log("Chrome started successfully.");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            attempts += 1;
        }

        let err = anyhow::anyhow!("Chrome failed to start after multiple attempts");
        let _ = self.log(&format!("Error: {}", err));
        Err(err)
    }

    fn patch_preferences(&self) -> anyhow::Result<()> {
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

        // Clean up SingletonLock
        let lock_file = self.user_data_dir.join("SingletonLock");
        if lock_file.exists() {
            let _ = std::fs::remove_file(lock_file);
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

    #[test]
    fn test_chrome_instance_manager_new() {
        let port = 9333;
        let manager = ChromeInstanceManager::new(port, true);
        assert_eq!(manager.port, port);
        assert!(manager.enable_automation);
        assert!(manager.user_data_dir.to_string_lossy().contains("9333"));

        let manager_no_auto = ChromeInstanceManager::new(port, false);
        assert!(!manager_no_auto.enable_automation);
    }
}
