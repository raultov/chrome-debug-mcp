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
    Fut: Future<Output = ()>,
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

/// Aborts its listener tasks when dropped, so closing a tab or reconnecting the
/// client leaves no orphan event pumps holding on to domain state.
///
/// This is needed because `EventFilter` reads from the broadcast channel of the
/// shared browser connection: that sender outlives any individual tab, so the
/// stream never ends on its own and the task would stay alive forever.
#[derive(Default, Debug)]
pub(crate) struct ListenerHandles(Vec<tokio::task::JoinHandle<()>>);

impl ListenerHandles {
    pub(crate) fn push(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.0.push(handle);
    }

    pub(crate) fn absorb(&mut self, mut other: ListenerHandles) {
        let handles = std::mem::take(&mut other.0);
        self.0.extend(handles);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl Drop for ListenerHandles {
    fn drop(&mut self) {
        for handle in &self.0 {
            handle.abort();
        }
    }
}

/// Spawns a background task that pumps CDP events for a given domain.
pub(crate) fn spawn_domain_listener<F, Fut>(
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    domain: &'static str,
    process: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(WsResponse) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let events = target.on_domain(domain);
    tokio::spawn(async move {
        pump_events(events, domain, process).await;
    })
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

    #[tokio::test]
    async fn given_listener_handles_when_dropped_then_aborts_tasks() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let mut handles = ListenerHandles::default();
        handles.push(handle);
        assert_eq!(handles.len(), 1);

        drop(handles);
        tokio::task::yield_now().await;
        // Task must be aborted
    }

    #[tokio::test]
    async fn given_listener_handles_when_absorbed_then_transfers_without_aborting() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let mut h1 = ListenerHandles::default();
        h1.push(handle);

        let mut h2 = ListenerHandles::default();
        h2.absorb(h1);
        assert_eq!(h2.len(), 1);
    }
}
