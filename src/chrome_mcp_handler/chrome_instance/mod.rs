pub mod cdp_browser_manager;
#[cfg(test)]
pub mod integration_tests;
pub mod launch;
pub mod restart_chrome;
pub mod stop_chrome;

use async_trait::async_trait;
use cdp_browser_lite::CdpClient;

#[async_trait]
pub trait ChromeManager: Send + Sync {
    async fn ensure_instance(&mut self) -> anyhow::Result<()>;
    async fn stop_instance(&mut self) -> anyhow::Result<()>;
    async fn client(&self) -> anyhow::Result<CdpClient>;
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Phase 1 removed the only production caller per spec §4.1; the trait signature is preserved for test observability (Phase 3 unit tests call it via CdpBrowserManager)"
        )
    )]
    fn get_port(&self) -> u16;
    fn set_proxy(&mut self, proxy: Option<String>);
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

    async fn stop_instance(&mut self) -> anyhow::Result<()> {
        // Mock: do nothing
        Ok(())
    }

    async fn client(&self) -> anyhow::Result<CdpClient> {
        let addr = format!("127.0.0.1:{}", self.port);
        CdpClient::new(&addr, std::time::Duration::from_secs(10))
            .await
            .map_err(Into::into)
    }

    fn get_port(&self) -> u16 {
        self.port
    }

    fn set_proxy(&mut self, _proxy: Option<String>) {
        // Mock: do nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;

    #[tokio::test]
    async fn given_mock_manager_when_client_requested_then_connects_to_its_port() {
        let port = spawn_mock_chrome_server().await;
        let manager = MockChromeManager::new(port);

        let client = manager
            .client()
            .await
            .expect("mock manager client() should connect");

        let res = client
            .send_raw_command("Runtime.enable", serde_json::json!({}))
            .await;
        assert!(res.is_ok(), "command should succeed: {:?}", res.err());
    }
}
