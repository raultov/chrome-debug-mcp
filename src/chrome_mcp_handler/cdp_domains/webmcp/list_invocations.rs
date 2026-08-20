use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "webmcp_list_invocations",
    description = "Lists all WebMCP tool invocations from this Chrome session with their current status. Side effects: none (read-only state access). Returns: JSON array with toolName, frameId, input, and status ('Pending', 'Completed', 'Error', or 'Canceled') for each invocation. Use this to find invocations that are waiting for user consent on the page (status 'Pending') or to recover an invocationId after losing it. Alternatives: webmcp_get_invocation for a single invocation by id."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ListWebmcpInvocationsTool {
    /// Optional filter: only return invocations with this status. Constraints: one of 'Pending', 'Completed', 'Error', 'Canceled'. Defaults to: None (all invocations).
    pub status: Option<String>,
}

impl ListWebmcpInvocationsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: ListWebmcpInvocationsTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        if let Some(filter) = &tool.status {
            const VALID: [&str; 4] = ["Pending", "Completed", "Error", "Canceled"];
            if !VALID.contains(&filter.as_str()) {
                return Err(CallToolError::from_message(format!(
                    "Invalid status filter '{}'. Must be one of: {}",
                    filter,
                    VALID.join(", ")
                )));
            }
        }

        let st = handler.webmcp_state.lock().await;
        let mut invocations: Vec<serde_json::Value> = Vec::new();

        for invocation in st.invocations.values() {
            let status = invocation
                .status
                .clone()
                .unwrap_or_else(|| "Pending".to_string());

            if let Some(filter) = &tool.status
                && filter != &status
            {
                continue;
            }

            invocations.push(serde_json::json!({
                "invocationId": invocation.invocation_id,
                "toolName": invocation.tool_name,
                "frameId": invocation.frame_id,
                "input": invocation.input,
                "status": status,
            }));
        }

        invocations.sort_by(|a, b| {
            a.get("invocationId")
                .and_then(|v| v.as_str())
                .cmp(&b.get("invocationId").and_then(|v| v.as_str()))
        });

        let content = serde_json::to_string_pretty(&invocations).map_err(|e| {
            CallToolError::from_message(format!("Failed to serialize invocations: {}", e))
        })?;
        Ok(CallToolResult::text_content(vec![content.into()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpInvocation;
    use serde_json::json;

    fn make_invocation(id: &str, status: Option<&str>) -> WebmcpInvocation {
        WebmcpInvocation {
            tool_name: "t".to_string(),
            frame_id: "f".to_string(),
            invocation_id: id.to_string(),
            input: "{}".to_string(),
            status: status.map(String::from),
            output: None,
            error_text: None,
        }
    }

    fn extract_text(result: &CallToolResult) -> String {
        let content_val = serde_json::to_value(&result.content).unwrap();
        content_val[0]["text"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn test_list_invocations_empty() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_invocations",
            "arguments": {}
        }))
        .unwrap();

        let result = ListWebmcpInvocationsTool::handle(params, &handler)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&extract_text(&result)).unwrap();
        assert_eq!(payload.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_list_invocations_all_and_filtered() {
        let handler = ChromeMcpHandler::new_test();
        {
            let mut st = handler.webmcp_state.lock().await;
            st.invocations
                .insert("a".to_string(), make_invocation("a", None));
            st.invocations
                .insert("b".to_string(), make_invocation("b", Some("Completed")));
        }

        // No filter: both appear
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_invocations",
            "arguments": {}
        }))
        .unwrap();
        let result = ListWebmcpInvocationsTool::handle(params, &handler)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&extract_text(&result)).unwrap();
        let all = payload.as_array().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0]["invocationId"], "a");
        assert_eq!(all[0]["status"], "Pending");
        assert_eq!(all[1]["invocationId"], "b");
        assert_eq!(all[1]["status"], "Completed");

        // Filter Pending: only "a"
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_invocations",
            "arguments": { "status": "Pending" }
        }))
        .unwrap();
        let result = ListWebmcpInvocationsTool::handle(params, &handler)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&extract_text(&result)).unwrap();
        let filtered = payload.as_array().unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["invocationId"], "a");
    }

    #[tokio::test]
    async fn test_list_invocations_invalid_status_rejected() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "webmcp_list_invocations",
            "arguments": { "status": "Running" }
        }))
        .unwrap();

        let err = ListWebmcpInvocationsTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Invalid status filter"));
    }

    #[test]
    fn test_list_invocations_schema_no_unknown_types() {
        let schema = ListWebmcpInvocationsTool::json_schema();
        let serialized = serde_json::to_string(&schema).unwrap();
        assert!(
            !serialized.contains("\"unknown\""),
            "schema must not contain type=unknown: {}",
            serialized
        );
    }
}
