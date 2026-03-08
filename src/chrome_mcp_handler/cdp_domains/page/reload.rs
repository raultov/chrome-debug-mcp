use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(name = "reload", description = "Reload the current page")]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ReloadTool {}

impl ReloadTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let response = cdp_client.send_raw_command("Page.reload", json!({})).await;

        match response {
            Ok(resp) => Ok(CallToolResult::text_content(vec![
                format!("Page reloaded: {:?}", resp).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!(
                "Failed to reload page: {:?}",
                e
            ))),
        }
    }
}
