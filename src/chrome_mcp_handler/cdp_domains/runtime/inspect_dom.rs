use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "inspect_dom",
    description = "Fetch the entire HTML of the current document or a snippet around a search query"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct InspectDomTool {
    /// Optional: Search for this specific text within the DOM
    pub query: Option<String>,
    /// Optional: Number of characters to include before the match (default 200)
    pub before: Option<u32>,
    /// Optional: Number of characters to include after the match (default 200)
    pub after: Option<u32>,
}

impl InspectDomTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: InspectDomTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let result = cdp_client
            .send_raw_command(
                "Runtime.evaluate",
                json!({
                    "expression": "document.documentElement.outerHTML",
                    "returnByValue": true
                }),
            )
            .await;

        match result {
            Ok(val) => {
                let html = val
                    .result
                    .as_ref()
                    .and_then(|r| r.get("result"))
                    .and_then(|r| r.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if let Some(query) = &args.query {
                    if let Some(pos) = html.find(query) {
                        let before = args.before.unwrap_or(200) as usize;
                        let after = args.after.unwrap_or(200) as usize;

                        let start = pos.saturating_sub(before);
                        let end = std::cmp::min(pos + query.len() + after, html.len());

                        let snippet = &html[start..end];
                        let message = format!(
                            "Found '{}' at position {}. Context (before={}, after={}):\n\n[...]\n{}\n[...]",
                            query, pos, before, after, snippet
                        );
                        Ok(CallToolResult::text_content(vec![message.into()]))
                    } else {
                        Ok(CallToolResult::text_content(vec![
                            format!("Query '{}' not found in the DOM.", query).into(),
                        ]))
                    }
                } else {
                    Ok(CallToolResult::text_content(vec![html.to_string().into()]))
                }
            }
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
    async fn test_inspect_dom_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "inspect_dom",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_inspect_dom_tool_deserialization() {
        let tool: Result<InspectDomTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
    }

    #[tokio::test]
    async fn test_inspect_dom_handle_full() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "inspect_dom",
            "arguments": {}
        }))
        .unwrap();

        let result = InspectDomTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        let content_str = format!("{:?}", call_result.content);
        assert!(content_str.contains("Hello World"));
    }

    #[tokio::test]
    async fn test_inspect_dom_handle_search() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "inspect_dom",
            "arguments": {
                "query": "Hello World",
                "before": 10,
                "after": 10
            }
        }))
        .unwrap();

        let result = InspectDomTool::handle(params, &handler).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        let content_str = format!("{:?}", call_result.content);
        assert!(content_str.contains("Found 'Hello World'"));
        assert!(content_str.contains("before=10, after=10"));
    }

    #[tokio::test]
    async fn test_inspect_dom_handle_search_not_found() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "inspect_dom",
            "arguments": {
                "query": "NOT_IN_DOM"
            }
        }))
        .unwrap();

        let result = InspectDomTool::handle(params, &handler).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        let content_str = format!("{:?}", call_result.content);
        assert!(content_str.contains("Query 'NOT_IN_DOM' not found"));
    }
}
