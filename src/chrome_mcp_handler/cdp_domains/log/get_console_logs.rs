use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "get_console_logs",
    description = "Retrieve console logs from the browser. This includes console.log/warn/error calls, exceptions, and network errors. Use this tool for troubleshooting page scripts and errors."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct GetConsoleLogsTool {
    /// If true, clears the internal console logs cache after retrieving the current logs. Use this to reset the state and only capture new logs going forward.
    #[serde(default)]
    pub clear: Option<bool>,

    /// Optional level filter (e.g., "error", "warning", "info", "log").
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
