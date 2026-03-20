use crate::chrome_mcp_handler::{ChromeMcpHandler, is_local_address};
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "navigate",
    description = "Navigate the current Chrome tab to a URL"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct NavigateTool {
    pub url: String,
}

impl NavigateTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: NavigateTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        if handler.local_only && !is_local_address(&args.url) {
            return Err(CallToolError::from_message(format!(
                "Navigation to '{}' is blocked. This MCP server is running with the 'local' argument, which restricts navigation to local addresses only (localhost, 127.0.0.1, 192.168.x.x, or *.local). To allow navigation to external addresses, restart the MCP server without the 'local' argument.",
                args.url
            )));
        }

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let result = cdp_client
            .send_raw_command(
                "Page.navigate",
                json!({
                    "url": args.url
                }),
            )
            .await;

        match result {
            Ok(val) => Ok(CallToolResult::text_content(vec![
                format!("Navigated to {}. Protocol Response: {:?}", args.url, val).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!("CDP Error: {:?}", e))),
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
    async fn test_navigate_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_navigate_tool_deserialization() {
        let tool: Result<NavigateTool, _> = serde_json::from_value(json!({
            "url": "https://example.com"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().url, "https://example.com");
    }

    #[tokio::test]
    async fn test_navigate_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com"
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Navigated to https://example.com"),
            "Content didn't match: {}",
            content_str
        );
    }

    #[tokio::test]
    async fn test_navigate_local_only_restriction() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.local_only = true;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://google.com"
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Navigation to 'https://google.com' is blocked")
        );

        // Test with local address
        let params_local: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "http://localhost:3000"
            }
        }))
        .unwrap();

        let port = spawn_mock_chrome_server().await;
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let result_local = NavigateTool::handle(params_local, &handler).await;
        assert!(
            result_local.is_ok(),
            "Local navigation should succeed: {:?}",
            result_local.err()
        );
    }

    #[tokio::test]
    async fn test_navigate_local_only_addresses() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.local_only = true;
        let port = spawn_mock_chrome_server().await;
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let local_urls = vec![
            "http://127.0.0.1:8080",
            "http://localhost:5173",
            "http://192.168.1.50/index.html",
            "http://myapp.local/",
        ];

        for url in local_urls {
            let params: CallToolRequestParams = serde_json::from_value(json!({
                "name": "navigate",
                "arguments": {
                    "url": url
                }
            }))
            .unwrap();
            let result = NavigateTool::handle(params, &handler).await;
            assert!(result.is_ok(), "URL {} should be allowed", url);
        }

        let blocked_urls = vec![
            "https://github.com",
            "http://10.0.0.1", // Currently not in my list, following prompt's specific list
            "http://1.1.1.1",
        ];

        for url in blocked_urls {
            let params: CallToolRequestParams = serde_json::from_value(json!({
                "name": "navigate",
                "arguments": {
                    "url": url
                }
            }))
            .unwrap();
            let result = NavigateTool::handle(params, &handler).await;
            assert!(result.is_err(), "URL {} should be blocked", url);
        }
    }
}
