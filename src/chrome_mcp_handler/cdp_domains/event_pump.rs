use cdp_browser_lite::{CdpResult, WsResponse};
use tokio_stream::{Stream, StreamExt};

/// Drives a CDP event stream, invoking `process` for every event it yields.
///
/// The stream yields `Err` when the underlying broadcast channel lags behind:
/// some events were dropped, but the stream itself stays usable. Treating that
/// as the end of the stream would silently kill the listener task and freeze
/// the domain's state cache for the rest of the session, so errors are reported
/// and the loop continues. Only stream exhaustion ends it.
///
/// Reporting goes to stderr on purpose: stdout carries the MCP JSON-RPC
/// protocol, and this crate pulls in no logging facade.
pub(crate) async fn pump_events<S, F, Fut>(mut events: S, domain: &'static str, mut process: F)
where
    S: Stream<Item = CdpResult<WsResponse>> + Unpin,
    F: FnMut(WsResponse) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    while let Some(item) = events.next().await {
        match item {
            Ok(event) => process(event).await,
            Err(e) => {
                eprintln!("[chrome-debug-mcp] {domain} event stream error, continuing: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdp_browser_lite::CdpError;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn event(method: &str) -> WsResponse {
        WsResponse {
            method: Some(method.to_string()),
            ..Default::default()
        }
    }

    /// Mirrors what `EventFilter` emits when the broadcast channel lags.
    fn lag() -> CdpError {
        CdpError::InternalError("Event stream lagged: 3".to_string())
    }

    async fn pump_and_collect(items: Vec<CdpResult<WsResponse>>) -> Vec<String> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        pump_events(
            tokio_stream::iter(items),
            "Test",
            move |event: WsResponse| {
                let sink = sink.clone();
                async move {
                    sink.lock()
                        .await
                        .push(event.method.clone().unwrap_or_default());
                }
            },
        )
        .await;
        seen.lock().await.clone()
    }

    #[tokio::test]
    async fn given_lagged_error_when_pumping_then_continues_with_next_event() {
        let seen = pump_and_collect(vec![
            Ok(event("Network.requestWillBeSent")),
            Err(lag()),
            Ok(event("Network.responseReceived")),
        ])
        .await;

        assert_eq!(
            seen,
            vec!["Network.requestWillBeSent", "Network.responseReceived"],
            "a lagging broadcast channel must not kill the listener task"
        );
    }

    #[tokio::test]
    async fn given_consecutive_errors_when_pumping_then_still_processes_later_events() {
        let seen =
            pump_and_collect(vec![Err(lag()), Err(lag()), Ok(event("Log.entryAdded"))]).await;

        assert_eq!(seen, vec!["Log.entryAdded"]);
    }

    #[tokio::test]
    async fn given_only_errors_when_pumping_then_processes_nothing_and_returns() {
        let seen = pump_and_collect(vec![Err(lag()), Err(lag())]).await;
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn given_all_ok_events_when_pumping_then_processes_all_in_order() {
        let seen = pump_and_collect(vec![
            Ok(event("A.one")),
            Ok(event("A.two")),
            Ok(event("A.three")),
        ])
        .await;

        assert_eq!(seen, vec!["A.one", "A.two", "A.three"]);
    }

    #[tokio::test]
    async fn given_empty_stream_when_pumping_then_returns_immediately() {
        let seen = pump_and_collect(vec![]).await;
        assert!(seen.is_empty());
    }
}
