pub mod cdp_domains;
pub mod chrome_instance;

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
use chrome_instance::restart_chrome::RestartChromeTool;
use chrome_instance::stop_chrome::StopChromeTool;

use async_trait::async_trait;
use cdp_lite::client::CdpClient;
use rust_mcp_sdk::{McpServer, mcp_server::ServerHandler, schema::*};
use std::sync::Arc;
use std::time::Duration;
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

#[derive(Default)]
pub(crate) struct NetworkState {
    pub requests: std::collections::HashMap<String, NetworkRequest>,
    pub ws_connections: std::collections::HashMap<String, String>,
    pub ws_frames: std::collections::HashMap<String, Vec<WebSocketFrame>>,
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
}

pub struct ChromeMcpHandler {
    pub(crate) client: Arc<Mutex<Option<CdpClient>>>,
    pub(crate) debugger_state: Arc<Mutex<DebuggerState>>,
    pub(crate) network_state: Arc<Mutex<NetworkState>>,
    pub(crate) log_state: Arc<Mutex<cdp_domains::log::LogState>>,
    pub(crate) tracing_state: Arc<Mutex<cdp_domains::tracing::TracingState>>,
    pub(crate) custom_state: Arc<Mutex<CustomState>>,
    pub(crate) chrome_manager: Arc<Mutex<dyn chrome_instance::ChromeManager>>,
    pub(crate) local_only: bool,
}

impl ChromeMcpHandler {
    pub fn new_with_port(port: u16, local_only: bool) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(cdp_domains::log::LogState::default())),
            tracing_state: Arc::new(Mutex::new(cdp_domains::tracing::TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            chrome_manager: Arc::new(Mutex::new(chrome_instance::ChromeInstanceManager::new(
                port,
            ))),
            local_only,
        }
    }

    #[cfg(test)]
    pub fn new_test() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(cdp_domains::log::LogState::default())),
            tracing_state: Arc::new(Mutex::new(cdp_domains::tracing::TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            chrome_manager: Arc::new(Mutex::new(chrome_instance::MockChromeManager::new(9999))),
            local_only: false,
        }
    }
}

impl Default for ChromeMcpHandler {
    fn default() -> Self {
        Self::new_with_port(9222, false)
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

impl ChromeMcpHandler {
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
            let port = {
                let manager = self.chrome_manager.lock().await;
                manager.get_port()
            };

            let addr = format!("127.0.0.1:{}", port);
            match CdpClient::new(&addr, Duration::from_secs(10)).await {
                Ok(mut client) => {
                    let _ = client
                        .send_raw_command("Runtime.enable", cdp_lite::protocol::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Page.enable", cdp_lite::protocol::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Network.enable", cdp_lite::protocol::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Log.enable", cdp_lite::protocol::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Performance.enable", cdp_lite::protocol::NoParams)
                        .await;

                    cdp_domains::debugger::start_debugger_listener(
                        &mut client,
                        self.debugger_state.clone(),
                    );

                    cdp_domains::network::start_network_listener(
                        &mut client,
                        self.network_state.clone(),
                    );

                    cdp_domains::log::start_log_listener(&mut client, self.log_state.clone());
                    cdp_domains::tracing::start_tracing_listener(
                        &mut client,
                        self.tracing_state.clone(),
                    );

                    let _ = client
                        .send_raw_command("Debugger.enable", cdp_lite::protocol::NoParams)
                        .await;

                    *client_lock = Some(client);
                }
                Err(e) => {
                    return Err(CallToolError::from_message(format!(
                        "Failed to connect to Chrome at {}: {}",
                        addr, e
                    )));
                }
            }
        }
        Ok(client_lock)
    }
}

#[async_trait]
impl ServerHandler for ChromeMcpHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: std::sync::Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
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
                GetNetworkLogsTool::tool(),
                GetConsoleLogsTool::tool(),
                GetPerformanceMetricsTool::tool(),
                ProfilePagePerformanceTool::tool(),
                EnableProxyAuthTool::tool(),
                SendCdpCommandTool::tool(),
                GetCustomEventsTool::tool(),
            ],
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: std::sync::Arc<dyn McpServer>,
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
        } else if params.name == "get_network_logs" {
            GetNetworkLogsTool::handle(params, self).await
        } else if params.name == "get_console_logs" {
            GetConsoleLogsTool::handle(params, self).await
        } else if params.name == "get_performance_metrics" {
            GetPerformanceMetricsTool::handle(params, self).await
        } else if params.name == "profile_page_performance" {
            ProfilePagePerformanceTool::handle(params, self).await
        } else if params.name == "enable_proxy_auth" {
            EnableProxyAuthTool::handle(params, self).await
        } else if params.name == "send_cdp_command" {
            SendCdpCommandTool::handle(params, self).await
        } else if params.name == "get_custom_events" {
            GetCustomEventsTool::handle(params, self).await
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
            _client_details: rust_mcp_sdk::schema::InitializeRequestParams,
        ) -> rust_mcp_sdk::error::SdkResult<()> {
            Ok(())
        }
        fn server_info(&self) -> &rust_mcp_sdk::schema::InitializeResult {
            unimplemented!()
        }
        fn client_info(&self) -> Option<rust_mcp_sdk::schema::InitializeRequestParams> {
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
            _message: rust_mcp_sdk::schema::schema_utils::MessageFromServer,
            _request_id: Option<rust_mcp_sdk::schema::RequestId>,
            _request_timeout: Option<std::time::Duration>,
        ) -> rust_mcp_sdk::error::SdkResult<Option<rust_mcp_sdk::schema::schema_utils::ClientMessage>>
        {
            Ok(None)
        }
        async fn send_batch(
            &self,
            _messages: Vec<rust_mcp_sdk::schema::schema_utils::ServerMessage>,
            _request_timeout: Option<std::time::Duration>,
        ) -> rust_mcp_sdk::error::SdkResult<
            Option<Vec<rust_mcp_sdk::schema::schema_utils::ClientMessage>>,
        > {
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

    #[tokio::test]
    async fn test_handle_list_tools_request() {
        let handler = ChromeMcpHandler::new_test();
        let mock_server = Arc::new(DummyMcpServer {});

        let result = handler
            .handle_list_tools_request(None, mock_server.clone())
            .await;

        assert!(result.is_ok());
        let tools = result.unwrap().tools;

        // Ensure all registered tools are present
        assert_eq!(tools.len(), 24);
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
        assert!(tool_names.contains(&"enable_proxy_auth".to_string()));
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
}
