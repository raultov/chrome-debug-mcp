use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "stop_chrome",
    description = "Gracefully terminates the managed Chrome instance and closes all debugging connections. Side effects: destructive - kills Chrome process; all open tabs closed; unsaved data lost. Prerequisites: Chrome instance must be running. Returns: termination success confirmation. Use this to clean up resources, prevent zombie processes, or end debugging session. Alternatives: 'restart_chrome' to restart instead of stop."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct StopChromeTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
}

impl StopChromeTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: StopChromeTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let session = handler.session(tool.instance_id.clone()).await?;
        let mut manager = session.chrome_manager.lock().await;

        // Reset the client connection before stopping
        {
            let mut client_lock = session.client.lock().await;
            *client_lock = None;
        }

        if let Err(e) = manager.stop_instance().await {
            return Err(CallToolError::from_message(format!(
                "Failed to stop Chrome: {}",
                e
            )));
        }

        Ok(CallToolResult::text_content(vec![
            "Chrome instance stopped successfully.".into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_stop_chrome_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "stop_chrome",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_stop_chrome_tool_deserialization() {
        let tool: Result<StopChromeTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_stop_chrome_handle() {
        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(9999)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "stop_chrome",
            "arguments": { "instance_id": "default" }
        }))
        .unwrap();

        let result = StopChromeTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Chrome instance stopped successfully"),
            "Content didn't match: {}",
            content_str
        );
    }
}
