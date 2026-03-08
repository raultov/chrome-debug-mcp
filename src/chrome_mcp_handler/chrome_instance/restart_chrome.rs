use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "restart_chrome",
    description = "Restarts the managed Chrome instance with remote debugging enabled. This tool can be used to start a new instance or restart an already existing one."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct RestartChromeTool {}

impl RestartChromeTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut manager = handler.chrome_manager.lock().await;

        // Reset the client connection before stopping/starting
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

        if let Err(e) = manager.ensure_instance().await {
            return Err(CallToolError::from_message(format!(
                "Failed to start Chrome: {}",
                e
            )));
        }

        Ok(CallToolResult::text_content(vec![
            "Chrome instance restarted successfully.".into(),
        ]))
    }
}
