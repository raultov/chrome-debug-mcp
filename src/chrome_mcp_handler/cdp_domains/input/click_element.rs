use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "click_element",
    description = "Clicks on an element in the DOM using a CSS selector"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ClickElementTool {
    /// CSS selector for the element to click
    pub selector: String,
}

impl ClickElementTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ClickElementTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        // Escaping double quotes in the selector just in case
        let safe_selector = args.selector.replace("\"", "\\\"");

        let expression = format!(
            "(function() {{
                const el = document.querySelector(\"{}\");
                if (!el) return null;
                const rect = el.getBoundingClientRect();
                return {{
                    x: rect.left + rect.width / 2,
                    y: rect.top + rect.height / 2
                }};
            }})()",
            safe_selector
        );

        let eval_result = cdp_client
            .send_raw_command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true
                }),
            )
            .await;

        match eval_result {
            Ok(val) => {
                let result_obj = val.result.as_ref().and_then(|r| r.get("result"));

                // Check if element was found
                if result_obj
                    .and_then(|r| r.get("type"))
                    .and_then(|t| t.as_str())
                    == Some("object")
                    && result_obj
                        .and_then(|r| r.get("subtype"))
                        .and_then(|s| s.as_str())
                        == Some("null")
                {
                    return Err(CallToolError::from_message(format!(
                        "Element not found for selector: {}",
                        args.selector
                    )));
                }

                let value_obj = result_obj.and_then(|r| r.get("value"));

                if let Some(value) = value_obj {
                    if let (Some(x), Some(y)) = (
                        value.get("x").and_then(|v| v.as_f64()),
                        value.get("y").and_then(|v| v.as_f64()),
                    ) {
                        // Dispatch mousePressed
                        let _ = cdp_client
                            .send_raw_command(
                                "Input.dispatchMouseEvent",
                                json!({
                                    "type": "mousePressed",
                                    "button": "left",
                                    "x": x,
                                    "y": y,
                                    "clickCount": 1
                                }),
                            )
                            .await
                            .map_err(|e| {
                                CallToolError::from_message(format!("CDP Error: {:?}", e))
                            })?;

                        // Dispatch mouseReleased
                        let _ = cdp_client
                            .send_raw_command(
                                "Input.dispatchMouseEvent",
                                json!({
                                    "type": "mouseReleased",
                                    "button": "left",
                                    "x": x,
                                    "y": y,
                                    "clickCount": 1
                                }),
                            )
                            .await
                            .map_err(|e| {
                                CallToolError::from_message(format!("CDP Error: {:?}", e))
                            })?;

                        Ok(CallToolResult::text_content(vec![
                            format!(
                                "Successfully clicked on element '{}' at coordinates ({}, {})",
                                args.selector, x, y
                            )
                            .into(),
                        ]))
                    } else {
                        Err(CallToolError::from_message(
                            "Could not determine coordinates for the element.".to_string(),
                        ))
                    }
                } else {
                    Err(CallToolError::from_message(format!(
                        "Element not found for selector: {}",
                        args.selector
                    )))
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
    async fn test_click_element_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "click_element",
            "arguments": {
                "selector": "#login-button"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_click_element_tool_deserialization() {
        let tool: Result<ClickElementTool, _> = serde_json::from_value(json!({
            "selector": "#login-button"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().selector, "#login-button");
    }

    #[tokio::test]
    async fn test_click_element_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "click_element",
            "arguments": {
                "selector": "#test"
            }
        }))
        .unwrap();

        let result = ClickElementTool::handle(params, &handler).await;
        // In the mock server, if the result is not exactly right, we might get an error because the mock server
        // doesn't return the mocked coordinates. We'll update the mock server to return valid coords for this test.
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());
    }
}
