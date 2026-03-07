use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "remove_breakpoint",
    description = "Remove a debugger breakpoint"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct RemoveBreakpointTool {
    pub breakpoint_id: String,
}

impl RemoveBreakpointTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: RemoveBreakpointTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let response = cdp_client
            .send_raw_command(
                "Debugger.removeBreakpoint",
                json!({ "breakpointId": args.breakpoint_id }),
            )
            .await;

        match response {
            Ok(resp) => Ok(CallToolResult::text_content(vec![
                format!("Breakpoint removed: {:?}", resp).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!(
                "Failed to remove breakpoint: {:?}",
                e
            ))),
        }
    }
}
