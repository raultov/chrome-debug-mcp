use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "close_instance",
    description = "Closes and stops the specified Chrome instance by id."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct CloseInstanceTool {
    /// The instance id to close.
    pub instance_id: String,
}

impl CloseInstanceTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: CloseInstanceTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        if args.instance_id == "default" {
            // Stop the default instance, but do not remove it from the registry
            let session = handler.default_session.clone();
            let mut mgr = session.chrome_manager.lock().await;
            mgr.stop_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to stop default instance: {}", e))
            })?;
            return Ok(CallToolResult::text_content(vec![
                "Stopped default Chrome instance (registry entry kept)."
                    .to_string()
                    .into(),
            ]));
        }

        if let Some(session) = handler.registry.remove_session(&args.instance_id) {
            let mut mgr = session.chrome_manager.lock().await;
            mgr.stop_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to stop instance: {}", e))
            })?;
            Ok(CallToolResult::text_content(vec![
                format!("Closed and stopped Chrome instance '{}'.", args.instance_id).into(),
            ]))
        } else {
            Err(CallToolError::from_message(format!(
                "Instance '{}' not found.",
                args.instance_id
            )))
        }
    }
}
