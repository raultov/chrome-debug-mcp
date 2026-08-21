use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "resume",
    description = "Continues full execution of the debugger from current breakpoint. Side effects: advances code execution until next breakpoint or completion. Prerequisites: requires an active, paused debugger session. Returns: confirmation of resume command. Use this to continue program flow after inspection. Alternatives: 'step_over' or 'step_out' for single-step execution."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ResumeTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
}

impl ResumeTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ResumeTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        let mut client_lock = session.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let _ = cdp_client
            .send_raw_command("Debugger.resume", cdp_browser_lite::NoParams)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to resume execution: {:?}", e))
            })?;

        Ok(CallToolResult::text_content(vec![
            "Debugger execution resumed.".into(),
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
    async fn test_resume_params_deserialization() {
        // Valid params should deserialize without error
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "resume",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_resume_tool_deserialization() {
        // ResumeTool has no fields, so empty object should work
        let tool: Result<ResumeTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_resume_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "resume",
            "arguments": {}
        }))
        .unwrap();

        let result = ResumeTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Debugger execution resumed"),
            "Content didn't match: {}",
            content_str
        );
    }
}
