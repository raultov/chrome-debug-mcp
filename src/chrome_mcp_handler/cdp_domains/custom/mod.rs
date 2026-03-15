pub mod get_custom_events;
pub mod send_cdp_command;

use crate::chrome_mcp_handler::{CustomEvent, CustomState};
use cdp_lite::client::CdpClient;
use cdp_lite::protocol::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) async fn process_custom_event(event: &WsResponse, state: &Arc<Mutex<CustomState>>) {
    if let Some(method) = event.method.as_deref()
        && let Some(params) = &event.params
    {
        let mut st = state.lock().await;

        // Maintain a limit to avoid memory leaks
        if st.events.len() >= 1000 {
            st.events.pop_front();
        }

        st.events.push_back(CustomEvent {
            method: method.to_string(),
            params: params.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }
}

pub(crate) async fn ensure_domain_listener(
    client: &mut CdpClient,
    state: &Arc<Mutex<CustomState>>,
    domain: &str,
) {
    let mut st = state.lock().await;
    if !st.active_domains.contains(domain) {
        // Since we spawn a task that needs 'static, we leak the domain name string.
        // This is safe because there's a finite number of CDP domains.
        let domain_static: &'static str = Box::leak(domain.to_string().into_boxed_str());
        let mut events = client.on_domain(domain_static);
        let state_clone = state.clone();
        tokio::spawn(async move {
            use tokio_stream::StreamExt;
            while let Some(Ok(event)) = events.next().await {
                process_custom_event(&event, &state_clone).await;
            }
        });
        st.active_domains.insert(domain.to_string());
    }
}
