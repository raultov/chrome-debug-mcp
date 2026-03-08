use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "step_over",
    description = "Step over the current line of code in the debugger"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct StepOverTool {}

impl StepOverTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let _ = cdp_client
            .send_raw_command("Debugger.stepOver", cdp_lite::protocol::NoParams)
            .await
            .map_err(|e| CallToolError::from_message(format!("Failed to step over: {:?}", e)))?;

        Ok(CallToolResult::text_content(vec![
            "Stepped over to the next expression in the debugger.".into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;

    #[tokio::test]
    async fn test_step_over_structural() {
        let handler = ChromeMcpHandler::default();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "step_over",
            "arguments": {}
        })).unwrap();

        let result = StepOverTool::handle(params, &handler).await;
        if let Err(e) = result {
             let msg = e.to_string();
             assert!(msg.contains("Failed") || msg.contains("connect") || msg.contains("paused") || msg.contains("Timed out"));
        }
    }
}
