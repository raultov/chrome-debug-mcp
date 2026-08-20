use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "webmcp_list_tools",
    description = "Lists all WebMCP tools currently registered by the web page. Side effects: none (read-only state access). Prerequisites: WebMCP feature must be enabled in Chrome and the page must have registered tools. Returns: JSON array of available tools with schemas and frame IDs. Use this to discover capabilities exposed by websites implementing WebMCP."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ListWebmcpToolsTool {}

impl ListWebmcpToolsTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let st = handler.webmcp_state.lock().await;
        let mut all_tools = Vec::new();

        for frame_tools in st.tools.values() {
            for tool in frame_tools.values() {
                all_tools.push(tool.clone());
            }
        }

        let content = serde_json::to_string_pretty(&all_tools).map_err(|e| {
            CallToolError::from_message(format!("Failed to serialize tools: {}", e))
        })?;

        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpTool;
    use serde_json::json;

    #[tokio::test]
    async fn test_webmcp_list_tools_handle() {
        let handler = ChromeMcpHandler::new_test();
        {
            let mut st = handler.webmcp_state.lock().await;
            st.tools.entry("frame-1".into()).or_default().insert(
                "mockTool".into(),
                WebmcpTool {
                    name: "mockTool".into(),
                    description: "desc".into(),
                    input_schema: json!({}),
                    annotations: None,
                    frame_id: "frame-1".into(),
                    backend_node_id: None,
                },
            );
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_tools",
            "arguments": {}
        }))
        .unwrap();

        let result = ListWebmcpToolsTool::handle(params, &handler).await.unwrap();
        let text = format!("{:?}", result.content);
        assert!(text.contains("mockTool"));
    }
}
