use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "set_breakpoint",
    description = "Set a debugger breakpoint at a specific script, line and column"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct SetBreakpointTool {
    pub script_hash: Option<String>,
    pub script_id: Option<String>,
    pub url: Option<String>,
    pub line_number: u32,
    pub column_number: Option<u32>,
}

impl SetBreakpointTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: SetBreakpointTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        if args.script_id.is_none() && args.url.is_none() && args.script_hash.is_none() {
            return Err(CallToolError::from_message(
                "Either script_id, url or script_hash must be provided",
            ));
        }

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let response = if let Some(script_id) = args.script_id {
            let mut location = json!({
                "scriptId": script_id,
                "lineNumber": args.line_number,
            });
            if let Some(col) = args.column_number {
                location
                    .as_object_mut()
                    .unwrap()
                    .insert("columnNumber".to_string(), json!(col));
            }
            cdp_client
                .send_raw_command("Debugger.setBreakpoint", json!({ "location": location }))
                .await
        } else if let Some(url) = args.url {
            let mut params = json!({
                "url": url,
                "lineNumber": args.line_number,
            });
            if let Some(col) = args.column_number {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("columnNumber".to_string(), json!(col));
            }
            cdp_client
                .send_raw_command("Debugger.setBreakpointByUrl", params)
                .await
        } else if let Some(script_hash) = args.script_hash {
            let mut params = json!({
                "scriptHash": script_hash,
                "lineNumber": args.line_number,
            });
            if let Some(col) = args.column_number {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("columnNumber".to_string(), json!(col));
            }
            cdp_client
                .send_raw_command("Debugger.setBreakpointByUrl", params)
                .await
        } else {
            return Err(CallToolError::from_message(
                "Either script_id, url or script_hash must be provided.".to_string(),
            ));
        };

        let response = response.map_err(|e| {
            CallToolError::from_message(format!("Failed to set breakpoint: {:?}", e))
        })?;

        Ok(CallToolResult::text_content(vec![
            format!("Breakpoint set successfully. Response: {:?}", response).into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::debugger::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_set_breakpoint_validation() {
        let handler = ChromeMcpHandler::new_test();

        // Missing all identifiers
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "set_breakpoint",
            "arguments": {
                "line_number": 10
            }
        }))
        .unwrap();
        let result = SetBreakpointTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Either script_id, url or script_hash must be provided")
        );
    }

    #[tokio::test]
    async fn test_set_breakpoint_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "set_breakpoint",
            "arguments": {
                "script_id": "1",
                "line_number": 10,
                "column_number": 5
            }
        }))
        .unwrap();

        let result = SetBreakpointTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Breakpoint set successfully"),
            "Content didn't match: {}",
            content_str
        );
    }
}
