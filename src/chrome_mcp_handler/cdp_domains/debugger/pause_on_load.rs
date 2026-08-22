// Pause on Load tool implementation
// This tool enables the debugger, sets a breakpoint on the next script, and reloads the page.

use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "pause_on_load",
    description = "Enables the debugger and injects a breakpoint at the first statement of any script loaded after reloading the page. Side effects: reloads the current page (destructive of unsaved state). Prerequisites: requires an active Chrome tab. Returns: confirmation of debugger enablement and page reload. Use this to debug script execution from the page load. Alternatives: 'set_breakpoint' for targeting specific scripts/lines, 'pause_on_exceptions' for exception-based pausing."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct PauseOnLoadTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
}

impl PauseOnLoadTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: PauseOnLoadTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;
        let target = session.target(args.tab_id.clone()).await?;

        target
            .send_raw_command("Debugger.enable", cdp_browser_lite::NoParams)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("CDP Debugger.enable error: {:?}", e))
            })?;

        target
            .send_raw_command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": "debugger;" }),
            )
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("CDP Page.addScript... error: {:?}", e))
            })?;

        target
            .send_raw_command("Page.reload", cdp_browser_lite::NoParams)
            .await
            .map_err(|e| CallToolError::from_message(format!("CDP Page.reload error: {:?}", e)))?;

        Ok(CallToolResult::text_content(vec![
            "Debugger enabled and paused on the next executed statement. Page reloaded.".into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::ChromeMcpHandler;
    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_pause_on_load_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "pause_on_load",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_pause_on_load_tool_deserialization() {
        let tool: Result<PauseOnLoadTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_pause_on_load_handle() {
        let port = spawn_mock_chrome_server().await;

        // Create a handler with a MockChromeManager that returns our mock server's port
        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "pause_on_load",
            "arguments": {}
        }))
        .unwrap();

        let result = PauseOnLoadTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Debugger enabled"),
            "Content didn't match: {}",
            content_str
        );
    }
}
