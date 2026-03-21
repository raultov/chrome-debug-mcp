use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult, ContentBlock, ImageContent},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "capture_screenshot",
    description = "Capture a screenshot of the current page. Returns a base64 encoded PNG image."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct CaptureScreenshotTool {
    /// Optional: Image format. Valid options: "png", "jpeg", "webp". Defaults to "png".
    pub format: Option<String>,
    /// Optional: JPEG or WEBP compression quality (0-100).
    pub quality: Option<u32>,
    /// Optional: Capture the full page layout (beyond the visible viewport). Defaults to false.
    pub full_page: Option<bool>,
}

impl CaptureScreenshotTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: CaptureScreenshotTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let format = args.format.unwrap_or_else(|| "png".to_string());

        let mut command_params = json!({
            "format": format,
            "fromSurface": true
        });

        if let Some(q) = args.quality {
            command_params
                .as_object_mut()
                .unwrap()
                .insert("quality".to_string(), json!(q));
        }

        if args.full_page.unwrap_or(false) {
            // 1. Get the metrics of the page
            let metrics_res = cdp_client
                .send_raw_command("Page.getLayoutMetrics", json!({}))
                .await
                .map_err(|e| {
                    CallToolError::from_message(format!("Failed to get metrics: {:?}", e))
                })?;

            let content_size = metrics_res
                .result
                .as_ref()
                .and_then(|r| r.get("contentSize"));

            if let Some(size) = content_size {
                let width = size.get("width").and_then(|v| v.as_f64()).unwrap_or(1280.0);
                let height = size.get("height").and_then(|v| v.as_f64()).unwrap_or(720.0);

                command_params.as_object_mut().unwrap().insert(
                    "clip".to_string(),
                    json!({
                        "x": 0,
                        "y": 0,
                        "width": width,
                        "height": height,
                        "scale": 1
                    }),
                );

                // Also enable captureBeyondViewport just in case for newer Chrome versions
                command_params
                    .as_object_mut()
                    .unwrap()
                    .insert("captureBeyondViewport".to_string(), json!(true));
            }
        }

        let result = cdp_client
            .send_raw_command("Page.captureScreenshot", command_params)
            .await;

        match result {
            Ok(val) => {
                let data = val
                    .result
                    .as_ref()
                    .and_then(|r| r.get("data"))
                    .and_then(|d| d.as_str());

                if let Some(base64_data) = data {
                    let mime_type = format!("image/{}", format);
                    let image_content = ImageContent::new(
                        base64_data.to_string(),
                        mime_type,
                        None, // annotations
                        None, // unknown fields
                    );

                    Ok(CallToolResult {
                        content: vec![ContentBlock::ImageContent(image_content)],
                        is_error: None,
                        meta: None,
                        structured_content: None,
                    })
                } else {
                    Err(CallToolError::from_message(
                        "No data returned from Page.captureScreenshot".to_string(),
                    ))
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
    async fn test_capture_screenshot_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "capture_screenshot",
            "arguments": {
                "format": "jpeg",
                "quality": 80,
                "full_page": true
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_capture_screenshot_tool_deserialization() {
        let tool: Result<CaptureScreenshotTool, _> = serde_json::from_value(json!({
            "format": "jpeg",
            "quality": 80,
            "full_page": true
        }));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.format.as_deref(), Some("jpeg"));
        assert_eq!(tool.quality, Some(80));
        assert_eq!(tool.full_page, Some(true));
    }

    #[tokio::test]
    async fn test_capture_screenshot_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "capture_screenshot",
            "arguments": {}
        }))
        .unwrap();

        let result = CaptureScreenshotTool::handle(params, &handler).await;
        // In the mock server, if the result is not exactly right, we might get an error because the mock server
        // doesn't return the mocked screenshot data. We'll update the mock server to return valid data.
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());
    }
}
