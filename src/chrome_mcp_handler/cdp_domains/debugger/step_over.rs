use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "step_over",
    description = "Executes the current line of code without entering function calls, pausing at the next line. Side effects: advances debugger execution state. Prerequisites: requires an active, paused debugger session. Returns: confirmation of step execution. Use this to skip function internals during debugging. Alternatives: use 'resume' to continue full execution, 'step_over' enters functions, or 'evaluate_on_call_frame' to inspect state without stepping."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct StepOverTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
}

impl StepOverTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: StepOverTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;
        let target = session.target(args.tab_id.clone()).await?;

        let _ = target
            .send_raw_command("Debugger.stepOver", cdp_browser_lite::NoParams)
            .await
            .map_err(|e| CallToolError::from_message(format!("Failed to step over: {:?}", e)))?;

        Ok(CallToolResult::text_content(vec![
            "Stepped over to the next expression in the debugger.".into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_step_over_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "step_over",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_step_over_tool_deserialization() {
        let tool: Result<StepOverTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_step_over_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "step_over",
            "arguments": {}
        }))
        .unwrap();

        let result = StepOverTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Stepped over to the next expression in the debugger"),
            "Content didn't match: {}",
            content_str
        );
    }
}
