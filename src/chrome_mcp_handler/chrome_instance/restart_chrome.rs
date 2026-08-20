use crate::chrome_mcp_handler::ChromeMcpHandler;
use crate::chrome_mcp_handler::chrome_instance::launch::ChromeFeature;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};

#[macros::mcp_tool(
    name = "restart_chrome",
    description = "Stops and restarts the managed Chrome instance with remote debugging enabled, optionally configuring proxy and opt-in capability presets. Side effects: destructive - terminates running Chrome process and all open tabs; closes debugging connection. Prerequisites: requires CHROME_PATH environment variable or chrome in PATH. Returns: restart success confirmation listing the presets applied. Use this to reset browser state, apply proxy settings, enable experimental browser capabilities, recover from crashes. Alternatives: 'reload' to refresh page without restart, 'navigate' to load new content."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct RestartChromeTool {
    /// Proxy server URL (e.g., 'http://proxy.example.com:8080'). Constraints: valid proxy URL with protocol and port. Interactions: applied to new Chrome instance; requires 'enable_proxy_auth' for authenticated proxies. Defaults to: None (no proxy).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_server: Option<String>,

    /// Chrome capability presets to enable on the new instance. Constraints: closed set - 'WEB_MCP' turns on the experimental WebMCP surface for sites that expose tools to the browser; 'WEBGL_SOFTWARE' forces SwiftShader software WebGL for GPU-less environments. Arbitrary Chrome flags are not accepted. Interactions: presets apply only to the instance started by this call and are cleared by a later restart that omits them. Defaults to: [] (no presets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<ChromeFeature>>,
}

impl RestartChromeTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: RestartChromeTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let mut manager = handler.chrome_manager.lock().await;

        // Reset the client connection before stopping/starting
        {
            let mut client_lock = handler.client.lock().await;
            *client_lock = None;
        }

        if let Err(e) = manager.stop_instance().await {
            return Err(CallToolError::from_message(format!(
                "Failed to stop Chrome: {}",
                e
            )));
        }

        manager.set_proxy(tool.proxy_server);

        let features = tool.features.unwrap_or_default();
        let summary = describe_features(&features);
        manager.set_features(features);

        if let Err(e) = manager.ensure_instance().await {
            return Err(CallToolError::from_message(format!(
                "Failed to start Chrome: {}",
                e
            )));
        }

        Ok(CallToolResult::text_content(vec![
            format!("Chrome instance restarted successfully. Features: {summary}.").into(),
        ]))
    }
}

/// Renders the requested presets for the tool response so the caller can confirm
/// what the restarted browser actually has enabled.
fn describe_features(features: &[ChromeFeature]) -> String {
    if features.is_empty() {
        return "none".to_string();
    }
    features
        .iter()
        .map(|f| f.as_name().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::chrome_instance::ChromeManager;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_restart_chrome_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": {}
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_restart_chrome_tool_deserialization() {
        let tool: Result<RestartChromeTool, _> = serde_json::from_value(json!({}));
        assert!(tool.is_ok());
        assert!(tool.unwrap().features.is_none());
    }

    #[tokio::test]
    async fn test_restart_chrome_handle() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(9999)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": {}
        }))
        .unwrap();

        let result = RestartChromeTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Chrome instance restarted successfully"),
            "Content didn't match: {}",
            content_str
        );
    }

    #[tokio::test]
    async fn given_known_feature_when_handling_then_manager_receives_it() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(9999)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": { "features": ["WEB_MCP"] }
        }))
        .unwrap();

        RestartChromeTool::handle(params, &handler)
            .await
            .expect("handle must succeed");

        let manager = handler.chrome_manager.lock().await;
        let mock = manager
            .as_any()
            .downcast_ref::<MockChromeManager>()
            .expect("manager must be the MockChromeManager");
        assert_eq!(mock.features(), &[ChromeFeature::WebMcp]);
    }

    #[tokio::test]
    async fn given_unknown_feature_when_handling_then_it_is_rejected() {
        let handler = ChromeMcpHandler::new_test();

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": { "features": ["DISABLE_WEB_SECURITY"] }
        }))
        .unwrap();

        let result = RestartChromeTool::handle(params, &handler).await;
        assert!(
            result.is_err(),
            "arbitrary presets must not reach the launcher"
        );
    }

    #[tokio::test]
    async fn given_no_features_when_handling_then_response_reports_none() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(9999)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "restart_chrome",
            "arguments": {}
        }))
        .unwrap();

        let call_result = RestartChromeTool::handle(params, &handler)
            .await
            .expect("handle must succeed");
        let content_str = format!("{:?}", call_result.content);
        assert!(content_str.contains("Features: none"), "{content_str}");
    }

    #[test]
    fn given_tool_schema_when_listed_then_it_constrains_features_to_the_allowed_presets() {
        let schema = serde_json::to_value(RestartChromeTool::tool().input_schema).unwrap();

        // The description alone mentions the preset names, so assert on the
        // machine-readable constraint instead. The derive emits one single-value
        // `enum` per variant, so compare the flattened union.
        let mut enumerations = Vec::new();
        collect_enums(&schema, &mut enumerations);
        let mut allowed: Vec<String> = enumerations.into_iter().flatten().collect();
        allowed.sort();
        allowed.dedup();

        assert_eq!(
            allowed,
            vec!["WEBGL_SOFTWARE".to_string(), "WEB_MCP".to_string()],
            "features must be schema-constrained to the closed preset list; schema was {schema:#}"
        );

        // Omitting the field must stay legal, otherwise a plain restart is
        // schema-invalid for strict clients.
        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|r| r.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(
            !required.contains(&"features"),
            "features must be optional; schema was {schema:#}"
        );
    }

    /// Walks the schema collecting every `enum` array of strings, wherever the
    /// derive decided to place it (inline, under `items`, or in `$defs`).
    fn collect_enums(value: &serde_json::Value, out: &mut Vec<Vec<String>>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(values)) = map.get("enum") {
                    out.push(
                        values
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect(),
                    );
                }
                for nested in map.values() {
                    collect_enums(nested, out);
                }
            }
            serde_json::Value::Array(items) => {
                for nested in items {
                    collect_enums(nested, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn given_features_when_describing_then_names_are_joined() {
        assert_eq!(describe_features(&[]), "none");
        assert_eq!(
            describe_features(&[ChromeFeature::WebMcp, ChromeFeature::WebglSoftware]),
            "WEB_MCP, WEBGL_SOFTWARE"
        );
    }
}
