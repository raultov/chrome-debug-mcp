pub mod capture_screenshot;
pub mod navigate;
pub mod reload;
pub mod scroll;

use crate::chrome_mcp_handler::NetworkState;
use cdp_browser_lite::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) async fn process_page_event(event: &WsResponse, state: &Arc<Mutex<NetworkState>>) {
    if event.method.as_deref() != Some("Page.frameNavigated") {
        return;
    }
    let Some(params) = &event.params else { return };
    let Some(frame) = params.get("frame") else {
        return;
    };
    // Main frame has no parentId. If parentId exists, it's an iframe navigation.
    if frame.get("parentId").is_some() {
        return;
    }
    state.lock().await.split_after_navigation();
}

pub(crate) fn start_page_listener(
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state_clone: Arc<Mutex<NetworkState>>,
) -> tokio::task::JoinHandle<()> {
    crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener(
        target,
        "Page",
        move |event| {
            let state = state_clone.clone();
            async move {
                process_page_event(&event, &state).await;
            }
        },
    )
}
