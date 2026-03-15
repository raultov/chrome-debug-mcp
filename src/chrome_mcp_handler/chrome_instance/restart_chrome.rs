use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "restart_chrome",
    description = "Restarts the managed Chrome instance with remote debugging enabled. This tool can be used to start a new instance or restart an already existing one."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct RestartChromeTool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_server: Option<String>,
}

impl RestartChromeTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: RestartChromeTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let mut manager = handler.chrome_manager.lock().await;

        // Reset the client connection before stopping/starting
        {
            let mut client_lock = handler.client.lock().await;
            *client_lock = None;
        }

        if let Err(e) = manager.stop_instance() {
            return Err(CallToolError::from_message(format!(
                "Failed to stop Chrome: {}",
                e
            )));
        }

        manager.set_proxy(tool.proxy_server);

        if let Err(e) = manager.ensure_instance().await {
            return Err(CallToolError::from_message(format!(
                "Failed to start Chrome: {}",
                e
            )));
        }

        Ok(CallToolResult::text_content(vec![
            "Chrome instance restarted successfully.".into(),
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
    async fn test_restart_chrome_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_restart_chrome_tool_deserialization() {
        let tool: Result<RestartChromeTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_restart_chrome_handle() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(9999)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": {}
        }))
        .unwrap();

        let result = RestartChromeTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Chrome instance restarted successfully"),
            "Content didn't match: {}",
            content_str
        );
    }
}
