pub mod profile_page_performance;

use cdp_browser_lite::WsResponse;
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
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state_clone: Arc<Mutex<TracingState>>,
) {
    let tracing_events = target.on_domain("Tracing");
    tokio::spawn(async move {
        crate::chrome_mcp_handler::cdp_domains::event_pump::pump_events(
            tracing_events,
            "Tracing",
            move |event| {
                let state = state_clone.clone();
                async move {
                    process_tracing_event(&event, &state).await;
                }
            },
        )
        .await;
    });
}
