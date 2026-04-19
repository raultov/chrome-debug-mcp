use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "reload",
    description = "Reloads the current page, discarding all unsaved changes and re-fetching resources from the server. Side effects: destructive of unsaved state; clears dynamic DOM state. Prerequisites: requires an active Chrome tab. Returns: reload confirmation. Use this to refresh page content or reset to initial load state. Alternatives: 'navigate' to load a different URL, 'pause_on_load' to debug reload execution."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ReloadTool {}

impl ReloadTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let response = cdp_client.send_raw_command("Page.reload", json!({})).await;

        match response {
            Ok(resp) => Ok(CallToolResult::text_content(vec![
                format!("Page reloaded: {:?}", resp).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!(
                "Failed to reload page: {:?}",
                e
            ))),
        }
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
    async fn test_reload_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "reload",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_reload_tool_deserialization() {
        let tool: Result<ReloadTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_reload_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "reload",
            "arguments": {}
        }))
        .unwrap();

        let result = ReloadTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Page reloaded"),
            "Content didn't match: {}",
            content_str
        );
    }
}
