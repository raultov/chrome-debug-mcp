use async_trait::async_trait;
use cdp_browser_lite::{BrowserClient, BrowserConfig, BrowserError, CdpClient};

use crate::chrome_mcp_handler::chrome_instance::launch::{LaunchParams, LaunchPlan};

pub(crate) struct CdpBrowserManager {
    pub(crate) params: LaunchParams,
    pub(crate) launcher: Box<dyn BrowserLauncher>,
    pub(crate) browser: Option<Box<dyn ManagedBrowser>>,
    pub(crate) resolved_port: u16,
}

impl CdpBrowserManager {
    pub(crate) fn new(params: LaunchParams, launcher: Box<dyn BrowserLauncher>) -> Self {
        let resolved_port = params.configured_port();
        Self {
            params,
            launcher,
            browser: None,
            resolved_port,
        }
    }
}

#[async_trait]
impl crate::chrome_mcp_handler::chrome_instance::ChromeManager for CdpBrowserManager {
    async fn ensure_instance(&mut self) -> anyhow::Result<()> {
        if let Some(browser) = self.browser.as_ref()
            && browser.is_alive().await
        {
            return Ok(());
        }
        self.browser = None;
        let plan: LaunchPlan = self.params.plan();
        let config = self.params.to_config(&plan);
        let browser = self.launcher.launch(config).await?;
        self.resolved_port = browser.resolved_port().await;
        self.browser = Some(browser);
        Ok(())
    }

    async fn stop_instance(&mut self) -> anyhow::Result<()> {
        if let Some(browser) = self.browser.take() {
            browser.stop().await?;
        }
        self.resolved_port = self.params.configured_port();
        Ok(())
    }

    async fn client(&self) -> anyhow::Result<CdpClient> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Chrome instance has not been ensured yet"))?;
        browser.client().await
    }

    async fn browser_client(&self) -> anyhow::Result<BrowserClient> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Chrome instance has not been ensured yet"))?;
        browser.browser_client().await
    }

    async fn profile_dir(&self) -> Option<std::path::PathBuf> {
        let browser = self.browser.as_ref()?;
        browser.profile_dir().await
    }

    fn get_port(&self) -> u16 {
        self.resolved_port
    }

    fn set_proxy(&mut self, proxy: Option<String>) {
        self.params.set_proxy(proxy);
    }

    fn features(&self) -> &[crate::chrome_mcp_handler::chrome_instance::launch::ChromeFeature] {
        self.params.features()
    }

    fn set_features(
        &mut self,
        features: Vec<crate::chrome_mcp_handler::chrome_instance::launch::ChromeFeature>,
    ) {
        self.params.set_features(features);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
pub(crate) trait ManagedBrowser: Send + Sync {
    async fn resolved_port(&self) -> u16;
    async fn is_alive(&self) -> bool;
    async fn client(&self) -> anyhow::Result<CdpClient>;
    /// Retrieves a browser-level CDP client for tab management.
    /// Default implementation rejects when the backend cannot provide one.
    async fn browser_client(&self) -> anyhow::Result<BrowserClient>;
    /// On-disk profile directory, if the backend exposes one.
    async fn profile_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
    async fn stop(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait BrowserLauncher: Send + Sync {
    async fn launch(&self, config: BrowserConfig) -> anyhow::Result<Box<dyn ManagedBrowser>>;
    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any;
}

pub(crate) struct RealBrowser {
    browser: std::sync::Arc<cdp_browser_lite::Browser>,
    id: cdp_browser_lite::BrowserId,
    pool: std::sync::Arc<cdp_browser_lite::BrowserPool>,
}

#[async_trait]
impl ManagedBrowser for RealBrowser {
    async fn resolved_port(&self) -> u16 {
        self.browser.debug_address().await.1
    }

    async fn is_alive(&self) -> bool {
        self.browser.is_alive().await
    }

    async fn client(&self) -> anyhow::Result<CdpClient> {
        self.browser
            .client()
            .await
            .map_err(|e: BrowserError| anyhow::anyhow!(e.to_string()))
    }

    async fn browser_client(&self) -> anyhow::Result<BrowserClient> {
        self.browser
            .browser_client()
            .await
            .map_err(|e: BrowserError| anyhow::anyhow!(e.to_string()))
    }

    async fn profile_dir(&self) -> Option<std::path::PathBuf> {
        self.browser.profile_dir().await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.pool
            .close(self.id)
            .await
            .map_err(|e: BrowserError| anyhow::anyhow!(e.to_string()))
    }
}

pub(crate) struct RealLauncher {
    pub(crate) pool: std::sync::Arc<cdp_browser_lite::BrowserPool>,
}

#[async_trait]
impl BrowserLauncher for RealLauncher {
    async fn launch(&self, config: BrowserConfig) -> anyhow::Result<Box<dyn ManagedBrowser>> {
        let id = self
            .pool
            .open(config)
            .await
            .map_err(|e: BrowserError| anyhow::anyhow!(e.to_string()))?;
        let browser = self.pool.get(id).await;
        let Some(browser) = browser else {
            return Err(anyhow::anyhow!("browser entry missing for {:?}", id));
        };
        Ok(Box::new(RealBrowser {
            browser,
            id,
            pool: self.pool.clone(),
        }))
    }

    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
pub(crate) struct FakeBrowser {
    pub(crate) resolved_port: u16,
    pub(crate) alive: std::sync::atomic::AtomicBool,
    pub(crate) stopped: std::sync::atomic::AtomicBool,
    pub(crate) stop_calls: std::sync::atomic::AtomicU32,
}

#[cfg(test)]
impl FakeBrowser {
    pub(crate) fn new(resolved_port: u16) -> Self {
        Self {
            resolved_port,
            alive: std::sync::atomic::AtomicBool::new(true),
            stopped: std::sync::atomic::AtomicBool::new(false),
            stop_calls: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub(crate) fn kill(&self) {
        self.alive.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
#[async_trait]
impl ManagedBrowser for FakeBrowser {
    async fn resolved_port(&self) -> u16 {
        self.resolved_port
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn client(&self) -> anyhow::Result<CdpClient> {
        Err(anyhow::anyhow!(
            "FakeBrowser::client is not implemented; tests should not reach this path"
        ))
    }

    async fn browser_client(&self) -> anyhow::Result<BrowserClient> {
        Err(anyhow::anyhow!(
            "FakeBrowser::browser_client is not implemented; tests should not reach this path"
        ))
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.stop_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct FakeLauncher {
    pub(crate) configs: std::sync::Mutex<Vec<BrowserConfig>>,
    pub(crate) next: std::sync::Mutex<std::collections::VecDeque<FakeBrowser>>,
    pub(crate) launch_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) launched: std::sync::Mutex<Vec<std::sync::Arc<FakeBrowser>>>,
}

#[cfg(test)]
impl FakeLauncher {
    pub(crate) fn new(initial: FakeBrowser) -> Self {
        Self {
            configs: std::sync::Mutex::new(Vec::new()),
            next: std::sync::Mutex::new(std::collections::VecDeque::from(vec![initial])),
            launch_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            launched: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn script(sequence: Vec<FakeBrowser>) -> Self {
        Self {
            configs: std::sync::Mutex::new(Vec::new()),
            next: std::sync::Mutex::new(std::collections::VecDeque::from(sequence)),
            launch_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            launched: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl BrowserLauncher for FakeLauncher {
    async fn launch(&self, config: BrowserConfig) -> anyhow::Result<Box<dyn ManagedBrowser>> {
        self.configs.lock().unwrap().push(config);
        self.launch_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let next = self
            .next
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("FakeLauncher: no scripted FakeBrowser left"))?;
        let arc = std::sync::Arc::new(next);
        self.launched.lock().unwrap().push(arc.clone());
        Ok(Box::new(FakeBrowserHandle(arc)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
struct FakeBrowserHandle(std::sync::Arc<FakeBrowser>);

#[cfg(test)]
#[async_trait]
impl ManagedBrowser for FakeBrowserHandle {
    async fn resolved_port(&self) -> u16 {
        self.0.resolved_port().await
    }

    async fn is_alive(&self) -> bool {
        self.0.is_alive().await
    }

    async fn client(&self) -> anyhow::Result<CdpClient> {
        Err(anyhow::anyhow!(
            "FakeBrowserHandle::client is not implemented; tests should not reach this path"
        ))
    }

    async fn browser_client(&self) -> anyhow::Result<BrowserClient> {
        self.0.browser_client().await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.0.stop().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::chrome_instance::ChromeManager;
    use crate::chrome_mcp_handler::chrome_instance::launch::LaunchParams;

    fn params(port: u16) -> LaunchParams {
        LaunchParams::new("127.0.0.1".into(), port, false, true, false)
    }

    #[tokio::test]
    async fn given_no_browser_when_ensure_instance_then_launches_once() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9222);
    }

    #[tokio::test]
    async fn given_live_browser_when_ensure_instance_then_does_not_relaunch() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        mgr.ensure_instance().await.unwrap();
        let fake: &FakeLauncher = mgr
            .launcher
            .as_any()
            .downcast_ref::<FakeLauncher>()
            .expect("launcher must be the FakeLauncher");
        assert_eq!(
            fake.launch_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn given_dead_browser_when_ensure_instance_then_relaunches() {
        let launcher = FakeLauncher::script(vec![FakeBrowser::new(9222), FakeBrowser::new(9333)]);
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9222);
        let first = {
            let fake: &FakeLauncher = mgr
                .launcher
                .as_any()
                .downcast_ref::<FakeLauncher>()
                .expect("launcher must be the FakeLauncher");
            fake.launched.lock().unwrap().first().cloned().unwrap()
        };
        first.kill();
        mgr.ensure_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9333);
    }

    #[tokio::test]
    async fn given_running_browser_when_stop_instance_then_browser_is_dropped() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        mgr.stop_instance().await.unwrap();
        assert!(mgr.browser.is_none());
    }

    #[tokio::test]
    async fn given_no_browser_when_stop_instance_then_succeeds() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.stop_instance().await.unwrap();
    }

    #[tokio::test]
    async fn given_stopped_browser_when_ensure_instance_then_launches_fresh_instance() {
        let launcher = FakeLauncher::script(vec![FakeBrowser::new(9222), FakeBrowser::new(9444)]);
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        mgr.stop_instance().await.unwrap();
        mgr.ensure_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9444);
    }

    #[tokio::test]
    async fn given_reassigned_port_when_get_port_then_returns_resolved_port() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9333));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9333);
    }

    #[tokio::test]
    async fn given_no_browser_when_get_port_then_returns_configured_port() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        assert_eq!(mgr.get_port(), 9222);
    }

    #[tokio::test]
    async fn given_stopped_browser_when_get_port_then_returns_configured_port() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.ensure_instance().await.unwrap();
        mgr.stop_instance().await.unwrap();
        assert_eq!(mgr.get_port(), 9222);
    }

    #[tokio::test]
    async fn given_proxy_set_when_ensure_instance_then_config_carries_proxy() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mut mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        mgr.set_proxy(Some("http://proxy.example.com:8080".into()));
        mgr.ensure_instance().await.unwrap();
    }

    #[tokio::test]
    async fn given_no_browser_when_client_requested_then_errors() {
        let launcher = FakeLauncher::new(FakeBrowser::new(9222));
        let mgr = CdpBrowserManager::new(params(9222), Box::new(launcher));
        let res = mgr.client().await;
        assert!(res.is_err(), "client() with no browser must error");
    }
}
