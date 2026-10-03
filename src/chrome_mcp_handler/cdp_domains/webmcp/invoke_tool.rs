use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "webmcp_invoke_tool",
    description = "Invokes a WebMCP tool registered by the current web page. Side effects: depends on the tool invoked; may modify page state, trigger network requests, or perform other actions defined by the page. Prerequisites: WebMCP feature must be enabled, target frame must exist, and the tool must be registered. Returns: The output of the tool invocation. Use this to interact with page-provided tools."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct InvokeWebmcpToolTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
    /// Target frame ID where the tool is registered. Constraints: must match a valid frame ID returned by webmcp_list_tools.
    #[serde(rename = "frameId")]
    pub frame_id: String,

    /// Name of the WebMCP tool to invoke. Constraints: must match a registered tool name.
    #[serde(rename = "toolName")]
    pub tool_name: String,

    /// JSON object string with the input parameters for the page tool (must match its inputSchema). Use "{}" when the tool takes no parameters. Defaults to: "{}".
    #[serde(default = "default_empty_json_object")]
    pub input: String,
}

fn default_empty_json_object() -> String {
    "{}".to_string()
}

fn parse_input_object(input: &str) -> Result<serde_json::Value, CallToolError> {
    let trimmed = input.trim();
    let value: serde_json::Value = if trimmed.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(trimmed).map_err(|e| {
            CallToolError::from_message(format!(
                "Invalid JSON in input: {}. Pass a JSON object string, e.g. \"{{}}\" or \"{{\\\"key\\\":\\\"value\\\"}}\".",
                e
            ))
        })?
    };

    match value {
        serde_json::Value::Object(_) => Ok(value),
        other => Err(CallToolError::from_message(format!(
            "WebMCP tool input must be a JSON object, got {}",
            other
        ))),
    }
}

impl InvokeWebmcpToolTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: InvokeWebmcpToolTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;
        let session = handler.session(tool.instance_id.clone()).await?;

        let input_obj = parse_input_object(&tool.input)?;

        let target = session.target(tool.tab_id.clone()).await?;

        let invoke_params = serde_json::json!({
            "frameId": tool.frame_id,
            "toolName": tool.tool_name,
            "input": input_obj
        });

        let response = target
            .send_raw_command("WebMCP.invokeTool", invoke_params)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to invoke WebMCP tool: {}", e))
            })?;

        let result_obj = response.result.unwrap_or_default();
        let invocation_id = result_obj
            .get("invocationId")
            .and_then(|id| id.as_str())
            .ok_or_else(|| CallToolError::from_message("Did not receive invocationId".to_string()))?
            .to_string();

        // Wait for the toolResponded event in the same tab's state
        let webmcp_state = session.webmcp_state(tool.tab_id.clone())?;
        let mut attempts = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            attempts += 1;

            // Timeout after 30 seconds (300 * 100ms)
            if attempts > 300 {
                return Err(CallToolError::from_message(format!(
                    "Invocation '{}' is still pending after 30s (the page tool may be waiting for user consent, e.g. a confirmation dialog). It has NOT been canceled: use webmcp_get_invocation with invocationId=\"{}\" to poll its status, or webmcp_list_invocations to see all pending invocations.",
                    tool.tool_name, invocation_id
                )));
            }

            let st = webmcp_state.lock().await;
            if let Some(invocation) = st.invocations.get(&invocation_id)
                && let Some(status) = &invocation.status
            {
                match status.as_str() {
                    "Completed" => {
                        let output = invocation.output.clone().unwrap_or(serde_json::json!({}));
                        let content = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|_| "{}".to_string());
                        return Ok(CallToolResult::text_content(vec![content.into()]));
                    }
                    "Error" => {
                        let error_text = invocation
                            .error_text
                            .clone()
                            .unwrap_or_else(|| "Unknown error".to_string());
                        return Err(CallToolError::from_message(format!(
                            "Tool execution failed: {}",
                            error_text
                        )));
                    }
                    "Canceled" => {
                        return Err(CallToolError::from_message(
                            "Tool execution was canceled".to_string(),
                        ));
                    }
                    _ => {
                        // Still waiting or unknown state, continue loop
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_webmcp_invoke_tool_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "webmcp_invoke_tool",
            "arguments": {
                "frameId": "frame-1",
                "toolName": "myTool",
                "input": "{\"key\":\"value\"}"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_webmcp_invoke_tool_deserialization() {
        let tool: Result<InvokeWebmcpToolTool, _> = serde_json::from_value(json!({
            "frameId": "frame-1",
            "toolName": "myTool",
            "input": "{\"key\":\"value\"}"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().input, "{\"key\":\"value\"}");
    }

    #[tokio::test]
    async fn test_webmcp_invoke_tool_tab_id_deserialization() {
        let tool: InvokeWebmcpToolTool = serde_json::from_value(json!({
            "tab_id": "tab-1",
            "frameId": "frame-1",
            "toolName": "myTool"
        }))
        .unwrap();
        assert_eq!(tool.tab_id.as_deref(), Some("tab-1"));
    }

    #[tokio::test]
    async fn test_webmcp_invoke_tool_tab_id_defaults_to_none() {
        let tool: InvokeWebmcpToolTool = serde_json::from_value(json!({
            "frameId": "frame-1",
            "toolName": "myTool"
        }))
        .unwrap();
        assert!(tool.tab_id.is_none());
    }

    #[tokio::test]
    async fn test_invoke_tool_unknown_tab_id_errors() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_invoke_tool",
            "arguments": {
                "tab_id": "tab-nope",
                "frameId": "frame-1",
                "toolName": "myTool"
            }
        }))
        .unwrap();
        let err = InvokeWebmcpToolTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected tab-not-found error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_invoke_tool_uses_tab_state_not_session_state() {
        use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpInvocation;
        use std::time::Duration;

        let handler = ChromeMcpHandler::new_test();
        let session = handler.session(None).await.unwrap();

        // Put a COMPLETED invocation in the SESSION state under a decoy id.
        {
            let mut st = session.webmcp_state.lock().await;
            st.invocations.insert(
                "decoy".into(),
                WebmcpInvocation {
                    tool_name: "myTool".into(),
                    frame_id: "frame-1".into(),
                    invocation_id: "decoy".into(),
                    input: "{}".into(),
                    status: Some("Completed".into()),
                    output: Some(json!({"from": "session"})),
                    error_text: None,
                },
            );
        }

        // Register a real tab.
        let tab_id = {
            use cdp_browser_lite::BrowserClient;
            let port =
                crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server().await;
            let browser =
                BrowserClient::connect(&format!("127.0.0.1:{}", port), Duration::from_secs(5))
                    .await
                    .expect("BrowserClient connect to mock");
            let tab = browser.attach("T-page-1").await.expect("attach to mock");

            let mut registry = session.tabs.write().unwrap();
            registry
                .register_tab(tab, None, "https://example.test".into())
                .expect("register tab")
        };

        // Put a COMPLETED invocation in the TAB state with a known id.
        {
            let state = session.webmcp_state(Some(tab_id.clone())).unwrap();
            let mut st = state.lock().await;
            st.invocations.insert(
                "tab-inv".into(),
                WebmcpInvocation {
                    tool_name: "myTool".into(),
                    frame_id: "frame-1".into(),
                    invocation_id: "tab-inv".into(),
                    input: "{}".into(),
                    status: Some("Completed".into()),
                    output: Some(json!({"from": "tab"})),
                    error_text: None,
                },
            );
        }

        // The handle() will try to send WebMCP.invokeTool through the mock
        // target; the mock returns {} so it fails with "Did not receive
        // invocationId". The important assertion is that it did NOT fail with
        // "Tab not found" (proving target(tab_id) and webmcp_state(tab_id) were
        // both resolved to the registered tab).
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_invoke_tool",
            "arguments": {
                "tab_id": tab_id,
                "frameId": "frame-1",
                "toolName": "myTool"
            }
        }))
        .unwrap();
        let err = InvokeWebmcpToolTool::handle(params, &handler)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("not found"),
            "tab_id should have resolved to the registered tab: {msg}"
        );
    }

    #[tokio::test]
    async fn test_webmcp_invoke_tool_input_defaults_to_empty_object() {
        let tool: InvokeWebmcpToolTool = serde_json::from_value(json!({
            "frameId": "frame-1",
            "toolName": "myTool"
        }))
        .unwrap();
        assert_eq!(tool.input, "{}");
    }

    #[test]
    fn test_parse_input_object_empty_and_object() {
        assert_eq!(parse_input_object("{}").unwrap(), json!({}));
        assert_eq!(parse_input_object("").unwrap(), json!({}));
        assert_eq!(parse_input_object("{\"a\":1}").unwrap(), json!({"a": 1}));
    }

    #[test]
    fn test_parse_input_object_rejects_non_object() {
        assert!(parse_input_object("[]").is_err());
        assert!(parse_input_object("\"x\"").is_err());
        assert!(parse_input_object("not-json").is_err());
    }

    #[test]
    fn test_webmcp_invoke_tool_schema_input_is_string() {
        let schema = InvokeWebmcpToolTool::json_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let input = props.get("input").unwrap();
        assert_eq!(input.get("type").and_then(|v| v.as_str()), Some("string"));
        assert_ne!(input.get("type").and_then(|v| v.as_str()), Some("unknown"));
    }

    #[test]
    fn test_webmcp_invoke_tool_schema_no_unknown_types() {
        let schema = InvokeWebmcpToolTool::json_schema();
        let serialized = serde_json::to_string(&schema).unwrap();
        assert!(
            !serialized.contains("\"unknown\""),
            "schema must not contain type=unknown: {serialized}"
        );
    }
}
