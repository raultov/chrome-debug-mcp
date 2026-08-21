use crate::chrome_mcp_handler::ChromeMcpHandler;
use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpAvailability;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "webmcp_list_tools",
    description = "Lists all WebMCP tools currently registered by the web page. Side effects: none (read-only state access). Prerequisites: WebMCP feature must be enabled in Chrome and the page must have registered tools. Returns: JSON array of available tools with schemas and frame IDs. Use this to discover capabilities exposed by websites implementing WebMCP."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ListWebmcpToolsTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
}

impl ListWebmcpToolsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: ListWebmcpToolsTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;
        let session = handler.session(tool.instance_id.clone()).await?;

        let st = session.webmcp_state.lock().await;
        let mut all_tools = Vec::new();

        for frame_tools in st.tools.values() {
            for tool in frame_tools.values() {
                all_tools.push(tool.clone());
            }
        }

        let content = serde_json::to_string_pretty(&all_tools).map_err(|e| {
            CallToolError::from_message(format!("Failed to serialize tools: {}", e))
        })?;

        let mut content_list = vec![content.into()];

        if all_tools.is_empty() {
            let warn_text = match st.availability {
                WebmcpAvailability::NotRequested => {
                    "\n\n[Warning] No tools registered. This instance was not launched with the 'WEB_MCP' preset. To enable WebMCP, please restart this instance with the 'WEB_MCP' feature active, or open a new instance requesting the 'WEB_MCP' feature."
                }
                WebmcpAvailability::Unsupported => {
                    "\n\n[Warning] No tools registered. The 'WEB_MCP' feature was requested, but this Chrome instance does not support or expose the WebMCP CDP domain."
                }
                WebmcpAvailability::Enabled => {
                    "\n\n[Note] The 'WEB_MCP' preset is active, but the current web page has not registered any tools yet. Make sure you have navigated to a WebMCP-capable page (like https://www.knot.kz/#/agent-tools) and the page has finished loading (try reloading)."
                }
            };
            content_list.push(warn_text.to_string().into());
        }

        Ok(CallToolResult::text_content(content_list))
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
            let mut st = handler.default_session.webmcp_state.lock().await;
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
