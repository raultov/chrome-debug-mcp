use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "scroll",
    description = "Scrolls the page by pixel offset, viewport pages, or to a specific element using CSS selector. Side effects: modifies DOM scroll position (observable but reversible). Prerequisites: requires an active Chrome tab with content. Returns: scroll completion confirmation. Use this to navigate within long pages or bring elements into view. Alternatives: 'click_element' to trigger scroll by clicking, 'evaluate_js' for custom scroll logic."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ScrollTool {
    /// Horizontal scroll distance in pixels. Constraints: integer (positive=right, negative=left). Interactions: ignored if 'selector' is provided; combined with 'y' for diagonal scrolling. Defaults to: 0 (no horizontal scroll).
    pub x: Option<i32>,
    /// Vertical scroll distance in pixels. Constraints: integer (positive=down, negative=up). Interactions: ignored if 'selector' or 'pages' is provided; overridden by 'pages'. Defaults to: 0 (no vertical scroll).
    pub y: Option<i32>,
    /// Number of viewport heights to scroll vertically. Constraints: positive float (e.g., 1.5 = 1.5× viewport height). Interactions: takes precedence over 'y' parameter if both provided; ignored if 'selector' provided. Defaults to: None.
    pub pages: Option<f64>,
    /// CSS selector of element to scroll into view. Constraints: valid CSS selector string. Interactions: takes precedence over 'x', 'y', 'pages' if provided; fails if element not found. Defaults to: None.
    pub selector: Option<String>,
}

impl ScrollTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ScrollTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let expression = if let Some(ref selector) = args.selector {
            let safe_selector = selector.replace("\"", "\\\"");
            format!(
                "(function() {{
                    const el = document.querySelector(\"{}\");
                    if (el) {{
                        el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                        return true;
                    }}
                    return false;
                }})()",
                safe_selector
            )
        } else if let Some(pages) = args.pages {
            format!(
                "window.scrollBy({{ left: 0, top: window.innerHeight * {}, behavior: 'instant' }})",
                pages
            )
        } else {
            let dx = args.x.unwrap_or(0);
            let dy = args.y.unwrap_or(0);
            format!(
                "window.scrollBy({{ left: {}, top: {}, behavior: 'instant' }})",
                dx, dy
            )
        };

        let result = cdp_client
            .send_raw_command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true
                }),
            )
            .await;

        match result {
            Ok(val) => {
                let res_obj = val.result.as_ref().and_then(|r| r.get("result"));
                let value = res_obj.and_then(|r| r.get("value"));

                if let Some(ref selector) = args.selector
                    && value.and_then(|v| v.as_bool()) != Some(true)
                {
                    return Err(CallToolError::from_message(format!(
                        "Element not found for scrolling: {}",
                        selector
                    )));
                }

                Ok(CallToolResult::text_content(vec![
                    "Successfully scrolled.".into(),
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
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_scroll_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "scroll",
            "arguments": {
                "y": 500,
                "pages": 1.5,
                "selector": "#bottom"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_scroll_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "scroll",
            "arguments": {
                "y": 100
            }
        }))
        .unwrap();

        let result = ScrollTool::handle(params, &handler).await;
        assert!(result.is_ok());
    }
}
