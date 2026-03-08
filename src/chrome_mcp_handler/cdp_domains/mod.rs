pub mod debugger;
pub mod page;
pub mod runtime;

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

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
                        let reply = json!({
                            "id": id,
                            "result": {}
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
}
