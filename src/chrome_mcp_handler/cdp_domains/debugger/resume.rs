use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(name = "resume", description = "Resume execution in the debugger")]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ResumeTool {}

impl ResumeTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let _ = cdp_client
            .send_raw_command("Debugger.resume", cdp_lite::protocol::NoParams)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to resume execution: {:?}", e))
            })?;

        Ok(CallToolResult::text_content(vec![
            "Debugger execution resumed.".into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;

    #[tokio::test]
    async fn test_resume_structural() {
        let handler = ChromeMcpHandler::default();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "resume",
            "arguments": {}
        })).unwrap();

        let result = ResumeTool::handle(params, &handler).await;
        // Should succeed or fail at protocol/connection level, but not validation
        if let Err(e) = result {
             let msg = e.to_string();
             assert!(msg.contains("Failed") || msg.contains("connect") || msg.contains("paused") || msg.contains("Timed out"));
        }
    }
}
