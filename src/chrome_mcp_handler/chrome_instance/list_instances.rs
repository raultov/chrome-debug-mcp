use crate::chrome_mcp_handler::{ChromeMcpHandler, MIN_SUPPORTED_CHROME_MAJOR, parse_chrome_major};
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
        let mut warnings = Vec::new();

        for desc in &descriptors {
            if let Some(product) = desc
                .browser_version
                .as_ref()
                .and_then(|ver| ver.product.as_ref())
            {
                if let Some(major) = parse_chrome_major(product) {
                    if major < MIN_SUPPORTED_CHROME_MAJOR {
                        warnings.push(format!(
                            "[Warning] Instance '{}' is running an outdated browser ({}, major {} < minimum supported {}). Some DevTools features may not work as expected.",
                            desc.id, product, major, MIN_SUPPORTED_CHROME_MAJOR
                        ));
                    }
                } else if !desc.features.is_empty() {
                    warnings.push(format!(
                        "[Note] Instance '{}' is running an unrecognized browser product ('{}'). Requested feature presets ({}) cannot be verified.",
                        desc.id,
                        product,
                        desc.features.join(", ")
                    ));
                }
            }
        }

        let result_json = serde_json::to_string_pretty(&descriptors)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let mut content = vec![result_json.into()];
        if !warnings.is_empty() {
            content.push(warnings.join("\n\n").into());
        }

        Ok(CallToolResult::text_content(content))
    }
}
