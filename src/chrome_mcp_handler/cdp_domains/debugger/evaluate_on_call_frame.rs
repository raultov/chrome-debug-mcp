use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "evaluate_on_call_frame",
    description = "Evaluate an expression in the context of the currently paused debugger call frame"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct EvaluateOnCallFrameTool {
    pub expression: String,
}

impl EvaluateOnCallFrameTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: EvaluateOnCallFrameTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let call_frame_id = {
            let state = handler.debugger_state.lock().await;
            state.paused_call_frame_id.clone()
        };

        let call_frame_id = call_frame_id.ok_or_else(|| {
            CallToolError::from_message(
                "No active call frame ID stored. Ensure debugger is paused.".to_string(),
            )
        })?;

        let expression_result = cdp_client
            .send_raw_command(
                "Debugger.evaluateOnCallFrame",
                json!({
                    "callFrameId": call_frame_id,
                    "returnByValue": true,
                    "expression": args.expression
                }),
            )
            .await
            .map_err(|e| CallToolError::from_message(format!("Evaluation failed: {:?}", e)))?;

        Ok(CallToolResult::text_content(vec![
            format!("{:?}", expression_result).into(),
        ]))
    }
}
