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
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
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
        let session = handler.session(args.instance_id.clone()).await?;
        let debugger_state = session.debugger_state(args.tab_id.clone())?;

        // Validation: check if we have a paused call frame ID FIRST. Reading
        // the per-tab state needs no connection, so this stays cheap.
        let call_frame_id = debugger_state
            .lock()
            .await
            .paused_call_frame_id
            .clone()
            .ok_or_else(|| {
                CallToolError::from_message(
                    "No active call frame ID stored in debugger state.".to_string(),
                )
            })?;

        let target = session.target(args.tab_id.clone()).await?;

        let expression_result = target
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

    /// Registers a real tab over the mock DevTools server so that explicit
    /// `tab_id` resolution routes state reads to the tab entry.
    async fn register_mock_tab(handler: &crate::chrome_mcp_handler::ChromeMcpHandler) -> String {
        use cdp_browser_lite::BrowserClient;
        use std::time::Duration;

        let session = handler.session(None).await.expect("default session");
        let port = crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server().await;
        let browser =
            BrowserClient::connect(&format!("127.0.0.1:{}", port), Duration::from_secs(5))
                .await
                .expect("BrowserClient connect to mock");
        let tab = browser.attach("T-page-1").await.expect("attach to mock");
        session
            .tabs
            .write()
            .unwrap()
            .register_tab(tab, None, "https://example.test".into(), false)
            .expect("register tab")
    }

    #[tokio::test]
    async fn test_evaluate_on_call_frame_reads_tab_state_not_session_state() {
        let handler = ChromeMcpHandler::new_test();
        let session = handler.session(None).await.unwrap();

        // Paused frame only in the SESSION state (would satisfy the old code).
        session
            .debugger_state(None)
            .unwrap()
            .lock()
            .await
            .paused_call_frame_id = Some("SESSION-CF".to_string());

        // Tab registered but its debugger state has NO paused frame.
        let tab_id = register_mock_tab(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "evaluate_on_call_frame",
            "arguments": { "expression": "1 + 1", "tab_id": tab_id }
        }))
        .unwrap();

        let err = EvaluateOnCallFrameTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("No active call frame ID stored"),
            "must read the TAB state (which has no paused frame), got: {err}"
        );
    }

    #[tokio::test]
    async fn test_evaluate_on_call_frame_unknown_tab_errors() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "evaluate_on_call_frame",
            "arguments": { "expression": "1 + 1", "tab_id": "tab-nope" }
        }))
        .unwrap();
        let err = EvaluateOnCallFrameTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected tab-not-found error, got: {err}"
        );
    }
}
