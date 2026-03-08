use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "stop_chrome",
    description = "Stops the managed Chrome instance. This tool should be used to ensure no zombie Chrome instances are left running after finishing operations with the MCP server."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct StopChromeTool {}

impl StopChromeTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut manager = handler.chrome_manager.lock().await;

        // Reset the client connection before stopping
        {
            let mut client_lock = handler.client.lock().await;
            *client_lock = None;
        }

        if let Err(e) = manager.stop_instance() {
            return Err(CallToolError::from_message(format!(
                "Failed to stop Chrome: {}",
                e
            )));
        }

        Ok(CallToolResult::text_content(vec![
            "Chrome instance stopped successfully.".into(),
        ]))
    }
}
