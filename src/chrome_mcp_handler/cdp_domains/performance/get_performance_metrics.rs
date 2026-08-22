use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "get_performance_metrics",
    description = "Captures runtime performance metrics including JS heap size, DOM node count, and layout timing. Side effects: none (read-only snapshot). Prerequisites: requires an active Chrome tab. Returns: JSON object mapping metric names to numeric values (e.g., JSHeapUsedSize, LayoutCount). Use this to monitor memory usage, detect memory leaks, or profile performance. Alternatives: 'profile_page_performance' for detailed tracing, browser DevTools Performance tab."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct GetPerformanceMetricsTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
}

impl GetPerformanceMetricsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: GetPerformanceMetricsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;
        let target = session.target(args.tab_id.clone()).await?;

        // `Performance.getMetrics` returns a list of metrics.
        let result = target
            .send_raw_command("Performance.getMetrics", serde_json::json!({}))
            .await;

        match result {
            Ok(resp) => {
                let metrics = resp.result.unwrap_or_default();
                let mut formatted_metrics = std::collections::HashMap::new();

                if let Some(metrics_arr) = metrics.get("metrics").and_then(|m| m.as_array()) {
                    for metric in metrics_arr {
                        if let (Some(name), Some(value)) = (
                            metric.get("name").and_then(|n| n.as_str()),
                            metric.get("value").and_then(|v| v.as_f64()),
                        ) {
                            formatted_metrics.insert(name.to_string(), value);
                        }
                    }
                }

                Ok(CallToolResult::text_content(vec![
                    serde_json::to_string_pretty(&formatted_metrics)
                        .unwrap_or_default()
                        .into(),
                ]))
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
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn test_get_performance_metrics_tool_deserialization() {
        let json = json!({});
        let tool: GetPerformanceMetricsTool = serde_json::from_value(json).unwrap();
        assert!(format!("{:?}", tool).contains("GetPerformanceMetricsTool"));
    }

    #[tokio::test]
    async fn test_get_performance_metrics_handle() {
        let port = spawn_mock_chrome_server().await;
        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager =
            std::sync::Arc::new(tokio::sync::Mutex::new(MockChromeManager::new(port)));

        let params = CallToolRequestParams {
            name: "get_performance_metrics".to_string(),
            arguments: Some(json!({}).as_object().unwrap().clone()),
            meta: None,
            task: None,
        };

        let result = GetPerformanceMetricsTool::handle(params, &handler).await;
        // The mock server doesn't handle Performance.getMetrics by default,
        // it returns an empty object {} in result, which leads to an empty metrics map.
        assert!(result.is_ok());
        let content = result.unwrap().content;
        assert_eq!(content.len(), 1);
    }
}
