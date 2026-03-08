pub mod cdp_domains;
pub mod chrome_instance;
pub mod connect_chrome;

// use cdp_domains::debugger;
use cdp_domains::debugger::evaluate_on_call_frame::EvaluateOnCallFrameTool;
use cdp_domains::debugger::pause_on_load::PauseOnLoadTool;
use cdp_domains::debugger::remove_breakpoint::RemoveBreakpointTool;
use cdp_domains::debugger::resume::ResumeTool;
use cdp_domains::debugger::search_scripts::SearchScriptsTool;
use cdp_domains::debugger::set_breakpoint::SetBreakpointTool;
use cdp_domains::debugger::step_over::StepOverTool;
use cdp_domains::page::navigate::NavigateTool;
use cdp_domains::page::reload::ReloadTool;
use cdp_domains::runtime::evaluate_js::EvaluateJsTool;
use cdp_domains::runtime::inspect_dom::InspectDomTool;
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

pub struct ChromeMcpHandler {
    pub(crate) client: Arc<Mutex<Option<CdpClient>>>,
    pub(crate) debugger_state: Arc<Mutex<DebuggerState>>,
    pub(crate) chrome_manager: Arc<Mutex<chrome_instance::ChromeInstanceManager>>,
}

impl Default for ChromeMcpHandler {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            chrome_manager: Arc::new(Mutex::new(chrome_instance::ChromeInstanceManager::new(
                9222,
            ))),
        }
    }
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

                    cdp_domains::debugger::start_debugger_listener(
                        &mut client,
                        self.debugger_state.clone(),
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
                RemoveBreakpointTool::tool(),
                RestartChromeTool::tool(),
                StopChromeTool::tool(),
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
        if params.name == "evaluate_js" {
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
        } else if params.name == "restart_chrome" {
            RestartChromeTool::handle(params, self).await
        } else if params.name == "stop_chrome" {
            StopChromeTool::handle(params, self).await
        } else {
            Err(CallToolError::unknown_tool(params.name))
        }
    }
}
