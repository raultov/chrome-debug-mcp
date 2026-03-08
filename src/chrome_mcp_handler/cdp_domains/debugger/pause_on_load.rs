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
    description = "Enable debugger and pause on the first statement of the next executed script, then reload the page"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct PauseOnLoadTool {}

impl PauseOnLoadTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let client = client_lock.as_mut().unwrap();

        client
            .send_raw_command("Debugger.enable", cdp_lite::protocol::NoParams)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("CDP Debugger.enable error: {:?}", e))
            })?;

        client
            .send_raw_command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": "debugger;" }),
            )
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("CDP Page.addScript... error: {:?}", e))
            })?;

        client
            .send_raw_command("Page.reload", cdp_lite::protocol::NoParams)
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
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

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
