use crate::chrome_mcp_handler::{ChromeMcpHandler, is_local_address};
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "navigate",
    description = "Navigates the current Chrome tab to a specified URL, loading new page content. Side effects: destructive of current page state; discards unsaved work. Prerequisites: requires an active Chrome tab; URL must be valid and accessible (subject to local-only restrictions if enabled). Returns: navigation confirmation. Auth requirements: subject to same-origin policy; may require credentials for restricted URLs. Rate limits: none. Use this to change the current page. Alternatives: 'reload' to refresh current page. Note: copy_cookies imports the user's real Chrome cookies, giving this browser access to their authenticated sessions — always ask the user before setting it to true. If the target instance is already running, setting copy_cookies is destructive: the instance is relaunched and all its open tabs are closed. In that case the call fails first with the list of tabs to be closed, so you can warn the user and re-issue with confirm_restart: true."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct NavigateTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
    /// Target URL to navigate to. Constraints: valid absolute URL (http/https/file). Interactions: navigation is blocked if MCP server started with 'local' flag and URL is not localhost/127.0.0.1/192.168.x.x/*.local. Defaults to: None (required).
    pub url: String,
    /// Optional flag to copy cookies from the user's real Chrome installation into this instance's isolated profile. Requires server running with --allow-cookie-import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_cookies: Option<bool>,
    /// Optional source profile name to copy cookies from (e.g. 'Default', 'Profile 1'). Defaults to the last used profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_profile: Option<String>,
    /// Acknowledges that importing cookies into an already-running instance requires relaunching it, closing all of its tabs. Only meaningful together with copy_cookies. Defaults to: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_restart: Option<bool>,
}

impl NavigateTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: NavigateTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;

        if handler.local_only && !is_local_address(&args.url) {
            return Err(CallToolError::from_message(format!(
                "Navigation to '{}' is blocked. This MCP server is running with the 'local' argument, which restricts navigation to local addresses only (localhost, 127.0.0.1, 192.168.x.x, or *.local). To allow navigation to external addresses, restart the MCP server without the 'local' argument.",
                args.url
            )));
        }

        if args.copy_cookies == Some(true) {
            if !handler.allow_cookie_import {
                return Err(CallToolError::from_message(
                    "Cookie import is disabled. Restart the MCP server with the '--allow-cookie-import' argument to enable it.".to_string()
                ));
            }
            if handler.base_params.user_profile {
                return Err(CallToolError::from_message(
                    "Cookie import is redundant when running in --user-profile mode, as the user's real profile is already in use.".to_string()
                ));
            }

            let source = crate::chrome_mcp_handler::chrome_instance::cookie_seed::resolve_source(
                args.source_profile.as_deref(),
                &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealEnvProvider,
                &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealFsProbe,
            )
            .map_err(CallToolError::from_message)?;

            let is_running = {
                let mgr = session.chrome_manager.lock().await;
                mgr.is_running().await
            };

            if is_running {
                if args.confirm_restart != Some(true) {
                    let instance_id_str = args.instance_id.as_deref().unwrap_or("default");
                    let (tab_count, tab_list) = {
                        let registry = session.tabs.read().unwrap();
                        let count = registry.tabs.len();
                        let list = registry
                            .tabs
                            .iter()
                            .map(|(id, entry)| {
                                let label_str = entry
                                    .label
                                    .as_ref()
                                    .map(|l| format!(" \"{l}\""))
                                    .unwrap_or_default();
                                let is_active = registry.active_tab_id.as_ref() == Some(id);
                                let active_str = if is_active { " (active)" } else { "" };
                                format!("  - {id}{label_str}{active_str}: {}", entry.url)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        (count, list)
                    };

                    let tab_info = if tab_count > 0 {
                        format!(
                            "its {tab_count} open tab(s), discarding their state (unsaved form input, breakpoints, and captured logs):\n{tab_list}"
                        )
                    } else {
                        "any open tabs".to_string()
                    };

                    return Err(CallToolError::from_message(format!(
                        "Cookie import requires relaunching instance '{instance_id_str}' with a seeded profile: a running browser cannot pick up cookies from disk.\n\nThis will CLOSE the instance and {tab_info}\n\nTell the user exactly which tabs will be closed and ask them to confirm. Only if they agree, call navigate again with the same arguments plus `confirm_restart: true`."
                    )));
                }

                let report = crate::chrome_mcp_handler::chrome_instance::cookie_seed::seed_cookies(
                    &source,
                    &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealEnvProvider,
                    &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealFsProbe,
                )
                .map_err(CallToolError::from_message)?;

                crate::chrome_mcp_handler::chrome_instance::seeded_relaunch::relaunch_seeded(
                    &session, report,
                )
                .await
                .map_err(CallToolError::from_message)?;
            } else if !is_running {
                let report = crate::chrome_mcp_handler::chrome_instance::cookie_seed::seed_cookies(
                    &source,
                    &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealEnvProvider,
                    &crate::chrome_mcp_handler::chrome_instance::cookie_seed::RealFsProbe,
                )
                .map_err(CallToolError::from_message)?;

                let mut mgr = session.chrome_manager.lock().await;
                mgr.set_seed_profile(Some(report));
            }
        }

        let target = session.target(args.tab_id.clone()).await?;

        let result = target
            .send_raw_command(
                "Page.navigate",
                json!({
                    "url": args.url
                }),
            )
            .await;

        match result {
            Ok(val) => Ok(CallToolResult::text_content(vec![
                format!("Navigated to {}. Protocol Response: {:?}", args.url, val).into(),
            ])),
            Err(e) => Err(CallToolError::from_message(format!("CDP Error: {:?}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_navigate_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_navigate_tool_deserialization() {
        let tool: Result<NavigateTool, _> = serde_json::from_value(json!({
            "url": "https://example.com"
        }));
        assert!(tool.is_ok());
        assert_eq!(tool.unwrap().url, "https://example.com");
    }

    #[tokio::test]
    async fn test_navigate_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com"
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Navigated to https://example.com"),
            "Content didn't match: {}",
            content_str
        );
    }

    #[tokio::test]
    async fn test_navigate_local_only_restriction() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.local_only = true;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://google.com"
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Navigation to 'https://google.com' is blocked")
        );

        // Test with local address
        let params_local: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "http://localhost:3000"
            }
        }))
        .unwrap();

        let port = spawn_mock_chrome_server().await;
        let mut handler_local = ChromeMcpHandler::new_test_with_port(port);
        handler_local.local_only = true;

        let result_local = NavigateTool::handle(params_local, &handler_local).await;
        assert!(
            result_local.is_ok(),
            "Local navigation should succeed: {:?}",
            result_local.err()
        );
    }

    #[tokio::test]
    async fn test_navigate_local_only_addresses() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.local_only = true;
        let port = spawn_mock_chrome_server().await;
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let local_urls = vec![
            "http://127.0.0.1:8080",
            "http://localhost:5173",
            "http://192.168.1.50/index.html",
            "http://myapp.local/",
        ];

        for url in local_urls {
            let params: CallToolRequestParams = serde_json::from_value(json!({
                "name": "navigate",
                "arguments": {
                    "url": url
                }
            }))
            .unwrap();
            let result = NavigateTool::handle(params, &handler).await;
            assert!(result.is_ok(), "URL {} should be allowed", url);
        }

        let blocked_urls = vec![
            "https://github.com",
            "http://10.0.0.1", // Currently not in my list, following prompt's specific list
            "http://1.1.1.1",
        ];

        for url in blocked_urls {
            let params: CallToolRequestParams = serde_json::from_value(json!({
                "name": "navigate",
                "arguments": {
                    "url": url
                }
            }))
            .unwrap();
            let result = NavigateTool::handle(params, &handler).await;
            assert!(result.is_err(), "URL {} should be blocked", url);
        }
    }

    #[tokio::test]
    async fn test_navigate_cookie_import_disabled_by_default() {
        let handler = ChromeMcpHandler::new_test();

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com",
                "copy_cookies": true
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Cookie import is disabled"));
    }

    #[tokio::test]
    async fn test_navigate_cookie_import_user_profile_error() {
        let mut handler = ChromeMcpHandler::new_test();
        handler.allow_cookie_import = true;
        handler.base_params.user_profile = true;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "navigate",
            "arguments": {
                "url": "https://example.com",
                "copy_cookies": true
            }
        }))
        .unwrap();

        let result = NavigateTool::handle(params, &handler).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("redundant when running in --user-profile mode")
        );
    }
}
