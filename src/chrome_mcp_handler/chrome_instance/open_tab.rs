use crate::chrome_mcp_handler::ChromeMcpHandler;
use crate::chrome_mcp_handler::cdp_domains;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "open_tab",
    description = "Opens a new tab in the specified Chrome instance. Side effects: creates a browser tab and registers it under a generated Tab ID; the tab stays open until closed with 'close_tab'. Prerequisites: none - the instance is launched lazily if not running yet. Returns: structured JSON with 'tab_id' (use it as the 'tab_id' argument of other tools), 'target_id' (raw CDP target id), 'url' and 'label'. Use this to work with several pages in parallel while keeping their CDP state isolated per tab. Alternatives: 'navigate' to load content in the active tab, 'list_tabs' to enumerate already open tabs."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct OpenTabTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// Target URL to navigate to in the new tab. Defaults to 'about:blank'.
    pub url: Option<String>,
    /// Optional unique label to identify the tab. Rejected if another tab in this instance already uses it.
    pub label: Option<String>,
}

impl OpenTabTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: OpenTabTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        // Ensure the instance is running
        {
            let mut manager = session.chrome_manager.lock().await;
            manager.ensure_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to ensure Chrome instance: {}", e))
            })?;
        }

        let url = args
            .url
            .clone()
            .unwrap_or_else(|| "about:blank".to_string());
        if handler.local_only && !crate::chrome_mcp_handler::is_local_address(&url) {
            return Err(CallToolError::from_message(format!(
                "Navigation to '{}' is blocked by local-only restrictions.",
                url
            )));
        }

        // Create the tab using the browser-level connection (browser_client)
        let browser_client = session.browser_client().await?;

        let tab = browser_client
            .new_tab(&url)
            .await
            .map_err(|e| CallToolError::from_message(format!("Failed to create new tab: {}", e)))?;

        let target_id = tab.target_id().to_string();

        // Register the tab in our TabRegistry
        let tab_id = {
            let mut registry = session.tabs.write().unwrap();
            registry
                .register_tab(tab.clone(), args.label.clone(), url.clone())
                .map_err(|e| {
                    CallToolError::from_message(format!(
                        "Failed to register tab in registry: {}",
                        e
                    ))
                })?
        };

        cdp_domains::enable_tab_domains(&tab).await;

        let states = {
            let registry = session.tabs.read().unwrap();
            registry
                .tabs
                .get(&tab_id)
                .map(|entry| entry.domain_states())
        };

        if let Some(states) = states {
            cdp_domains::start_tab_listeners(&tab, states);
        }

        Ok(CallToolResult::text_content(vec![
            serde_json::json!({
                "tab_id": tab_id,
                "target_id": target_id,
                "url": url,
                "label": args.label,
            })
            .to_string()
            .into(),
        ]))
    }
}
