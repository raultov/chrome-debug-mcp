use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "evaluate_on_call_frame",
    description = "Evaluates JavaScript expressions within the scope of a paused call frame, accessing local variables and call stack. Side effects: read-only by default; can modify state if expression includes mutations. Prerequisites: requires debugger to be paused at a breakpoint with active call frame. Returns: expression result with type and value. Use this to inspect variables and call stack during debugging. Alternatives: 'evaluate_js' for global scope evaluation, 'step_over' to advance without evaluation."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct EvaluateOnCallFrameTool {
    /// JavaScript expression to evaluate in call frame scope. Constraints: valid JavaScript accessing local/closure variables. Interactions: requires active paused debugger session; has access to function parameters and local variables. Defaults to: None (required).
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

        // Validation: check if we have a paused call frame ID FIRST
        {
            let st = handler.debugger_state.lock().await;
            if st.paused_call_frame_id.is_none() {
                return Err(CallToolError::from_message(
                    "No active call frame ID stored in debugger state.",
                ));
            }
        }

        let mut chrome_handler_lock = handler.get_or_connect().await?;
        let cdp_client = chrome_handler_lock.as_mut().unwrap();

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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_mcp_sdk::schema::CallToolRequestParams;

    #[tokio::test]
    async fn test_evaluate_on_call_frame_no_frame_error() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "evaluate_on_call_frame",
            "arguments": {
                "expression": "1 + 1"
            }
        }))
        .unwrap();

        let result = EvaluateOnCallFrameTool::handle(params, &handler).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No active call frame ID stored"));
    }
}
