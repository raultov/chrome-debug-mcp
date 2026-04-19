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
pub struct GetPerformanceMetricsTool {}

impl GetPerformanceMetricsTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().ok_or_else(|| {
            CallToolError::from_message("Chrome connection is not established".to_string())
        })?;

        // `Performance.getMetrics` returns a list of metrics.
        let result = cdp_client
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
    use serde_json::json;

    #[test]
    fn test_get_performance_metrics_tool_deserialization() {
        let json = json!({});
        let tool: GetPerformanceMetricsTool = serde_json::from_value(json).unwrap();
        assert!(format!("{:?}", tool).contains("GetPerformanceMetricsTool"));
    }

    #[tokio::test]
    async fn test_get_performance_metrics_handle() {
        let port = spawn_mock_chrome_server().await;
        let handler = ChromeMcpHandler::new_test();
        {
            let mut manager = handler.chrome_manager.lock().await;
            manager.set_port(port);
        }

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
