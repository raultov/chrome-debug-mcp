pub mod restart_chrome;
pub mod stop_chrome;

use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct ChromeInstanceManager {
    child: Option<Child>,
    port: u16,
    user_data_dir: std::path::PathBuf,
}

impl ChromeInstanceManager {
    pub fn new(port: u16) -> Self {
        let user_data_dir = std::env::temp_dir().join(format!("chrome-mcp-profile-{}", port));
        Self {
            child: None,
            port,
            user_data_dir,
        }
    }

    pub fn get_port(&self) -> u16 {
        self.port
    }

    pub async fn ensure_instance(&mut self) -> anyhow::Result<()> {
        let _ = self.log("ensure_instance started");
        if self.is_port_open().await {
            // Already running
            return Ok(());
        }

        self.start_instance()?;

        // Wait for it to be ready
        let mut attempts = 0;
        while attempts < 20 {
            if self.is_port_open().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            attempts += 1;
        }

        anyhow::bail!(
            "Timed out waiting for Chrome to start on port {}",
            self.port
        )
    }

    async fn is_port_open(&self) -> bool {
        let addr = format!("127.0.0.1:{}", self.port);
        match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(200)) {
            Ok(_) => {
                let _ = self.log(&format!("TCP port {} is open", self.port));
                true
            }
            Err(_) => false,
        }
    }

    fn log(&self, _msg: &str) -> std::io::Result<()> {
        // Silenced to avoid protocol corruption in some hosts
        Ok(())
    }

    fn start_instance(&mut self) -> anyhow::Result<()> {
        let _ = self.log("start_instance called");
        let os = std::env::consts::OS;
        let binaries = match os {
            "windows" => vec![
                "chrome.exe".to_string(),
                r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
            ],
            "macos" => {
                vec!["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()]
            }
            _ => vec![
                "/opt/google/chrome/chrome".to_string(),
                "google-chrome".to_string(),
                "chromium".to_string(),
            ],
        };


        let _ = self.log(&format!(
            "Starting new Chrome instance on port {} for OS {}...",
            self.port, os
        ));

        let mut last_error = None;

        for exec in binaries {
            match Command::new(&exec)
                .arg(format!("--remote-debugging-port={}", self.port))
                .arg("--remote-allow-origins=*")
                .arg("--no-first-run")
                .arg("--no-default-browser-check")
                .arg(format!("--user-data-dir={}", self.user_data_dir.to_string_lossy()))
                .arg("--disable-session-crashed-bubble")
                .arg("--disable-infobars")
                .arg("--noerrdialogs")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .stdin(Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    self.child = Some(child);
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Failed to launch Chrome: {}", e));
                }
            }
        }

        let err =
            last_error.unwrap_or_else(|| anyhow::anyhow!("No Chrome binaries found for OS {}", os));
        let _ = self.log(&format!("CRITICAL: {}", err));
        Err(err)
    }

    pub fn stop_instance(&mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Fallback: Ensure no process is left on the port
        #[cfg(target_os = "linux")]
        {
            let cmd = format!("fuser -k {}/tcp", self.port);
            let _ = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(target_os = "windows")]
        {
            let cmd = format!(
                "for /f \"tokens=5\" %a in ('netstat -aon ^| findstr :{}') do taskkill /f /pid %a",
                self.port
            );
            let _ = Command::new("cmd")
                .arg("/c")
                .arg(cmd)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(target_os = "macos")]
        {
            let cmd = format!(
                "lsof -i tcp:{} | grep LISTEN | awk '{{print $2}}' | xargs kill -9",
                self.port
            );
            let _ = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }

        // Cleanup SingletonLock to prevent "Chrome didn't shut down correctly"
        let lock_file = self.user_data_dir.join("SingletonLock");
        if lock_file.exists() {
            let _ = std::fs::remove_file(lock_file);
        }

        Ok(())
    }
}
