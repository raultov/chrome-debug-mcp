use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "close_tab",
    description = "Closes a specific tab by its registered Tab ID. Side effects: destructive - removes the tab from the registry and closes it in Chrome; if it was the active tab, another registered tab becomes active. Prerequisites: the tab must exist (see 'list_tabs'). Returns: structured JSON with the closed 'tab_id' and the new 'active_tab_id' (null when no tabs remain). Use this to clean up tabs you no longer need. Alternatives: 'close_instance' to stop a whole Chrome instance."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct CloseTabTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the tab to close (e.g. 'tab-1').
    pub tab_id: String,
}

impl CloseTabTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: CloseTabTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        // 1. Remove from registry first to prevent stale references
        let entry_opt = {
            let mut registry = session.tabs.write().unwrap();
            registry.remove_tab(&args.tab_id)
        };

        let entry = entry_opt.ok_or_else(|| {
            CallToolError::from_message(format!("Tab ID '{}' not found", args.tab_id))
        })?;

        // 2. Instruct the browser client to close it
        let browser_client = {
            let manager = session.chrome_manager.lock().await;
            manager.browser_client().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to obtain browser client: {}", e))
            })?
        };

        browser_client
            .close_tab(entry.tab.target_id())
            .await
            .map_err(|e| {
                CallToolError::from_message(format!("Failed to close tab on browser: {}", e))
            })?;

        let new_active = session.tabs.read().unwrap().active_tab_id.clone();

        Ok(CallToolResult::text_content(vec![
            serde_json::json!({
                "closed_tab_id": args.tab_id,
                "active_tab_id": new_active,
            })
            .to_string()
            .into(),
        ]))
    }
}
