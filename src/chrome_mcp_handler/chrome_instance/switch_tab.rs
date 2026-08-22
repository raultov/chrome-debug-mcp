use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "switch_tab",
    description = "Switches the active default tab to the specified Tab ID. Side effects: the switched tab becomes the default target of all subsequent tool calls in this instance that omit 'tab_id'; when 'activate' is true it is also brought to the foreground in the Chrome window ('activate' defaults to true). Prerequisites: the tab must exist (see 'list_tabs'). Returns: structured JSON with the new 'active_tab_id' and whether the tab was brought to the foreground. Use this before interacting with a different page without repeating 'tab_id' on every call. Alternatives: pass 'tab_id' directly on an individual tool call to address a tab without switching."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct SwitchTabTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID to switch to (e.g. 'tab-1').
    pub tab_id: String,
    /// If true, brings the tab to the foreground in the browser. Defaults to true.
    pub activate: Option<bool>,
}

impl SwitchTabTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: SwitchTabTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        // 1. Obtener la tab y cambiar el ID activo
        let tab = {
            let mut registry = session.tabs.write().unwrap();
            registry
                .switch_tab(&args.tab_id)
                .map_err(|e| CallToolError::from_message(format!("Failed to switch tab: {}", e)))?;
            registry.tabs.get(&args.tab_id).unwrap().tab.clone()
        };

        // 2. Traer al frente en Chrome si se solicita
        let activate = args.activate.unwrap_or(true);
        if activate {
            tab.activate().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to activate tab in browser: {}", e))
            })?;
        }

        Ok(CallToolResult::text_content(vec![
            serde_json::json!({
                "active_tab_id": args.tab_id,
                "activated_in_browser": activate,
            })
            .to_string()
            .into(),
        ]))
    }
}
