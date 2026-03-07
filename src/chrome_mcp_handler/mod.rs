pub mod connect_chrome;
pub mod debugger;
pub mod page;
pub mod runtime;

use connect_chrome::ConnectChromeTool;
use debugger::pause_on_load::PauseOnLoadTool;
use page::navigate::NavigateTool;
use page::reload::ReloadTool;
use runtime::evaluate_js::EvaluateJsTool;
use runtime::inspect_dom::InspectDomTool;

use debugger::evaluate_on_call_frame::EvaluateOnCallFrameTool;
use debugger::remove_breakpoint::RemoveBreakpointTool;
use debugger::resume::ResumeTool;
use debugger::search_scripts::SearchScriptsTool;
use debugger::set_breakpoint::SetBreakpointTool;
use debugger::step_over::StepOverTool;

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
}

impl Default for ChromeMcpHandler {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
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
        let mut client_lock = self.client.lock().await;
        if client_lock.is_none() {
            match CdpClient::new("127.0.0.1:9222", Duration::from_secs(5)).await {
                Ok(mut client) => {
                    let _ = client
                        .send_raw_command("Runtime.enable", cdp_lite::protocol::NoParams)
                        .await;
                    let _ = client
                        .send_raw_command("Page.enable", cdp_lite::protocol::NoParams)
                        .await;

                    debugger::start_debugger_listener(&mut client, self.debugger_state.clone());

                    let _ = client
                        .send_raw_command("Debugger.enable", cdp_lite::protocol::NoParams)
                        .await;

                    *client_lock = Some(client);
                }
                Err(_) => {
                    return Err(CallToolError::from_message("Not connected to Chrome. Use connect_chrome tool first, or ensure Chrome is running with --remote-debugging-port=9222".to_string()));
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
                ConnectChromeTool::tool(),
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
        if params.name == "connect_chrome" {
            ConnectChromeTool::handle(params, self).await
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
        } else {
            Err(CallToolError::unknown_tool(params.name))
        }
    }
}
