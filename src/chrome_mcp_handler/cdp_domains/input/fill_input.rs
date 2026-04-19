use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "fill_input",
    description = "Focuses an input field via CSS selector and inserts text using native input simulation, triggering input/change events. Side effects: modifies DOM input value; triggers input/change event handlers. Prerequisites: element must exist, be visible, and be an input/textarea or contenteditable element. Returns: success confirmation. Use this to populate form fields, search boxes, text areas. Alternatives: 'evaluate_js' for direct value assignment without events, 'click_element' to focus manually."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct FillInputTool {
    /// CSS selector identifying the input element. Constraints: valid CSS selector matching an input/textarea/contenteditable element. Interactions: element must be focusable and writable. Defaults to: None (required).
    pub selector: String,
    /// Text content to insert. Constraints: any string (special chars escaped automatically). Interactions: replaces any existing text after focus; triggers input/change events. Defaults to: None (required).
    pub text: String,
}

impl FillInputTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: FillInputTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        // Escaping double quotes in the selector just in case
        let safe_selector = args.selector.replace("\"", "\\\"");

        let expression = format!(
            "(function() {{
                const el = document.querySelector(\"{}\");
                if (!el) return false;
                el.focus();
                return true;
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
                let value_obj = result_obj.and_then(|r| r.get("value"));

                if value_obj.and_then(|v| v.as_bool()) == Some(true) {
                    // Element found and focused, now insert text
                    let _ = cdp_client
                        .send_raw_command(
                            "Input.insertText",
                            json!({
                                "text": args.text
                            }),
                        )
                        .await
                        .map_err(|e| CallToolError::from_message(format!("CDP Error: {:?}", e)))?;

                    Ok(CallToolResult::text_content(vec![
                        format!("Successfully filled input '{}' with text.", args.selector).into(),
                    ]))
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
    async fn test_fill_input_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "fill_input",
            "arguments": {
                "selector": "#username",
                "text": "my_user"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_fill_input_tool_deserialization() {
        let tool: Result<FillInputTool, _> = serde_json::from_value(json!({
            "selector": "#username",
            "text": "my_user"
        }));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.selector, "#username");
        assert_eq!(tool.text, "my_user");
    }

    #[tokio::test]
    async fn test_fill_input_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "fill_input",
            "arguments": {
                "selector": "#test-input",
                "text": "Hello World"
            }
        }))
        .unwrap();

        let result = FillInputTool::handle(params, &handler).await;
        // In the mock server, if the result is not exactly right, we might get an error.
        // We'll update the mock server to return true for focusing input test.
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());
    }
}
