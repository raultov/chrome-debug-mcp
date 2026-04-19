use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "get_network_logs",
    description = "Retrieves intercepted HTTP/REST requests and WebSocket frames from network activity cache with filtering. Side effects: optionally clears cached logs when 'clear' is true. Prerequisites: requires active Chrome tab with network monitoring enabled. Returns: JSON array of requests/WebSocket frames with optional full details. Rate limits: none. Use this to audit API calls, debug network issues, inspect WebSocket traffic. Alternatives: browser DevTools Network tab, HAR file export."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct GetNetworkLogsTool {
    /// Clear network cache after returning logs. Constraints: boolean. Interactions: when true, subsequent calls return only new traffic. Defaults to: false.
    #[serde(default)]
    pub clear: Option<bool>,

    /// Traffic type to include. Constraints: 'rest', 'websocket', or 'both' (case-insensitive). Interactions: limits results to specified type. Defaults to: "both".
    #[serde(default)]
    pub type_filter: Option<String>,

    /// Partial URL match (case-insensitive). Constraints: non-empty string. Interactions: filters both REST and WebSocket URLs; empty string disables filtering. Defaults to: None (no URL filtering).
    #[serde(default)]
    pub url_filter: Option<String>,

    /// WebSocket frame direction filter. Constraints: 'sent', 'received', or 'both'. Interactions: applies only when type_filter includes 'websocket'. Defaults to: "both".
    #[serde(default)]
    pub ws_direction_filter: Option<String>,

    /// WebSocket payload substring match (case-insensitive). Constraints: non-empty string. Interactions: applies only when type_filter includes 'websocket'; filters by payload content. Defaults to: None (no content filtering).
    #[serde(default)]
    pub ws_content_filter: Option<String>,

    /// Include full request/response details. Constraints: boolean. Interactions: when false, returns summary only (URL, method, status); when true, includes headers, bodies. Defaults to: true.
    #[serde(default)]
    pub include_details: Option<bool>,
}

impl GetNetworkLogsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: GetNetworkLogsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let type_filter = args
            .type_filter
            .unwrap_or_else(|| "both".to_string())
            .to_lowercase();
        let want_rest = type_filter == "rest" || type_filter == "both";
        let want_ws = type_filter == "websocket" || type_filter == "both";
        let url_filter = args.url_filter.unwrap_or_default().to_lowercase();
        let ws_dir_filter = args
            .ws_direction_filter
            .unwrap_or_else(|| "both".to_string())
            .to_lowercase();
        let ws_content_filter = args.ws_content_filter.unwrap_or_default().to_lowercase();
        let include_details = args.include_details.unwrap_or(true);

        let (mut requests, mut ws_frames) = {
            let mut st = handler.network_state.lock().await;
            let reqs = st.requests.clone();
            let ws = st.ws_frames.clone();
            if args.clear.unwrap_or(false) {
                st.requests.clear();
                st.ws_frames.clear();
            }
            (reqs, ws)
        };

        if !want_rest {
            requests.clear();
        } else {
            requests.retain(|_, req| {
                if !url_filter.is_empty() && !req.url.to_lowercase().contains(&url_filter) {
                    return false;
                }
                true
            });

            if include_details {
                let client_lock_opt = handler.get_or_connect().await.ok();
                if let Some(mut client_lock) = client_lock_opt
                    && let Some(client) = client_lock.as_mut()
                {
                    for (req_id, req) in requests.iter_mut() {
                        if req.response_status.is_some()
                            && req.response_body.is_none()
                            && let Ok(body_resp) = client
                                .send_raw_command(
                                    "Network.getResponseBody",
                                    json!({"requestId": req_id}),
                                )
                                .await
                            && let Some(body) = body_resp
                                .result
                                .as_ref()
                                .and_then(|r| r.get("body"))
                                .and_then(|b| b.as_str())
                        {
                            req.response_body = Some(body.to_string());
                        }
                    }
                }
            }
        }

        if !want_ws {
            ws_frames.clear();
        } else {
            ws_frames.retain(|_, frames| {
                frames.retain(|f| {
                    if !url_filter.is_empty() && !f.url.to_lowercase().contains(&url_filter) {
                        return false;
                    }
                    if ws_dir_filter == "sent" && !f.is_sent {
                        return false;
                    }
                    if ws_dir_filter == "received" && f.is_sent {
                        return false;
                    }
                    if !ws_content_filter.is_empty()
                        && !f.payload_data.to_lowercase().contains(&ws_content_filter)
                    {
                        return false;
                    }
                    true
                });
                !frames.is_empty()
            });
        }

        let output_requests = if include_details {
            serde_json::to_value(&requests).unwrap_or_default()
        } else {
            let mut min_reqs = serde_json::Map::new();
            for (id, req) in requests {
                min_reqs.insert(
                    id,
                    json!({
                        "url": req.url,
                        "method": req.method,
                        "status": req.response_status,
                        "statusText": req.response_status_text,
                        "resourceType": req.resource_type
                    }),
                );
            }
            serde_json::Value::Object(min_reqs)
        };

        let output_ws = if include_details {
            serde_json::to_value(&ws_frames).unwrap_or_default()
        } else {
            let mut min_ws = serde_json::Map::new();
            for (id, frames) in ws_frames {
                let min_frames: Vec<_> = frames
                    .into_iter()
                    .map(|f| {
                        json!({
                            "url": f.url,
                            "is_sent": f.is_sent,
                            "payload_len": f.payload_data.len()
                        })
                    })
                    .collect();
                min_ws.insert(id, serde_json::to_value(min_frames).unwrap_or_default());
            }
            serde_json::Value::Object(min_ws)
        };

        let mut output = serde_json::Map::new();
        if want_rest {
            output.insert("requests".to_string(), output_requests);
        }
        if want_ws {
            output.insert("websocket_frames".to_string(), output_ws);
        }

        Ok(CallToolResult::text_content(vec![
            serde_json::to_string_pretty(&output)
                .unwrap_or_default()
                .into(),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::{NetworkRequest, WebSocketFrame};
    use rust_mcp_sdk::schema::ContentBlock;

    trait ContentBlockExt {
        fn as_text(&self) -> Option<&str>;
    }

    impl ContentBlockExt for ContentBlock {
        fn as_text(&self) -> Option<&str> {
            match self {
                ContentBlock::TextContent(t) => Some(&t.text),
                _ => None,
            }
        }
    }

    async fn setup_mock_data(handler: &ChromeMcpHandler) {
        let mut st = handler.network_state.lock().await;

        // Mock REST request
        st.requests.insert(
            "req-1".to_string(),
            NetworkRequest {
                url: "https://example.com/api/v1".to_string(),
                method: "GET".to_string(),
                resource_type: Some("XHR".to_string()),
                request_headers: None,
                request_post_data: None,
                response_status: Some(200),
                response_status_text: Some("OK".to_string()),
                response_headers: None,
                response_body: None,
            },
        );

        st.requests.insert(
            "req-2".to_string(),
            NetworkRequest {
                url: "https://google.com/search".to_string(),
                method: "GET".to_string(),
                resource_type: Some("Document".to_string()),
                request_headers: None,
                request_post_data: None,
                response_status: Some(200),
                response_status_text: Some("OK".to_string()),
                response_headers: None,
                response_body: None,
            },
        );

        // Mock WS frames
        st.ws_frames.insert(
            "ws-1".to_string(),
            vec![
                WebSocketFrame {
                    url: "wss://socket.com/feed".to_string(),
                    payload_data: "hello server".to_string(),
                    is_sent: true,
                },
                WebSocketFrame {
                    url: "wss://socket.com/feed".to_string(),
                    payload_data: "welcome client".to_string(),
                    is_sent: false,
                },
            ],
        );
    }

    #[tokio::test]
    async fn test_get_network_logs_filter_type_rest() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "type_filter": "rest"
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        assert!(json.get("requests").is_some());
        assert!(json.get("websocket_frames").is_none());
        assert_eq!(json["requests"].as_object().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_get_network_logs_filter_type_websocket() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "type_filter": "websocket"
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        assert!(json.get("requests").is_none());
        assert!(json.get("websocket_frames").is_some());
        assert_eq!(json["websocket_frames"].as_object().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_get_network_logs_filter_url() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "url_filter": "example.com"
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        assert_eq!(json["requests"].as_object().unwrap().len(), 1);
        assert!(json["requests"].as_object().unwrap().contains_key("req-1"));
        assert_eq!(json["websocket_frames"].as_object().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_get_network_logs_ws_direction_sent() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "type_filter": "websocket",
                "ws_direction_filter": "sent"
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        let frames = &json["websocket_frames"]["ws-1"];
        assert_eq!(frames.as_array().unwrap().len(), 1);
        assert!(frames[0]["is_sent"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_get_network_logs_ws_content_filter() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "ws_content_filter": "welcome"
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        let frames = &json["websocket_frames"]["ws-1"];
        assert_eq!(frames.as_array().unwrap().len(), 1);
        assert!(
            frames[0]["payload_data"]
                .as_str()
                .unwrap()
                .contains("welcome")
        );
    }

    #[tokio::test]
    async fn test_get_network_logs_include_details_false() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "include_details": false
            }
        }))
        .unwrap();

        let result = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json: serde_json::Value =
            serde_json::from_str(result.content[0].as_text().unwrap()).unwrap();

        // Check REST summary
        let req = &json["requests"]["req-1"];
        assert!(req.get("url").is_some());
        assert!(req.get("request_headers").is_none());
        assert!(req.get("statusText").is_some());

        // Check WS summary
        let frames = &json["websocket_frames"]["ws-1"];
        assert!(frames[0].get("payload_len").is_some());
        assert!(frames[0].get("payload_data").is_none());
    }

    #[tokio::test]
    async fn test_get_network_logs_clear() {
        let handler = ChromeMcpHandler::new_test();
        setup_mock_data(&handler).await;

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "get_network_logs",
            "arguments": {
                "clear": true
            }
        }))
        .unwrap();

        // First call should return data
        let result1 = GetNetworkLogsTool::handle(params.clone(), &handler)
            .await
            .unwrap();
        let json1: serde_json::Value =
            serde_json::from_str(result1.content[0].as_text().unwrap()).unwrap();
        assert!(!json1["requests"].as_object().unwrap().is_empty());

        // Second call should be empty
        let result2 = GetNetworkLogsTool::handle(params, &handler).await.unwrap();
        let json2: serde_json::Value =
            serde_json::from_str(result2.content[0].as_text().unwrap()).unwrap();
        assert!(json2["requests"].as_object().unwrap().is_empty());
        assert!(json2["websocket_frames"].as_object().unwrap().is_empty());
    }
}
