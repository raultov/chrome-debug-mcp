use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "pause_on_load",
    description = "Enable debugger and pause on the first statement of the next executed script, then reload the page"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct PauseOnLoadTool {}

impl PauseOnLoadTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        // Enable debugger
        let _ = cdp_client
            .send_raw_command("Debugger.enable", cdp_lite::protocol::NoParams)
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to enable Debugger: {:?}", e))
            })?;

        // Add a script that executes `debugger;` at the very beginning of the new document
        let _ = cdp_client
            .send_raw_command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source": "debugger;"}),
            )
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to inject debugger statement: {:?}", e))
            })?;

        // Reload the page so the injected script runs and pauses execution
        let _ = cdp_client
            .send_raw_command("Page.reload", cdp_lite::protocol::NoParams)
            .await
            .map_err(|e| CallToolError::from_message(format!("Failed to reload page: {:?}", e)))?;

        Ok(CallToolResult::text_content(vec![
            "Debugger enabled and paused on the next executed statement. Page reloaded.".into(),
        ]))
    }
}
