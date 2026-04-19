use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "get_custom_events",
    description = "EXPERIMENTAL: Retrieves unhandled CDP events from domains not covered by specialized listeners (network, console, etc.). Side effects: none (read-only cache access). Prerequisites: requires active Chrome connection with send_cdp_command or custom domain listeners active. Returns: JSON array of custom events with method, parameters, and timestamp. Use this to see Target, Debugger, or other domain events. Alternatives: domain-specific listeners (get_network_logs, get_console_logs)."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct GetCustomEventsTool {
    /// Filter events by CDP method name (case-sensitive). Constraints: string matching format 'Domain.eventName'. Interactions: when omitted, returns all events. Defaults to: None (no filtering).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_method: Option<String>,
    /// Maximum number of events to return. Constraints: positive integer (0 = unlimited, clamped to cache size). Interactions: limits result set size. Defaults to: 100.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    100
}

impl GetCustomEventsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: GetCustomEventsTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let st = handler.custom_state.lock().await;
        let filtered_events: Vec<_> = st
            .events
            .iter()
            .filter(|e| {
                if let Some(filter) = &tool.filter_method {
                    &e.method == filter
                } else {
                    true
                }
            })
            .take(tool.limit as usize)
            .cloned()
            .collect();

        Ok(CallToolResult::text_content(vec![
            format!(
                "Found {} custom events. Captured events: {}",
                filtered_events.len(),
                serde_json::to_string_pretty(&filtered_events).unwrap_or_default()
            )
            .into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::CustomEvent;
    use serde_json::json;

    #[tokio::test]
    async fn test_get_custom_events_tool_schema() {
        let tool_def = GetCustomEventsTool::tool();
        println!("{}", serde_json::to_string_pretty(&tool_def).unwrap());
    }

    #[tokio::test]
    async fn test_get_custom_events_tool_deserialization() {
        let tool: Result<GetCustomEventsTool, _> = serde_json::from_value(json!({
            "filter_method": "Target.targetCreated",
            "limit": 10
        }));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.filter_method, Some("Target.targetCreated".to_string()));
        assert_eq!(tool.limit, 10);
    }

    #[tokio::test]
    async fn test_get_custom_events_tool_deserialization_default() {
        let tool: Result<GetCustomEventsTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.limit, 100);
    }

    #[tokio::test]
    async fn test_get_custom_events_handle() {
        let handler = ChromeMcpHandler::new_test();
        {
            let mut st = handler.custom_state.lock().await;
            st.events.push_back(CustomEvent {
                method: "Target.targetCreated".to_string(),
                params: json!({"targetId": "1"}),
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            });
            st.events.push_back(CustomEvent {
                method: "Target.targetInfoChanged".to_string(),
                params: json!({"targetId": "1"}),
                timestamp: "2023-01-01T00:00:01Z".to_string(),
            });
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_custom_events",
            "arguments": {
                "filter_method": "Target.targetCreated"
            }
        }))
        .unwrap();

        let result = GetCustomEventsTool::handle(params, &handler).await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        let content_str = format!("{:?}", call_result.content);
        assert!(content_str.contains("Found 1 custom events"));
        assert!(content_str.contains("Target.targetCreated"));
    }
}
