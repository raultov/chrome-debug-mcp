pub mod custom;
pub mod debugger;
pub mod fetch;
pub mod input;
pub mod network;
pub mod page;
pub mod runtime;
pub mod webmcp;

#[cfg(test)]
pub(crate) mod tests {
    use cdp_browser_lite::CdpClient;
    use serde_json::json;
    use std::time::Duration;

    pub(crate) async fn spawn_mock_chrome_server() -> u16 {
        use cdp_browser_lite::test_support::mock_devtools::{MockDevTools, MockWsBehavior};
        let mock = MockDevTools::start(MockWsBehavior::StayOpen).await;
        let port = mock.http_port;
        // Leak the mock so it keeps running during the test
        Box::leak(Box::new(mock));
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
pub(crate) mod cdp_target;
pub(crate) mod event_pump;
pub mod log;
pub mod performance;
pub mod tracing;
