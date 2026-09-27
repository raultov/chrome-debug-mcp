pub mod cdp_domains;
pub mod chrome_instance;
mod schema_compat;

// use cdp_domains::debugger;
use cdp_domains::custom::get_custom_events::GetCustomEventsTool;
use cdp_domains::custom::send_cdp_command::SendCdpCommandTool;
use cdp_domains::debugger::evaluate_on_call_frame::EvaluateOnCallFrameTool;
use cdp_domains::debugger::pause_on_load::PauseOnLoadTool;
use cdp_domains::debugger::remove_breakpoint::RemoveBreakpointTool;
use cdp_domains::debugger::resume::ResumeTool;
use cdp_domains::debugger::search_scripts::SearchScriptsTool;
use cdp_domains::debugger::set_breakpoint::SetBreakpointTool;
use cdp_domains::debugger::step_over::StepOverTool;
use cdp_domains::fetch::enable_proxy_auth::EnableProxyAuthTool;
use cdp_domains::input::click_element::ClickElementTool;
use cdp_domains::input::fill_input::FillInputTool;
use cdp_domains::log::get_console_logs::GetConsoleLogsTool;
use cdp_domains::network::get_network_logs::GetNetworkLogsTool;
use cdp_domains::page::capture_screenshot::CaptureScreenshotTool;
use cdp_domains::page::navigate::NavigateTool;
use cdp_domains::page::reload::ReloadTool;
use cdp_domains::page::scroll::ScrollTool;
use cdp_domains::performance::get_performance_metrics::GetPerformanceMetricsTool;
use cdp_domains::runtime::evaluate_js::EvaluateJsTool;
use cdp_domains::runtime::inspect_dom::InspectDomTool;
use cdp_domains::tracing::profile_page_performance::ProfilePagePerformanceTool;
use chrome_instance::close_instance::CloseInstanceTool;
use chrome_instance::close_tab::CloseTabTool;
use chrome_instance::list_instances::ListInstancesTool;
use chrome_instance::list_tabs::ListTabsTool;
use chrome_instance::open_instance::OpenInstanceTool;
use chrome_instance::open_tab::OpenTabTool;
use chrome_instance::restart_chrome::RestartChromeTool;
use chrome_instance::stop_chrome::StopChromeTool;
use chrome_instance::switch_tab::SwitchTabTool;

use async_trait::async_trait;
use cdp_browser_lite::CdpClient;
use rust_mcp_sdk::{McpServer, mcp_server::ServerHandler, schema::*};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub(crate) struct ScriptInfo {
    pub hash: String,
    pub start_line: i32,
    pub start_column: i32,
}

#[derive(Default)]
pub(crate) struct DebuggerState {
    pub scripts: std::collections::HashMap<String, ScriptInfo>,
    pub paused_call_frame_id: Option<String>,
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub resource_type: Option<String>,
    pub request_headers: Option<serde_json::Value>,
    pub request_post_data: Option<String>,
    pub response_status: Option<i64>,
    pub response_status_text: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub response_body: Option<String>,
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct WebSocketFrame {
    pub url: String,
    pub payload_data: String,
    pub is_sent: bool,
}

pub(crate) const MAX_REQUESTS_PER_NAVIGATION: usize = 1000;
pub(crate) const MAX_NAVIGATIONS_RETAINED: usize = 3;
pub(crate) const MAX_WS_FRAMES_PER_CONNECTION: usize = 500;
pub(crate) const MAX_CONSOLE_MESSAGES: usize = 1000;

#[derive(Default, Clone)]
pub(crate) struct NavigationBucket {
    pub requests: std::collections::HashMap<String, NetworkRequest>,
    pub order: std::collections::VecDeque<String>,
}

pub(crate) struct NetworkState {
    pub navigations: std::collections::VecDeque<NavigationBucket>,
    pub ws_connections: std::collections::HashMap<String, String>,
    pub ws_frames: std::collections::HashMap<String, std::collections::VecDeque<WebSocketFrame>>,
}

impl Default for NetworkState {
    fn default() -> Self {
        let mut navigations = std::collections::VecDeque::new();
        navigations.push_back(NavigationBucket::default());
        Self {
            navigations,
            ws_connections: std::collections::HashMap::new(),
            ws_frames: std::collections::HashMap::new(),
        }
    }
}

impl NetworkState {
    pub(crate) fn insert_request(&mut self, id: String, req: NetworkRequest) {
        if self.navigations.is_empty() {
            self.navigations.push_back(NavigationBucket::default());
        }
        if let Some(bucket) = self.navigations.front_mut() {
            bucket.order.push_back(id.clone());
            bucket.requests.insert(id, req);
            if bucket.order.len() > MAX_REQUESTS_PER_NAVIGATION
                && let Some(oldest_id) = bucket.order.pop_front()
            {
                bucket.requests.remove(&oldest_id);
            }
        }
    }

    pub(crate) fn find_request_mut(&mut self, id: &str) -> Option<&mut NetworkRequest> {
        for bucket in &mut self.navigations {
            if let Some(req) = bucket.requests.get_mut(id) {
                return Some(req);
            }
        }
        None
    }

    pub(crate) fn split_after_navigation(&mut self) {
        self.navigations.push_front(NavigationBucket::default());
        self.navigations.truncate(MAX_NAVIGATIONS_RETAINED);
    }

    pub(crate) fn push_ws_frame(&mut self, id: String, frame: WebSocketFrame) {
        let frames = self.ws_frames.entry(id).or_default();
        frames.push_back(frame);
        if frames.len() > MAX_WS_FRAMES_PER_CONNECTION {
            frames.pop_front();
        }
    }

    pub(crate) fn iter_all(&self) -> std::collections::HashMap<String, NetworkRequest> {
        let mut all = std::collections::HashMap::new();
        for bucket in self.navigations.iter().rev() {
            for (id, req) in &bucket.requests {
                all.insert(id.clone(), req.clone());
            }
        }
        all
    }

    pub(crate) fn iter_current(&self) -> std::collections::HashMap<String, NetworkRequest> {
        self.navigations
            .front()
            .map(|b| b.requests.clone())
            .unwrap_or_default()
    }

    pub(crate) fn current_ids(&self) -> std::collections::HashSet<String> {
        self.navigations
            .front()
            .map(|b| b.requests.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn clear_all(&mut self) {
        self.navigations.clear();
        self.navigations.push_back(NavigationBucket::default());
        self.ws_connections.clear();
        self.ws_frames.clear();
    }
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct CustomEvent {
    pub method: String,
    pub params: serde_json::Value,
    pub timestamp: String,
}

#[derive(Default)]
pub(crate) struct CustomState {
    pub events: std::collections::VecDeque<CustomEvent>,
    pub active_domains: std::collections::HashSet<String>,
    pub handles: cdp_domains::event_pump::ListenerHandles,
}

pub(crate) const MIN_SUPPORTED_CHROME_MAJOR: u32 = 120;

#[derive(Clone, Debug, Default, ::serde::Serialize, ::serde::Deserialize)]
pub struct BrowserVersion {
    pub product: Option<String>,
    pub revision: Option<String>,
    pub protocol_version: Option<String>,
    pub js_version: Option<String>,
}

pub(crate) fn parse_chrome_major(product: &str) -> Option<u32> {
    let prefix = if product.starts_with("HeadlessChrome/") {
        "HeadlessChrome/"
    } else if product.starts_with("Chrome/") {
        "Chrome/"
    } else {
        return None;
    };

    let rest = &product[prefix.len()..];
    let major_str = rest.split('.').next()?;
    major_str.parse::<u32>().ok()
}

pub(crate) struct BrowserSession {
    pub(crate) client: Arc<Mutex<Option<CdpClient>>>,
    pub(crate) debugger_state: Arc<Mutex<DebuggerState>>,
    pub(crate) network_state: Arc<Mutex<NetworkState>>,
    pub(crate) log_state: Arc<Mutex<cdp_domains::log::LogState>>,
    pub(crate) tracing_state: Arc<Mutex<cdp_domains::tracing::TracingState>>,
    pub(crate) custom_state: Arc<Mutex<CustomState>>,
    pub(crate) webmcp_state: Arc<Mutex<cdp_domains::webmcp::WebmcpState>>,
    pub(crate) chrome_manager: Arc<Mutex<dyn chrome_instance::ChromeManager>>,
    pub(crate) tabs: Arc<std::sync::RwLock<chrome_instance::tab_registry::TabRegistry>>,
    pub(crate) session_listeners: Arc<Mutex<cdp_domains::event_pump::ListenerHandles>>,
    pub(crate) browser_version: Arc<Mutex<Option<BrowserVersion>>>,
}

impl BrowserSession {
    /// Resets the cached CDP client and clears the tab registry so that the
    /// next tool call reconnects to a freshly launched Chrome instance.
    pub(crate) async fn reset_connection_state(&self) {
        *self.client.lock().await = None;
        *self.session_listeners.lock().await = cdp_domains::event_pump::ListenerHandles::default();
        *self.browser_version.lock().await = None;
        cdp_domains::custom::clear_custom_listeners(&self.custom_state).await;
        self.tabs.write().unwrap().clear();
    }

    /// Returns the browser-level CDP client used for tab management.
    pub(crate) async fn browser_client(
        &self,
    ) -> std::result::Result<cdp_browser_lite::BrowserClient, CallToolError> {
        let manager = self.chrome_manager.lock().await;
        manager.browser_client().await.map_err(|e| {
            CallToolError::from_message(format!("Failed to obtain browser client: {}", e))
        })
    }

    pub(crate) async fn get_or_connect(
        &self,
    ) -> std::result::Result<tokio::sync::MutexGuard<'_, Option<CdpClient>>, CallToolError> {
        // First ensure instance is running
        {
            let mut manager = self.chrome_manager.lock().await;
            manager.ensure_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to ensure Chrome instance: {}", e))
            })?;
        }

        let mut client_lock = self.client.lock().await;
        if client_lock.is_none() {
            let client_res = {
                let manager = self.chrome_manager.lock().await;
                manager.client().await
            };
            match client_res {
                Ok(client) => {
                    let _ = client
                        .send_raw_command("Runtime.enable", cdp_browser_lite::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Page.enable", cdp_browser_lite::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Network.enable", cdp_browser_lite::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Log.enable", cdp_browser_lite::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Performance.enable", cdp_browser_lite::NoParams)
                        .await;

                    if let Ok(ver_res) = client
                        .send_raw_command("Browser.getVersion", cdp_browser_lite::NoParams)
                        .await
                        && let Some(res) = ver_res.result
                    {
                        let product = res
                            .get("product")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let revision = res
                            .get("revision")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let protocol_version = res
                            .get("protocolVersion")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let js_version = res
                            .get("jsVersion")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        *self.browser_version.lock().await = Some(BrowserVersion {
                            product,
                            revision,
                            protocol_version,
                            js_version,
                        });
                    }

                    let has_webmcp = {
                        let manager = self.chrome_manager.lock().await;
                        manager
                            .features()
                            .contains(&chrome_instance::launch::ChromeFeature::WebMcp)
                    };
                    if has_webmcp {
                        let enable_res = client
                            .send_raw_command("WebMCP.enable", cdp_browser_lite::NoParams)
                            .await;
                        let mut webmcp_st = self.webmcp_state.lock().await;
                        if enable_res.is_ok() {
                            webmcp_st.availability =
                                cdp_domains::webmcp::WebmcpAvailability::Enabled;
                        } else {
                            webmcp_st.availability =
                                cdp_domains::webmcp::WebmcpAvailability::Unsupported;
                        }
                    } else {
                        let mut webmcp_st = self.webmcp_state.lock().await;
                        webmcp_st.availability =
                            cdp_domains::webmcp::WebmcpAvailability::NotRequested;
                    }

                    let target = cdp_domains::cdp_target::CdpTarget::Client(client.clone());

                    let mut handles = cdp_domains::event_pump::ListenerHandles::default();
                    handles.push(cdp_domains::debugger::start_debugger_listener(
                        &target,
                        self.debugger_state.clone(),
                    ));

                    handles.push(cdp_domains::network::start_network_listener(
                        &target,
                        self.network_state.clone(),
                    ));

                    handles.push(cdp_domains::page::start_page_listener(
                        &target,
                        self.network_state.clone(),
                    ));

                    handles.absorb(cdp_domains::log::start_log_listener(
                        &target,
                        self.log_state.clone(),
                    ));
                    handles.push(cdp_domains::tracing::start_tracing_listener(
                        &target,
                        self.tracing_state.clone(),
                    ));
                    if has_webmcp {
                        handles.push(cdp_domains::webmcp::start_webmcp_listener(
                            &target,
                            self.webmcp_state.clone(),
                        ));
                    }

                    let _ = client
                        .send_raw_command("Debugger.enable", cdp_browser_lite::NoParams)
                        .await;

                    // Start the tab lifecycle listener (Target.*)
                    if let Ok(browser_client) = {
                        let manager = self.chrome_manager.lock().await;
                        manager.browser_client().await
                    } {
                        handles.push(
                            chrome_instance::tab_lifecycle::start_tab_lifecycle_listener(
                                browser_client,
                                self.tabs.clone(),
                            ),
                        );
                    }

                    *self.session_listeners.lock().await = handles;

                    *client_lock = Some(client);
                }
                Err(e) => {
                    return Err(CallToolError::from_message(format!(
                        "Failed to connect to Chrome: {}",
                        e
                    )));
                }
            }
        }
        Ok(client_lock)
    }

    pub(crate) async fn target(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<cdp_domains::cdp_target::CdpTarget, CallToolError> {
        enum Lookup {
            Found(cdp_domains::cdp_target::CdpTarget),
            NotFound(String),
            FallbackToDefault,
        }

        let lookup = {
            let registry = self.tabs.read().unwrap();
            if let Some(id) = tab_id {
                if let Some(entry) = registry.tabs.get(&id) {
                    Lookup::Found(cdp_domains::cdp_target::CdpTarget::Tab(entry.tab.clone()))
                } else {
                    Lookup::NotFound(id)
                }
            } else if let Some(ref active_id) = registry.active_tab_id
                && let Some(entry) = registry.tabs.get(active_id)
            {
                Lookup::Found(cdp_domains::cdp_target::CdpTarget::Tab(entry.tab.clone()))
            } else {
                Lookup::FallbackToDefault
            }
        };

        match lookup {
            Lookup::Found(target) => return Ok(target),
            Lookup::NotFound(id) => {
                let registry = self.tabs.read().unwrap();
                let available: Vec<_> = registry.tabs.keys().cloned().collect();
                let avail_str = if available.is_empty() {
                    "none (if Chrome was restarted, previous tab IDs are no longer valid)"
                        .to_string()
                } else {
                    available.join(", ")
                };
                return Err(CallToolError::from_message(format!(
                    "Tab with ID '{}' not found in this session. Available tabs: {}. Use 'list_tabs' to discover current tab IDs.",
                    id, avail_str
                )));
            }
            Lookup::FallbackToDefault => {}
        }

        let client_guard = self.get_or_connect().await?;
        if let Some(ref client) = *client_guard {
            Ok(cdp_domains::cdp_target::CdpTarget::Client(client.clone()))
        } else {
            Err(CallToolError::from_message(
                "Failed to retrieve target: no active tabs or default connection available"
                    .to_string(),
            ))
        }
    }

    fn tab_or_fallback_state<T, F, G>(
        &self,
        tab_id: Option<String>,
        tab_extractor: F,
        fallback_extractor: G,
    ) -> std::result::Result<Arc<Mutex<T>>, CallToolError>
    where
        F: FnOnce(&chrome_instance::tab_registry::TabEntry) -> Arc<Mutex<T>>,
        G: FnOnce(&Self) -> Arc<Mutex<T>>,
    {
        let registry = self.tabs.read().unwrap();
        if let Some(id) = tab_id {
            return if let Some(entry) = registry.tabs.get(&id) {
                Ok(tab_extractor(entry))
            } else {
                let available: Vec<_> = registry.tabs.keys().cloned().collect();
                let avail_str = if available.is_empty() {
                    "none (if Chrome was restarted, previous tab IDs are no longer valid)"
                        .to_string()
                } else {
                    available.join(", ")
                };
                Err(CallToolError::from_message(format!(
                    "Tab '{}' not found. Available tabs: {}. Use 'list_tabs' to discover current tab IDs.",
                    id, avail_str
                )))
            };
        }
        if let Some(ref active_id) = registry.active_tab_id
            && let Some(entry) = registry.tabs.get(active_id)
        {
            return Ok(tab_extractor(entry));
        }
        Ok(fallback_extractor(self))
    }

    #[expect(
        dead_code,
        reason = "Debugger state retrieval wired to tools starting in Phase 5 E2E"
    )]
    pub(crate) fn debugger_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<DebuggerState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.debugger_state.clone(),
            |s| s.debugger_state.clone(),
        )
    }

    pub(crate) fn network_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<NetworkState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.network_state.clone(),
            |s| s.network_state.clone(),
        )
    }

    pub(crate) fn log_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<cdp_domains::log::LogState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.log_state.clone(),
            |s| s.log_state.clone(),
        )
    }

    #[expect(
        dead_code,
        reason = "Tracing state retrieval wired to tools starting in Phase 5 E2E"
    )]
    pub(crate) fn tracing_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<cdp_domains::tracing::TracingState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.tracing_state.clone(),
            |s| s.tracing_state.clone(),
        )
    }

    pub(crate) fn custom_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<CustomState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.custom_state.clone(),
            |s| s.custom_state.clone(),
        )
    }

    pub(crate) fn webmcp_state(
        &self,
        tab_id: Option<String>,
    ) -> std::result::Result<Arc<Mutex<cdp_domains::webmcp::WebmcpState>>, CallToolError> {
        self.tab_or_fallback_state(
            tab_id,
            |entry| entry.webmcp_state.clone(),
            |s| s.webmcp_state.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct HandlerParams {
    pub host: String,
    pub port: u16,
    pub local_only: bool,
    pub enable_automation: bool,
    pub headless: bool,
    pub user_profile: bool,
    pub allow_cookie_import: bool,
    pub proxy_server: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
}

impl Default for HandlerParams {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9222,
            local_only: false,
            enable_automation: false,
            headless: false,
            user_profile: false,
            allow_cookie_import: false,
            proxy_server: None,
            proxy_username: None,
            proxy_password: None,
        }
    }
}

pub struct ChromeMcpHandler {
    pub(crate) default_session: Arc<BrowserSession>,
    pub(crate) registry: Arc<chrome_instance::registry::Registry>,
    pub(crate) pool: Arc<cdp_browser_lite::BrowserPool>,
    pub(crate) base_params: chrome_instance::launch::LaunchParams,
    pub(crate) local_only: bool,
    pub(crate) allow_cookie_import: bool,
    pub(crate) proxy_server: Option<String>,
    pub(crate) proxy_username: Option<String>,
    pub(crate) proxy_password: Option<String>,
    pub(crate) is_test: bool,
}

impl ChromeMcpHandler {
    pub async fn session(
        &self,
        id: Option<String>,
    ) -> std::result::Result<Arc<BrowserSession>, CallToolError> {
        let id = id.unwrap_or_else(|| "default".to_string());
        if let Some(session) = self.registry.get_session(&id) {
            Ok(session)
        } else if id == "default" {
            // Lazily add the default session
            let default_features = {
                let mgr = self.default_session.chrome_manager.lock().await;
                mgr.features()
                    .iter()
                    .map(|f| f.as_name().to_string())
                    .collect::<Vec<_>>()
            };
            let desc = chrome_instance::registry::InstanceDescriptor {
                id: "default".to_string(),
                label: None,
                host: "127.0.0.1".to_string(),
                port: self.base_params.configured_port(),
                profile_dir: None,
                features: default_features,
                is_default: true,
                browser_version: None,
            };
            self.registry
                .add_session(desc, self.default_session.clone())
                .map_err(|e| {
                    CallToolError::from_message(format!(
                        "Failed to register default session: {}",
                        e
                    ))
                })?;
            Ok(self.default_session.clone())
        } else {
            let available: Vec<_> = self
                .registry
                .list_descriptors()
                .into_iter()
                .map(|d| d.id)
                .collect();
            Err(CallToolError::from_message(format!(
                "Instance id '{}' not found. Available instances: {}. Use 'list_instances' to discover instance IDs.",
                id,
                available.join(", ")
            )))
        }
    }

    pub fn new_with_params(options: HandlerParams) -> Self {
        let port = options.port;
        let mut params = chrome_instance::launch::LaunchParams::new(
            options.host,
            options.port,
            options.enable_automation,
            options.headless,
            options.user_profile,
        );
        if let Some(proxy) = &options.proxy_server {
            params.set_proxy(Some(proxy.clone()));
        }
        let pool = Arc::new(cdp_browser_lite::BrowserPool::new());
        let registry = Arc::new(chrome_instance::registry::Registry::new(8)); // max 8 instances

        let manager = chrome_instance::cdp_browser_manager::CdpBrowserManager::new(
            params.clone(),
            Box::new(chrome_instance::cdp_browser_manager::RealLauncher { pool: pool.clone() }),
        );
        let session = Arc::new(BrowserSession {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(cdp_domains::log::LogState::default())),
            tracing_state: Arc::new(Mutex::new(cdp_domains::tracing::TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            webmcp_state: Arc::new(Mutex::new(cdp_domains::webmcp::WebmcpState::default())),
            chrome_manager: Arc::new(Mutex::new(manager)),
            tabs: Arc::new(std::sync::RwLock::new(
                chrome_instance::tab_registry::TabRegistry::new(16),
            )),
            session_listeners: Arc::new(Mutex::new(
                cdp_domains::event_pump::ListenerHandles::default(),
            )),
            browser_version: Arc::new(Mutex::new(None)),
        });

        let default_features = {
            params
                .features()
                .iter()
                .map(|f| f.as_name().to_string())
                .collect::<Vec<_>>()
        };
        let desc = chrome_instance::registry::InstanceDescriptor {
            id: "default".to_string(),
            label: None,
            host: "127.0.0.1".to_string(), // approximation
            port,
            profile_dir: None, // lazy
            features: default_features,
            is_default: true,
            browser_version: None,
        };
        registry.register_descriptor(desc);

        Self {
            default_session: session,
            registry,
            pool,
            base_params: params,
            local_only: options.local_only,
            allow_cookie_import: options.allow_cookie_import,
            proxy_server: options.proxy_server,
            proxy_username: options.proxy_username,
            proxy_password: options.proxy_password,
            is_test: false,
        }
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        Self::new_test_with_port(9999)
    }

    #[cfg(test)]
    pub fn new_test_with_port(port: u16) -> Self {
        let params = chrome_instance::launch::LaunchParams::new(
            "127.0.0.1".into(),
            port,
            false,
            false,
            false,
        );
        let pool = Arc::new(cdp_browser_lite::BrowserPool::new());
        let registry = Arc::new(chrome_instance::registry::Registry::new(8));

        let session = Arc::new(BrowserSession {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(cdp_domains::log::LogState::default())),
            tracing_state: Arc::new(Mutex::new(cdp_domains::tracing::TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            webmcp_state: Arc::new(Mutex::new(cdp_domains::webmcp::WebmcpState::default())),
            chrome_manager: Arc::new(Mutex::new(chrome_instance::MockChromeManager::new(port))),
            tabs: Arc::new(std::sync::RwLock::new(
                chrome_instance::tab_registry::TabRegistry::new(16),
            )),
            session_listeners: Arc::new(Mutex::new(
                cdp_domains::event_pump::ListenerHandles::default(),
            )),
            browser_version: Arc::new(Mutex::new(None)),
        });

        let desc = chrome_instance::registry::InstanceDescriptor {
            id: "default".to_string(),
            label: None,
            host: "127.0.0.1".to_string(),
            port,
            profile_dir: None,
            features: vec![],
            is_default: true,
            browser_version: None,
        };
        registry.register_descriptor(desc);

        Self {
            default_session: session,
            registry,
            pool,
            base_params: params,
            local_only: false,
            allow_cookie_import: false,
            proxy_server: None,
            proxy_username: None,
            proxy_password: None,
            is_test: true,
        }
    }
}

impl Default for ChromeMcpHandler {
    fn default() -> Self {
        Self::new_with_params(HandlerParams::default())
    }
}

pub(crate) fn is_local_address(url_str: &str) -> bool {
    let url = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return false,
    };

    let host = match url.host_str() {
        Some(h) => h,
        None => return false,
    };

    // localhost, 127.0.0.1, [::1], 192.168.x.x, or .local
    if host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host.ends_with(".local")
        || host.starts_with("192.168.")
    {
        return true;
    }

    false
}

pub(crate) fn extract_from_value<'a>(
    value: &'a Option<serde_json::Value>,
    param_name: &str,
) -> Option<&'a str> {
    value
        .as_ref()
        .and_then(|p| p.get(param_name))
        .and_then(|v| v.as_str())
}

pub(crate) fn find_line_column(source: &str, pattern: &str) -> Option<(u32, u32)> {
    let byte_index = source.find(pattern)?;
    let prefix = &source[..byte_index];
    let line_number = prefix.lines().count().saturating_sub(1) as u32;
    let column_number = prefix.lines().last().map(|line| line.len()).unwrap_or(0) as u32;

    Some((line_number, column_number))
}

#[async_trait]
impl ServerHandler for ChromeMcpHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        let mut tools = vec![
            CaptureScreenshotTool::tool(),
            ClickElementTool::tool(),
            FillInputTool::tool(),
            EvaluateJsTool::tool(),
            NavigateTool::tool(),
            InspectDomTool::tool(),
            PauseOnLoadTool::tool(),
            StepOverTool::tool(),
            ResumeTool::tool(),
            SearchScriptsTool::tool(),
            SetBreakpointTool::tool(),
            EvaluateOnCallFrameTool::tool(),
            ReloadTool::tool(),
            ScrollTool::tool(),
            RemoveBreakpointTool::tool(),
            RestartChromeTool::tool(),
            StopChromeTool::tool(),
            OpenInstanceTool::tool(),
            ListInstancesTool::tool(),
            CloseInstanceTool::tool(),
            OpenTabTool::tool(),
            ListTabsTool::tool(),
            CloseTabTool::tool(),
            SwitchTabTool::tool(),
            GetNetworkLogsTool::tool(),
            GetConsoleLogsTool::tool(),
            GetPerformanceMetricsTool::tool(),
            ProfilePagePerformanceTool::tool(),
            SendCdpCommandTool::tool(),
            GetCustomEventsTool::tool(),
            cdp_domains::webmcp::ListWebmcpToolsTool::tool(),
            cdp_domains::webmcp::InvokeWebmcpToolTool::tool(),
            cdp_domains::webmcp::GetWebmcpInvocationTool::tool(),
            cdp_domains::webmcp::ListWebmcpInvocationsTool::tool(),
        ];

        if self.proxy_server.is_some() {
            tools.push(EnableProxyAuthTool::tool());
        }

        Ok(ListToolsResult {
            tools: tools
                .into_iter()
                .map(|t| schema_compat::normalize_tool_with_options(t, self.allow_cookie_import))
                .collect(),
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        if params.name == "capture_screenshot" {
            CaptureScreenshotTool::handle(params, self).await
        } else if params.name == "click_element" {
            ClickElementTool::handle(params, self).await
        } else if params.name == "fill_input" {
            FillInputTool::handle(params, self).await
        } else if params.name == "evaluate_js" {
            EvaluateJsTool::handle(params, self).await
        } else if params.name == "navigate" {
            NavigateTool::handle(params, self).await
        } else if params.name == "inspect_dom" {
            InspectDomTool::handle(params, self).await
        } else if params.name == "pause_on_load" {
            PauseOnLoadTool::handle(params, self).await
        } else if params.name == "step_over" {
            StepOverTool::handle(params, self).await
        } else if params.name == "resume" {
            ResumeTool::handle(params, self).await
        } else if params.name == "search_scripts" {
            SearchScriptsTool::handle(params, self).await
        } else if params.name == "set_breakpoint" {
            SetBreakpointTool::handle(params, self).await
        } else if params.name == "evaluate_on_call_frame" {
            EvaluateOnCallFrameTool::handle(params, self).await
        } else if params.name == "remove_breakpoint" {
            RemoveBreakpointTool::handle(params, self).await
        } else if params.name == "reload" {
            ReloadTool::handle(params, self).await
        } else if params.name == "scroll" {
            ScrollTool::handle(params, self).await
        } else if params.name == "restart_chrome" {
            RestartChromeTool::handle(params, self).await
        } else if params.name == "stop_chrome" {
            StopChromeTool::handle(params, self).await
        } else if params.name == "open_instance" {
            OpenInstanceTool::handle(params, self).await
        } else if params.name == "list_instances" {
            ListInstancesTool::handle(params, self).await
        } else if params.name == "close_instance" {
            CloseInstanceTool::handle(params, self).await
        } else if params.name == "open_tab" {
            OpenTabTool::handle(params, self).await
        } else if params.name == "list_tabs" {
            ListTabsTool::handle(params, self).await
        } else if params.name == "close_tab" {
            CloseTabTool::handle(params, self).await
        } else if params.name == "switch_tab" {
            SwitchTabTool::handle(params, self).await
        } else if params.name == "get_network_logs" {
            GetNetworkLogsTool::handle(params, self).await
        } else if params.name == "get_console_logs" {
            GetConsoleLogsTool::handle(params, self).await
        } else if params.name == "get_performance_metrics" {
            GetPerformanceMetricsTool::handle(params, self).await
        } else if params.name == "profile_page_performance" {
            ProfilePagePerformanceTool::handle(params, self).await
        } else if params.name == "enable_proxy_auth" {
            if self.proxy_server.is_none() {
                return Err(CallToolError::from_message(
                    "Tool 'enable_proxy_auth' is disabled. Start chrome-debug-mcp with --proxy-server to enable proxy support.",
                ));
            }
            EnableProxyAuthTool::handle(params, self).await
        } else if params.name == "send_cdp_command" {
            SendCdpCommandTool::handle(params, self).await
        } else if params.name == "get_custom_events" {
            GetCustomEventsTool::handle(params, self).await
        } else if params.name == "webmcp_list_tools" {
            cdp_domains::webmcp::ListWebmcpToolsTool::handle(params, self).await
        } else if params.name == "webmcp_invoke_tool" {
            cdp_domains::webmcp::InvokeWebmcpToolTool::handle(params, self).await
        } else if params.name == "webmcp_get_invocation" {
            cdp_domains::webmcp::GetWebmcpInvocationTool::handle(params, self).await
        } else if params.name == "webmcp_list_invocations" {
            cdp_domains::webmcp::ListWebmcpInvocationsTool::handle(params, self).await
        } else {
            Err(CallToolError::unknown_tool(params.name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct DummyMcpServer {}

    // A dummy implementation of McpServer for testing
    #[async_trait]
    impl McpServer for DummyMcpServer {
        async fn start(self: Arc<Self>) -> rust_mcp_sdk::error::SdkResult<()> {
            Ok(())
        }
        async fn set_client_details(
            &self,
            _client_details: InitializeRequestParams,
        ) -> rust_mcp_sdk::error::SdkResult<()> {
            Ok(())
        }
        fn server_info(&self) -> &InitializeResult {
            unimplemented!()
        }
        fn client_info(&self) -> Option<InitializeRequestParams> {
            None
        }
        async fn auth_info(
            &self,
        ) -> tokio::sync::RwLockReadGuard<'_, Option<rust_mcp_sdk::auth::AuthInfo>> {
            unimplemented!()
        }
        async fn auth_info_cloned(&self) -> Option<rust_mcp_sdk::auth::AuthInfo> {
            None
        }
        async fn update_auth_info(&self, _auth_info: Option<rust_mcp_sdk::auth::AuthInfo>) {}
        async fn wait_for_initialization(&self) {}
        fn task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ServerTaskStore>> {
            None
        }
        fn client_task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ClientTaskStore>> {
            None
        }
        async fn stderr_message(&self, _message: String) -> rust_mcp_sdk::error::SdkResult<()> {
            Ok(())
        }
        fn session_id(&self) -> Option<String> {
            None
        }
        async fn send(
            &self,
            _message: MessageFromServer,
            _request_id: Option<RequestId>,
            _request_timeout: Option<std::time::Duration>,
        ) -> rust_mcp_sdk::error::SdkResult<Option<ClientMessage>> {
            Ok(None)
        }
        async fn send_batch(
            &self,
            _messages: Vec<ServerMessage>,
            _request_timeout: Option<std::time::Duration>,
        ) -> rust_mcp_sdk::error::SdkResult<Option<Vec<ClientMessage>>> {
            Ok(None)
        }
    }
    #[test]
    fn test_is_local_address() {
        // Positive cases
        assert!(is_local_address("http://localhost"));
        assert!(is_local_address("https://localhost:8080"));
        assert!(is_local_address("http://127.0.0.1"));
        assert!(is_local_address("http://127.0.0.1:3000/path"));
        assert!(is_local_address("http://[::1]"));
        assert!(is_local_address("http://[::1]:5173"));
        assert!(is_local_address("http://192.168.1.1"));
        assert!(is_local_address("https://192.168.0.100/admin"));
        assert!(is_local_address("http://mydevice.local"));
        assert!(is_local_address("http://service.local:8000"));

        // Negative cases
        assert!(!is_local_address("http://google.com"));
        assert!(!is_local_address("https://github.com/raultov"));
        assert!(!is_local_address("http://8.8.8.8"));
        assert!(!is_local_address("http://10.0.0.1"));
        assert!(!is_local_address("not a url"));
        assert!(!is_local_address("http://local.com"));
        assert!(!is_local_address("http://192.167.1.1"));
    }

    #[test]
    fn test_extract_from_value() {
        let val = Some(json!({"testKey": "testValue", "numKey": 42}));
        assert_eq!(extract_from_value(&val, "testKey"), Some("testValue"));
        assert_eq!(extract_from_value(&val, "numKey"), None); // As string fails
        assert_eq!(extract_from_value(&val, "missing"), None);
        assert_eq!(extract_from_value(&None, "testKey"), None);
    }

    #[test]
    fn test_find_line_column() {
        let source = "function test() {\n  let a = 1;\n  console.log(a);\n}";

        let (line, col) = find_line_column(source, "let a").unwrap();
        assert_eq!(line, 1);
        assert_eq!(col, 2);

        let (line, col) = find_line_column(source, "console.log").unwrap();
        assert_eq!(line, 2);
        assert_eq!(col, 2);

        let (line, col) = find_line_column(source, "function test").unwrap();
        assert_eq!(line, 0);
        assert_eq!(col, 0);

        assert_eq!(find_line_column(source, "not_found"), None);
    }

    #[test]
    fn test_chrome_mcp_handler_new_with_params() {
        let handler = ChromeMcpHandler::new_with_params(HandlerParams {
            host: "host.docker.internal".into(),
            port: 9222,
            local_only: true,
            enable_automation: true,
            headless: true,
            user_profile: false,
            allow_cookie_import: false,
            ..Default::default()
        });
        assert!(handler.local_only);
        assert!(!handler.allow_cookie_import);
        assert_eq!(handler.proxy_server, None);
    }

    #[test]
    fn test_chrome_mcp_handler_new_with_automation() {
        let handler = ChromeMcpHandler::new_with_params(HandlerParams {
            host: "127.0.0.1".into(),
            port: 9444,
            local_only: true,
            enable_automation: true,
            headless: false,
            user_profile: true,
            allow_cookie_import: false,
            proxy_server: Some("http://proxy.example.com:8080".into()),
            proxy_username: Some("user".into()),
            proxy_password: Some("pass".into()),
        });
        assert!(handler.local_only);
        assert_eq!(
            handler.proxy_server,
            Some("http://proxy.example.com:8080".into())
        );
        assert_eq!(handler.proxy_username, Some("user".into()));
        assert_eq!(handler.proxy_password, Some("pass".into()));
    }

    #[tokio::test]
    async fn test_handle_list_tools_request() {
        let handler = ChromeMcpHandler::new_test();
        let mock_server = Arc::new(DummyMcpServer {});

        let result = handler
            .handle_list_tools_request(None, mock_server.clone())
            .await;

        assert!(result.is_ok());
        let tools = result.unwrap().tools;

        // Without proxy configured, 34 tools are listed (enable_proxy_auth is excluded)
        assert_eq!(tools.len(), 34);
        let tool_names: Vec<String> = tools.into_iter().map(|t| t.name).collect();
        assert!(tool_names.contains(&"scroll".to_string()));
        assert!(tool_names.contains(&"capture_screenshot".to_string()));
        assert!(tool_names.contains(&"click_element".to_string()));
        assert!(tool_names.contains(&"fill_input".to_string()));
        assert!(tool_names.contains(&"evaluate_js".to_string()));
        assert!(tool_names.contains(&"navigate".to_string()));
        assert!(tool_names.contains(&"restart_chrome".to_string()));
        assert!(tool_names.contains(&"stop_chrome".to_string()));
        assert!(tool_names.contains(&"get_console_logs".to_string()));
        assert!(tool_names.contains(&"get_performance_metrics".to_string()));
        assert!(tool_names.contains(&"profile_page_performance".to_string()));
        assert!(!tool_names.contains(&"enable_proxy_auth".to_string()));
    }

    #[tokio::test]
    async fn test_handle_list_tools_request_with_proxy() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.proxy_server = Some("http://proxy.example.com:8080".to_string());
        let mock_server = Arc::new(DummyMcpServer {});

        let result = handler
            .handle_list_tools_request(None, mock_server.clone())
            .await;

        assert!(result.is_ok());
        let tools = result.unwrap().tools;

        // With proxy configured, enable_proxy_auth is included (35 tools)
        assert_eq!(tools.len(), 35);
        let tool_names: Vec<String> = tools.into_iter().map(|t| t.name).collect();
        assert!(tool_names.contains(&"enable_proxy_auth".to_string()));
    }

    /// Gemini rejects a function declaration whose `items` sits next to a
    /// non-scalar `type`, so every published schema must use a single type
    /// name and give any array an `items` subschema.
    #[tokio::test]
    async fn given_listed_tools_when_inspecting_schemas_then_types_are_scalar_and_arrays_are_typed()
    {
        let handler = ChromeMcpHandler::new_test();
        let tools = handler
            .handle_list_tools_request(None, Arc::new(DummyMcpServer {}))
            .await
            .expect("listing tools must succeed")
            .tools;

        fn assert_strict(schema: &serde_json::Value, path: &str) {
            let serde_json::Value::Object(map) = schema else {
                return;
            };
            if let Some(type_) = map.get("type") {
                assert!(
                    type_.is_string(),
                    "{path}.type must be a single type name, got {type_}"
                );
            }
            if map.contains_key("items") {
                assert_eq!(
                    map.get("type").and_then(|t| t.as_str()),
                    Some("array"),
                    "{path} declares items so it must be typed as an array; got {schema}"
                );
            }
            for (key, child) in map {
                assert_strict(child, &format!("{path}.{key}"));
            }
        }

        for tool in tools {
            let schema = serde_json::to_value(&tool.input_schema).unwrap();
            assert_strict(&schema, &tool.name);
        }
    }

    #[tokio::test]
    async fn test_handle_call_tool_request_unknown_tool() {
        let handler = ChromeMcpHandler::new_test();
        let mock_server = Arc::new(DummyMcpServer {});

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "non_existent_tool_123",
            "arguments": {}
        }))
        .unwrap();

        let result = handler.handle_call_tool_request(params, mock_server).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Unknown tool: non_existent_tool_123")
        );
    }

    #[test]
    fn test_parse_chrome_major() {
        assert_eq!(parse_chrome_major("Chrome/152.0.6367.60"), Some(152));
        assert_eq!(parse_chrome_major("HeadlessChrome/151.0.1"), Some(151));
        assert_eq!(parse_chrome_major("Chrome/120.0"), Some(120));
        assert_eq!(parse_chrome_major("MockChrome/1.0"), None);
        assert_eq!(parse_chrome_major("Firefox/110.0"), None);
        assert_eq!(parse_chrome_major(""), None);
    }
}
