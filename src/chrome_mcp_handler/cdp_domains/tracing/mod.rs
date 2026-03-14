pub mod profile_page_performance;

use cdp_lite::client::CdpClient;
use cdp_lite::protocol::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
pub(crate) struct TracingState {
    pub completion_channel: Option<tokio::sync::mpsc::Sender<String>>,
}

pub(crate) async fn process_tracing_event(event: &WsResponse, state: &Arc<Mutex<TracingState>>) {
    let method = match event.method.as_deref() {
        Some(m) => m,
        None => return,
    };

    if method == "Tracing.tracingComplete"
        && let Some(params) = &event.params
        && let Some(stream_id) = params.get("stream").and_then(|v| v.as_str())
    {
        let mut st = state.lock().await;
        if let Some(sender) = st.completion_channel.take() {
            let _ = sender.send(stream_id.to_string()).await;
        }
    }
}

pub(crate) fn start_tracing_listener(
    client: &mut CdpClient,
    state_clone: Arc<Mutex<TracingState>>,
) {
    let mut tracing_events = client.on_domain("Tracing");
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = tracing_events.next().await {
            process_tracing_event(&event, &state_clone).await;
        }
    });
}
