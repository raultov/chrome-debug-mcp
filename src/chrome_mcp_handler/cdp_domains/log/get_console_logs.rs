use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "get_console_logs",
    description = "Retrieves cached console messages including log, warning, error, info levels and uncaught exceptions. Side effects: optionally clears cache when 'clear' is true. Prerequisites: requires an active Chrome tab. Returns: JSON array of console messages with timestamp, level, and text. Use this to debug script errors, monitor page health, inspect exception traces. Alternatives: browser DevTools Console, error logging services."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct GetConsoleLogsTool {
    /// Clear console cache after returning logs. Constraints: boolean. Interactions: when true, subsequent calls only return new messages. Defaults to: false.
    #[serde(default)]
    pub clear: Option<bool>,

    /// Filter logs by severity level (case-insensitive). Constraints: 'error', 'warning', 'info', 'log', or similar CDP log level. Interactions: when provided, returns only matching level; empty returns all. Defaults to: None (no filtering).
    #[serde(default)]
    pub level_filter: Option<String>,
}

impl GetConsoleLogsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: GetConsoleLogsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut logs = {
            let mut st = handler.log_state.lock().await;
            let current_logs = st.messages.clone();
            if args.clear.unwrap_or(false) {
                st.messages.clear();
            }
            current_logs
        };

        if let Some(level) = args.level_filter {
            let level_lower = level.to_lowercase();
            logs.retain(|msg| msg.level.to_lowercase() == level_lower);
        }

        let result_json = serde_json::to_value(&logs).unwrap_or_default();
        Ok(CallToolResult::text_content(vec![
            serde_json::to_string_pretty(&result_json)
                .unwrap_or_default()
                .into(),
        ]))
    }
}
