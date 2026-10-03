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
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
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

        let webmcp_state = session.webmcp_state(tool.tab_id.clone())?;
        let st = webmcp_state.lock().await;
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
                    "\n\n[Warning] No tools registered. WebMCP testing features are not active. Make sure the server was started with --enable-webmcp."
                }
                WebmcpAvailability::Unsupported => {
                    "\n\n[Warning] No tools registered. WebMCP was enabled via --enable-webmcp, but this Chrome instance does not support or expose the WebMCP CDP domain."
                }
                WebmcpAvailability::Enabled => {
                    "\n\n[Note] WebMCP is active, but the current web page has not registered any tools yet. Make sure you have navigated to a WebMCP-capable page (like https://www.knot.kz/#/agent-tools) and the page has finished loading (try reloading)."
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

    fn make_tool(name: &str, frame_id: &str) -> WebmcpTool {
        WebmcpTool {
            name: name.into(),
            description: "desc".into(),
            input_schema: json!({}),
            annotations: None,
            frame_id: frame_id.into(),
            backend_node_id: None,
        }
    }

    fn extract_text(result: &CallToolResult) -> String {
        let content_val = serde_json::to_value(&result.content).unwrap();
        content_val[0]["text"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn test_webmcp_list_tools_handle() {
        let handler = ChromeMcpHandler::new_test();
        {
            let mut st = handler.default_session.webmcp_state.lock().await;
            st.tools
                .entry("frame-1".into())
                .or_default()
                .insert("mockTool".into(), make_tool("mockTool", "frame-1"));
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_tools",
            "arguments": {}
        }))
        .unwrap();

        let result = ListWebmcpToolsTool::handle(params, &handler).await.unwrap();
        let text = extract_text(&result);
        assert!(text.contains("mockTool"));
    }

    #[tokio::test]
    async fn test_list_tools_reads_tab_state_not_session_state() {
        let handler = ChromeMcpHandler::new_test();
        let session = handler.session(None).await.unwrap();

        // Put a tool in the SESSION state (the fallback).
        {
            let mut st = session.webmcp_state.lock().await;
            st.tools
                .entry("frame-s".into())
                .or_default()
                .insert("sessionTool".into(), make_tool("sessionTool", "frame-s"));
        }

        // Register a tab and put a different tool in the TAB state.
        let tab_state = {
            use cdp_browser_lite::BrowserClient;
            use std::time::Duration;

            let port =
                crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server().await;
            let browser =
                BrowserClient::connect(&format!("127.0.0.1:{}", port), Duration::from_secs(5))
                    .await
                    .expect("BrowserClient connect to mock");
            let tab = browser
                .attach("T-page-1")
                .await
                .expect("attach to mock target");

            let tab_id = {
                let mut registry = session.tabs.write().unwrap();
                registry
                    .register_tab(tab, None, "https://example.test".into())
                    .expect("register tab")
            };

            let state = session.webmcp_state(Some(tab_id.clone())).unwrap();
            {
                let mut st = state.lock().await;
                st.tools
                    .entry("frame-t".into())
                    .or_default()
                    .insert("tabTool".into(), make_tool("tabTool", "frame-t"));
            }
            (tab_id, state)
        };
        let (tab_id, _) = tab_state;

        // Explicit tab_id: must return the TAB tool, not the session tool.
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_tools",
            "arguments": { "tab_id": tab_id }
        }))
        .unwrap();
        let result = ListWebmcpToolsTool::handle(params, &handler).await.unwrap();
        let text = extract_text(&result);
        assert!(text.contains("tabTool"), "expected tab tool, got: {text}");
        assert!(
            !text.contains("sessionTool"),
            "must not leak session state into tab result"
        );

        // Omitted tab_id (active tab = the registered tab): same result.
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_tools",
            "arguments": {}
        }))
        .unwrap();
        let result = ListWebmcpToolsTool::handle(params, &handler).await.unwrap();
        let text = extract_text(&result);
        assert!(text.contains("tabTool"), "active-tab lookup failed: {text}");
    }

    #[tokio::test]
    async fn test_list_tools_unknown_tab_id_errors() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_tools",
            "arguments": { "tab_id": "tab-nope" }
        }))
        .unwrap();
        let err = ListWebmcpToolsTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected tab-not-found error, got: {err}"
        );
    }

    #[test]
    fn test_list_tools_schema_no_unknown_types() {
        let schema = ListWebmcpToolsTool::json_schema();
        let serialized = serde_json::to_string(&schema).unwrap();
        assert!(
            !serialized.contains("\"unknown\""),
            "schema must not contain type=unknown: {serialized}"
        );
    }
}
