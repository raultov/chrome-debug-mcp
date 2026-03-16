use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "send_cdp_command",
    description = "EXPERIMENTAL: Send a raw CDP command to the browser. Use ONLY if existing specialized tools (like navigate, click_element, etc.) do not satisfy your needs. Requires knowledge of the Chrome DevTools Protocol (CDP)."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct SendCdpCommandTool {
    /// The CDP method name (e.g., 'DOM.getDocument').
    pub method: String,
    /// A JSON string representing the parameters for the CDP command (e.g., '{"url": "https://example.com"}'). Omit or provide '{}' if no parameters are needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
}

impl SendCdpCommandTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: SendCdpCommandTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let parsed_params: serde_json::Value = match &tool.params {
            Some(s) if !s.trim().is_empty() => serde_json::from_str(s).map_err(|e| {
                CallToolError::from_message(format!("Invalid JSON in params: {}", e))
            })?,
            _ => serde_json::Value::Object(Default::default()),
        };

        let mut client_lock = handler.get_or_connect().await?;
        if let Some(client) = client_lock.as_mut() {
            // Extract domain from method (e.g., "DOM" from "DOM.getDocument")
            if let Some(domain) = tool.method.split('.').next() {
                super::ensure_domain_listener(client, &handler.custom_state, domain).await;
            }

            let response = client.send_raw_command(&tool.method, parsed_params).await;
            match response {
                Ok(res) => Ok(CallToolResult::text_content(vec![
                    format!(
                        "Command '{}' executed successfully. Result: {:?}",
                        tool.method, res
                    )
                    .into(),
                ])),
                Err(e) => Err(CallToolError::from_message(format!(
                    "Failed to execute CDP command '{}': {}",
                    tool.method, e
                ))),
            }
        } else {
            Err(CallToolError::from_message("Not connected to Chrome"))
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
    async fn test_send_cdp_command_schema() {
        let tool_def = SendCdpCommandTool::tool();
        println!("{}", serde_json::to_string_pretty(&tool_def).unwrap());
    }

    #[tokio::test]
    async fn test_send_cdp_command_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "send_cdp_command",
            "arguments": {
                "method": "Runtime.enable",
                "params": "{}"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_send_cdp_command_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "send_cdp_command",
            "arguments": {
                "method": "Runtime.enable",
                "params": "{}"
            }
        }))
        .unwrap();

        let result = SendCdpCommandTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Command 'Runtime.enable' executed successfully"),
            "Content didn't match: {}",
            content_str
        );

        // Verify active_domains has 'Runtime'
        let st = handler.custom_state.lock().await;
        assert!(st.active_domains.contains("Runtime"));
    }
}
