pub mod get_network_logs;

use crate::chrome_mcp_handler::NetworkRequest;
use crate::chrome_mcp_handler::NetworkState;
use crate::chrome_mcp_handler::WebSocketFrame;
use cdp_lite::client::CdpClient;
use cdp_lite::protocol::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) async fn process_network_event(event: &WsResponse, state: &Arc<Mutex<NetworkState>>) {
    let method = match event.method.as_deref() {
        Some(m) => m,
        None => return,
    };

    let params = match &event.params {
        Some(p) => p,
        None => return,
    };

    match method {
        "Network.requestWillBeSent" => {
            if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str())
                && let Some(req) = params.get("request")
            {
                let url = req
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let method = req
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let headers = req.get("headers").cloned();
                let post_data = req
                    .get("postData")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let mut st = state.lock().await;
                st.requests.insert(
                    request_id.to_string(),
                    NetworkRequest {
                        url,
                        method,
                        resource_type: None,
                        request_headers: headers,
                        request_post_data: post_data,
                        response_status: None,
                        response_status_text: None,
                        response_headers: None,
                        response_body: None,
                    },
                );
            }
        }
        "Network.responseReceived" => {
            if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str()) {
                let r_type = params
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(res) = params.get("response") {
                    let status = res.get("status").and_then(|v| v.as_i64());
                    let status_text = res
                        .get("statusText")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let headers = res.get("headers").cloned();

                    let mut st = state.lock().await;
                    if let Some(req) = st.requests.get_mut(request_id) {
                        req.response_status = status;
                        req.response_status_text = status_text;
                        req.response_headers = headers;
                        if req.resource_type.is_none() {
                            req.resource_type = r_type;
                        }
                    }
                }
            }
        }
        "Network.webSocketCreated" => {
            if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str()) {
                let url = params
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut st = state.lock().await;
                st.ws_connections.insert(request_id.to_string(), url);
            }
        }
        "Network.webSocketFrameSent" => {
            if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str())
                && let Some(response) = params.get("response")
            {
                let payload_data = response
                    .get("payloadData")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut st = state.lock().await;
                let url = st
                    .ws_connections
                    .get(request_id)
                    .cloned()
                    .unwrap_or_default();
                st.ws_frames
                    .entry(request_id.to_string())
                    .or_default()
                    .push(WebSocketFrame {
                        url,
                        payload_data,
                        is_sent: true,
                    });
            }
        }
        "Network.webSocketFrameReceived" => {
            if let Some(request_id) = params.get("requestId").and_then(|v| v.as_str())
                && let Some(response) = params.get("response")
            {
                let payload_data = response
                    .get("payloadData")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut st = state.lock().await;
                let url = st
                    .ws_connections
                    .get(request_id)
                    .cloned()
                    .unwrap_or_default();
                st.ws_frames
                    .entry(request_id.to_string())
                    .or_default()
                    .push(WebSocketFrame {
                        url,
                        payload_data,
                        is_sent: false,
                    });
            }
        }
        _ => {}
    }
}

pub(crate) fn start_network_listener(
    client: &mut CdpClient,
    state_clone: Arc<Mutex<NetworkState>>,
) {
    let mut network_events = client.on_domain("Network");
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = network_events.next().await {
            process_network_event(&event, &state_clone).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_event(method: &str, params: serde_json::Value) -> WsResponse {
        WsResponse {
            id: None,
            result: None,
            error: None,
            method: Some(method.to_string()),
            params: Some(params),
        }
    }

    #[tokio::test]
    async fn test_request_will_be_sent() {
        let state = Arc::new(Mutex::new(NetworkState::default()));
        let event = make_event(
            "Network.requestWillBeSent",
            json!({
                "requestId": "req-1",
                "request": {
                    "url": "https://example.com/api",
                    "method": "POST",
                    "headers": { "Content-Type": "application/json" },
                    "postData": "{\"foo\": \"bar\"}"
                }
            }),
        );

        process_network_event(&event, &state).await;

        let st = state.lock().await;
        let req = st.requests.get("req-1").unwrap();
        assert_eq!(req.url, "https://example.com/api");
        assert_eq!(req.method, "POST");
        assert_eq!(req.request_post_data.as_deref(), Some("{\"foo\": \"bar\"}"));
    }

    #[tokio::test]
    async fn test_response_received() {
        let state = Arc::new(Mutex::new(NetworkState::default()));
        // Pre-insert request
        {
            let mut st = state.lock().await;
            st.requests.insert(
                "req-1".to_string(),
                NetworkRequest {
                    url: "https://example.com".into(),
                    method: "GET".into(),
                    resource_type: None,
                    request_headers: None,
                    request_post_data: None,
                    response_status: None,
                    response_status_text: None,
                    response_headers: None,
                    response_body: None,
                },
            );
        }

        let event = make_event(
            "Network.responseReceived",
            json!({
                "requestId": "req-1",
                "type": "XHR",
                "response": {
                    "status": 200,
                    "statusText": "OK",
                    "headers": { "X-Test": "val" }
                }
            }),
        );

        process_network_event(&event, &state).await;

        let st = state.lock().await;
        let req = st.requests.get("req-1").unwrap();
        assert_eq!(req.response_status, Some(200));
        assert_eq!(req.response_status_text.as_deref(), Some("OK"));
        assert_eq!(req.resource_type.as_deref(), Some("XHR"));
    }

    #[tokio::test]
    async fn test_websocket_lifecycle() {
        let state = Arc::new(Mutex::new(NetworkState::default()));

        // 1. Created
        let event_created = make_event(
            "Network.webSocketCreated",
            json!({
                "requestId": "ws-1",
                "url": "wss://socket.com"
            }),
        );
        process_network_event(&event_created, &state).await;

        // 2. Frame Sent
        let event_sent = make_event(
            "Network.webSocketFrameSent",
            json!({
                "requestId": "ws-1",
                "response": { "payloadData": "ping" }
            }),
        );
        process_network_event(&event_sent, &state).await;

        // 3. Frame Received
        let event_received = make_event(
            "Network.webSocketFrameReceived",
            json!({
                "requestId": "ws-1",
                "response": { "payloadData": "pong" }
            }),
        );
        process_network_event(&event_received, &state).await;

        let st = state.lock().await;
        assert_eq!(st.ws_connections.get("ws-1").unwrap(), "wss://socket.com");
        let frames = st.ws_frames.get("ws-1").unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames[0].is_sent);
        assert_eq!(frames[0].payload_data, "ping");
        assert!(!frames[1].is_sent);
        assert_eq!(frames[1].payload_data, "pong");
    }
}
