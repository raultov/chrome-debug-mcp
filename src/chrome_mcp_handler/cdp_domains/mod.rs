pub mod debugger;
pub mod input;
pub mod network;
pub mod page;
pub mod runtime;

#[cfg(test)]
pub(crate) mod tests {
    use cdp_lite::client::CdpClient;
    use serde_json::json;
    use std::time::Duration;

    pub(crate) async fn spawn_mock_chrome_server() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            // 1. Handle HTTP request for /json/list (cdp-lite uses this)
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf).await;
                let body = format!(
                    "[{{\"type\": \"page\", \"webSocketDebuggerUrl\": \"ws://127.0.0.1:{}/devtools/page/1\", \"title\": \"Mock\", \"url\": \"http://mock\"}}]",
                    port
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    drop(stream);
                });
            }

            // 2. Handle WebSocket connection
            if let Ok((stream, _)) = listener.accept().await {
                let mut ws_stream = tokio_tungstenite::accept_async(stream).await.unwrap();
                use futures_util::{SinkExt, StreamExt};

                while let Some(msg) = ws_stream.next().await {
                    if let Ok(tokio_tungstenite::tungstenite::Message::Text(text)) = msg
                        && let Ok(req) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(id) = req.get("id").and_then(|i| i.as_i64())
                    {
                        // Reply with success
                        let mut result = json!({});
                        if let Some(method) = req.get("method").and_then(|m| m.as_str()) {
                            if method == "Runtime.evaluate" {
                                if let Some(params) = req.get("params")
                                    && let Some(expr) =
                                        params.get("expression").and_then(|e| e.as_str())
                                {
                                    if expr == "document.documentElement.outerHTML" {
                                        result = json!({
                                            "result": {
                                                "type": "string",
                                                "value": "<html><body><h1>Hello World</h1><div id='test'>This is a test</div></body></html>"
                                            }
                                        });
                                    } else if expr.contains("getBoundingClientRect") {
                                        result = json!({
                                            "result": {
                                                "type": "object",
                                                "value": {
                                                    "x": 100.5,
                                                    "y": 200.5
                                                }
                                            }
                                        });
                                    } else if expr.contains("focus()") {
                                        result = json!({
                                            "result": {
                                                "type": "boolean",
                                                "value": true
                                            }
                                        });
                                    }
                                }
                            } else if method == "Page.captureScreenshot" {
                                result = json!({
                                    "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII="
                                });
                            } else if method == "Page.getLayoutMetrics" {
                                result = json!({
                                    "contentSize": {
                                        "x": 0,
                                        "y": 0,
                                        "width": 1920,
                                        "height": 1080
                                    }
                                });
                            }
                        }

                        let reply = json!({
                            "id": id,
                            "result": result
                        });
                        let _ = ws_stream
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                reply.to_string().into(),
                            ))
                            .await;
                    }
                }
            }
        });

        port
    }

    #[tokio::test]
    async fn test_mock_chrome_server_connection() {
        let port = spawn_mock_chrome_server().await;
        let addr = format!("127.0.0.1:{}", port);

        let client_res = CdpClient::new(&addr, Duration::from_secs(2)).await;
        assert!(
            client_res.is_ok(),
            "Failed to connect to mock server: {:?}",
            client_res.err()
        );

        let client = client_res.unwrap();
        let res = client.send_raw_command("Runtime.enable", json!({})).await;
        assert!(res.is_ok(), "Failed to send command: {:?}", res.err());
    }

    #[tokio::test]
    async fn test_mock_chrome_server_multiple_commands() {
        let port = spawn_mock_chrome_server().await;
        let addr = format!("127.0.0.1:{}", port);

        let client = CdpClient::new(&addr, Duration::from_secs(2))
            .await
            .expect("Failed to connect");

        for i in 0..5 {
            let res = client
                .send_raw_command("Runtime.evaluate", json!({"expression": format!("{}", i)}))
                .await;
            assert!(res.is_ok(), "Command {} failed: {:?}", i, res.err());
        }
    }
}
pub mod log;
