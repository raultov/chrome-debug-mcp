use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "close_instance",
    description = "Closes and stops the specified Chrome instance by id. Side effects: destructive - for secondary instances the Chrome process is terminated and the instance is fully removed from the registry; for 'default' the process is stopped but the registry entry is kept and the instance is re-created lazily on the next tool call. Returns: structured JSON with the instance_id and whether it was removed from the registry. Use this to free resources used by sessions created with 'open_instance'. Alternatives: 'stop_chrome' to stop an instance, 'close_tab' to close a single tab."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct CloseInstanceTool {
    /// The instance id to close. The 'default' instance cannot be removed: it is stopped and kept (re-created lazily on next use); all other instances are fully removed.
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
            return Ok(CallToolResult::text_content(vec![serde_json::json!({
                "instance_id": "default",
                "stopped": true,
                "removed_from_registry": false,
                "note": "The default instance is stopped but kept; it is re-created lazily on the next tool call.",
            })
            .to_string()
            .into()]));
        }

        if let Some(session) = handler.registry.remove_session(&args.instance_id) {
            let mut mgr = session.chrome_manager.lock().await;
            mgr.stop_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to stop instance: {}", e))
            })?;
            Ok(CallToolResult::text_content(vec![
                serde_json::json!({
                    "instance_id": args.instance_id,
                    "stopped": true,
                    "removed_from_registry": true,
                })
                .to_string()
                .into(),
            ]))
        } else {
            Err(CallToolError::from_message(format!(
                "Instance '{}' not found.",
                args.instance_id
            )))
        }
    }
}
