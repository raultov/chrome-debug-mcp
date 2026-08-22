use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "list_tabs",
    description = "Lists all tabs currently open and registered in the specified Chrome instance. Side effects: none (read-only registry snapshot). Returns: structured JSON with 'active_tab_id', the current tab targeted by tools that omit 'tab_id', and a 'tabs' array of tab_id, label, target_id and url. When no tabs are registered, tools fall back to the instance's default single-tab connection and a 'note' explains it. Use this to discover Tab IDs before passing 'tab_id' to other tools. Alternatives: 'list_instances' to enumerate Chrome instances."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ListTabsTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
}

impl ListTabsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ListTabsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        let registry = session.tabs.read().unwrap();

        let tabs: Vec<serde_json::Value> = registry
            .tabs
            .iter()
            .map(|(id, entry)| {
                serde_json::json!({
                    "tab_id": id,
                    "label": entry.label,
                    "target_id": entry.tab.target_id(),
                    "url": entry.url,
                })
            })
            .collect();

        let result = if tabs.is_empty() {
            serde_json::json!({
                "active_tab_id": serde_json::Value::Null,
                "tabs": tabs,
                "note": "No tabs registered in this session. Tools that omit 'tab_id' fall back to the instance's default single-tab connection.",
            })
        } else {
            serde_json::json!({
                "active_tab_id": registry.active_tab_id,
                "tabs": tabs,
            })
        };

        Ok(CallToolResult::text_content(vec![
            result.to_string().into(),
        ]))
    }
}
