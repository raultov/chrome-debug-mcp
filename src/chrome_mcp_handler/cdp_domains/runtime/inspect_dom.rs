use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "inspect_dom",
    description = "Fetch the entire HTML of the current document"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct InspectDomTool {}

impl InspectDomTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let result = cdp_client
            .send_raw_command(
                "Runtime.evaluate",
                json!({
                    "expression": "document.documentElement.outerHTML",
                    "returnByValue": true
                }),
            )
            .await;

        match result {
            Ok(val) => {
                let formatted = format!("{:?}", val);
                Ok(CallToolResult::text_content(vec![formatted.into()]))
            }
            Err(e) => Err(CallToolError::from_message(format!("CDP Error: {:?}", e))),
        }
    }
}
