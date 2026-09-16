pub mod get_custom_events;
pub mod send_cdp_command;

use crate::chrome_mcp_handler::{CustomEvent, CustomState};
use cdp_browser_lite::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex as StdMutex};

// Interner uses `std::sync::Mutex` because the critical section is a tiny
// synchronous hash lookup; calling `tokio::sync::Mutex::blocking_lock` from
// inside an async runtime panics with "Cannot block the current thread from
// within a runtime".
static DOMAIN_INTERNER: LazyLock<StdMutex<HashSet<&'static str>>> =
    LazyLock::new(|| StdMutex::new(HashSet::new()));

fn intern_domain(domain: &str) -> &'static str {
    let mut guard = DOMAIN_INTERNER
        .lock()
        .expect("domain interner mutex poisoned");
    if let Some(&existing) = guard.get(domain) {
        existing
    } else {
        let leaked: &'static str = Box::leak(domain.to_string().into_boxed_str());
        guard.insert(leaked);
        leaked
    }
}

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
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state: &Arc<Mutex<CustomState>>,
    domain: &str,
) {
    let mut st = state.lock().await;
    if !st.active_domains.contains(domain) {
        let domain_static = intern_domain(domain);
        let state_clone = state.clone();
        let handle = crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener(
            target,
            domain_static,
            move |event| {
                let state = state_clone.clone();
                async move {
                    process_custom_event(&event, &state).await;
                }
            },
        );
        st.handles.push(handle);
        st.active_domains.insert(domain.to_string());
    }
}

pub(crate) async fn clear_custom_listeners(state: &Arc<Mutex<CustomState>>) {
    let mut st = state.lock().await;
    st.handles = crate::chrome_mcp_handler::cdp_domains::event_pump::ListenerHandles::default();
    st.active_domains.clear();
}
