use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "list_instances",
    description = "Lists all running or registered Chrome instances. Side effects: none (read-only registry snapshot). Returns: JSON array of instance descriptors with id, label, host, port, profile_dir, features and is_default. Use this to discover instance_ids before passing 'instance_id' to other tools. Alternatives: 'list_tabs' to enumerate tabs within an instance."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ListInstancesTool {}

impl ListInstancesTool {
    pub async fn handle(
        _params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let descriptors = handler.registry.list_descriptors();
        let result_json = serde_json::to_string_pretty(&descriptors)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        Ok(CallToolResult::text_content(vec![result_json.into()]))
    }
}
