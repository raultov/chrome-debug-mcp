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
