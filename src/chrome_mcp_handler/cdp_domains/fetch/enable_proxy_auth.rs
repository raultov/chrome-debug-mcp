use crate::chrome_mcp_handler::ChromeMcpHandler;
use cdp_lite::protocol::NoParams;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;
use std::time::Duration;
use tokio_stream::StreamExt;

#[macros::mcp_tool(
    name = "enable_proxy_auth",
    description = "Enables proxy authentication using the Fetch domain. Call this after restarting Chrome with a proxy-server if your proxy requires authentication. It maintains active listening for up to 30 seconds of inactivity before automatically unhooking itself."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct EnableProxyAuthTool {
    pub username: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prewarm_url: Option<String>,
}

impl EnableProxyAuthTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let tool: EnableProxyAuthTool = serde_json::from_value(serde_json::Value::Object(
            params.arguments.unwrap_or_default(),
        ))
        .map_err(|e| CallToolError::from_message(format!("Failed to parse arguments: {}", e)))?;

        let prewarm_url = tool
            .prewarm_url
            .clone()
            .unwrap_or_else(|| "http://api.ipify.org?format=json".to_string());

        let mut client_lock = handler.get_or_connect().await?;
        if let Some(client) = client_lock.as_mut() {
            let resource_type = tool
                .resource_type
                .clone()
                .unwrap_or_else(|| "Document".to_string());
            let fetch_params = json!({
                "patterns": [
                    {
                        "urlPattern": "*",
                        "resourceType": resource_type,
                        "requestStage": "Request"
                    }
                ],
                "handleAuthRequests": true
            });
            client
                .send_raw_command("Fetch.enable", fetch_params)
                .await
                .map_err(|e| {
                    CallToolError::from_message(format!("Failed to enable Fetch domain: {}", e))
                })?;

            let mut fetch_events = client.on_domain("Fetch");
            let cdp_client_clone = client.clone();
            let cdp_client_nav = client.clone();
            let username = tool.username.clone();
            let password = tool.password.clone();

            tokio::spawn(async move {
                eprintln!("Proxy auth handler started. Waiting for challenges...");
                loop {
                    let event_result =
                        tokio::time::timeout(Duration::from_secs(30), fetch_events.next()).await;

                    match event_result {
                        Ok(Some(Ok(event))) => match event.method.as_deref() {
                            Some("Fetch.requestPaused") => {
                                let request_id = event
                                    .params
                                    .as_ref()
                                    .and_then(|p: &serde_json::Value| p.get("requestId"))
                                    .and_then(|v: &serde_json::Value| v.as_str());

                                if let Some(req_id) = request_id {
                                    let params = json!({"requestId": req_id});
                                    let _ = cdp_client_clone
                                        .send_raw_command("Fetch.continueRequest", params)
                                        .await;
                                }
                            }
                            Some("Fetch.authRequired") => {
                                let request_id = event
                                    .params
                                    .as_ref()
                                    .and_then(|p: &serde_json::Value| p.get("requestId"))
                                    .and_then(|v: &serde_json::Value| v.as_str());

                                if let Some(req_id) = request_id {
                                    eprintln!("Auth challenge received for requestId: {}", req_id);
                                    let params = json!({
                                        "requestId": req_id,
                                        "authChallengeResponse": {
                                            "response": "ProvideCredentials",
                                            "username": username,
                                            "password": password
                                        }
                                    });
                                    if let Err(e) = cdp_client_clone
                                        .send_raw_command("Fetch.continueWithAuth", params)
                                        .await
                                    {
                                        eprintln!("Failed to continue with auth: {}", e);
                                    } else {
                                        eprintln!(
                                            "Credentials supplied to browser for proxy auth."
                                        );
                                    }
                                }
                            }
                            _ => {}
                        },
                        Ok(Some(Err(e))) => {
                            eprintln!("Error receiving Fetch event: {}", e);
                            break;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            eprintln!(
                                "Proxy auth handler timed out after 30s of inactivity. Disabling Fetch domain."
                            );
                            let _ = cdp_client_clone
                                .send_raw_command("Fetch.disable", NoParams)
                                .await;
                            break;
                        }
                    }
                }
            });

            // Pre-warming task
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                eprintln!("Navigating to pre-warm URL: {}", prewarm_url);
                let params = json!({"url": prewarm_url});
                let _ = cdp_client_nav
                    .send_raw_command("Page.navigate", params)
                    .await;
            });
        }

        Ok(CallToolResult::text_content(vec![
            "Proxy authentication is configured and active (30s timeout). Pre-warming initiated."
                .into(),
        ]))
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
    async fn test_enable_proxy_auth_params_deserialization() {
        let params: Result<CallToolRequestParams, _> = serde_json::from_value(json!({
            "name": "enable_proxy_auth",
            "arguments": {
                "username": "user",
                "password": "pass"
            }
        }));
        assert!(params.is_ok());
    }

    #[tokio::test]
    async fn test_enable_proxy_auth_tool_deserialization_default() {
        let tool: Result<EnableProxyAuthTool, _> = serde_json::from_value(json!({
            "username": "user",
            "password": "pass"
        }));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.username, "user");
        assert_eq!(tool.password, "pass");
        assert_eq!(tool.resource_type, None);
        assert_eq!(tool.prewarm_url, None);
    }

    #[tokio::test]
    async fn test_enable_proxy_auth_tool_deserialization_with_optionals() {
        let tool: Result<EnableProxyAuthTool, _> = serde_json::from_value(json!({
            "username": "user",
            "password": "pass",
            "resource_type": "Image",
            "prewarm_url": "https://example.com"
        }));
        assert!(tool.is_ok());
        let tool = tool.unwrap();
        assert_eq!(tool.resource_type, Some("Image".to_string()));
        assert_eq!(tool.prewarm_url, Some("https://example.com".to_string()));
    }

    #[tokio::test]
    async fn test_enable_proxy_auth_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "enable_proxy_auth",
            "arguments": {
                "username": "testuser",
                "password": "testpassword"
            }
        }))
        .unwrap();

        let result = EnableProxyAuthTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
        let content_str = format!("{:?}", call_result.content);
        assert!(
            content_str.contains("Proxy authentication is configured"),
            "Content didn't match: {}",
            content_str
        );
    }
}
