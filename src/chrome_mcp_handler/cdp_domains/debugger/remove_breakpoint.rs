use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "remove_breakpoint",
    description = "Remove a debugger breakpoint"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct RemoveBreakpointTool {
    pub breakpoint_id: String,
}

impl RemoveBreakpointTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: RemoveBreakpointTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let response = cdp_client
            .send_raw_command(
                "Debugger.removeBreakpoint",
                json!({ "breakpointId": args.breakpoint_id }),
            )
            .await;

        match response {
            Ok(resp) => Ok(CallToolResult::text_content(vec![
                format!("Breakpoint removed: {:?}", resp).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!(
                "Failed to remove breakpoint: {:?}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::debugger::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_remove_breakpoint_missing_id_fails_deserialization() {
        let handler = ChromeMcpHandler::new_test();

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "remove_breakpoint",
            "arguments": {}
        }))
        .unwrap();
        let result = RemoveBreakpointTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing field `breakpoint_id`")
        );
    }

    #[tokio::test]
    async fn test_remove_breakpoint_tool_deserialization() {
        let tool: Result<RemoveBreakpointTool, _> = serde_json::from_value(json!({
            "breakpoint_id": "bp-123"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().breakpoint_id, "bp-123");
    }

    #[tokio::test]
    async fn test_remove_breakpoint_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "remove_breakpoint",
            "arguments": {
                "breakpoint_id": "mock_bp_id_1"
            }
        }))
        .unwrap();

        let result = RemoveBreakpointTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Breakpoint removed"),
            "Content didn't match: {}",
            content_str
        );
    }
}
