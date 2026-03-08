use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "evaluate_js",
    description = "Evaluate JavaScript in the current Chrome tab"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct EvaluateJsTool {
    pub expression: String,
}

impl EvaluateJsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: EvaluateJsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().ok_or_else(|| {
            CallToolError::from_message("Chrome connection is not established".to_string())
        })?;

        let result = cdp_client
            .send_raw_command(
                "Runtime.evaluate",
                json!({
                    "expression": args.expression,
                    "returnByValue": true,
                    "awaitPromise": true
                }),
            )
            .await;

        match result {
            Ok(val) => {
                let formatted = format!("{:?}", val);
                Ok(CallToolResult::text_content(vec![formatted.into()]))
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
    async fn test_evaluate_js_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "evaluate_js",
            "arguments": {
                "expression": "console.log('test')"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_evaluate_js_tool_deserialization() {
        let tool: Result<EvaluateJsTool, _> = serde_json::from_value(json!({
            "expression": "2 + 2"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().expression, "2 + 2");
    }

    #[tokio::test]
    async fn test_evaluate_js_missing_expression_fails() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "evaluate_js",
            "arguments": {}
        }))
        .unwrap();

        let result = EvaluateJsTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing field `expression`")
        );
    }

    #[tokio::test]
    async fn test_evaluate_js_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "evaluate_js",
            "arguments": {
                "expression": "2 + 2"
            }
        }))
        .unwrap();

        let result = EvaluateJsTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        // Checking if the basic success output format is returned
        assert!(content_str.contains("WsResponse"));
    }
}
