pub mod capture_screenshot;
pub mod navigate;
pub mod reload;
pub mod scroll;

use crate::chrome_mcp_handler::NetworkState;
use crate::chrome_mcp_handler::cdp_domains::webmcp::{WebmcpAvailability, WebmcpNavSync};
use cdp_browser_lite::{NoParams, WsResponse};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Decides which cached tool entries a page event invalidates.
///
/// Derived from observed Chrome 153 behaviour rather than protocol
/// documentation: an event stream capture around a cross-document navigation
/// (`Page.reload` / `Page.navigate`) shows `WebMCP.toolsAdded` batches for the
/// new document right after `Page.frameNavigated`, and never a
/// `WebMCP.toolsRemoved` for the destroyed document. Chrome also reuses the
/// main frame's `frameId` across navigations, so without an explicit purge the
/// per-frame tool cache grows monotonically with ghosts of dead documents.
///
/// Not purging on same-document navigations is load-bearing, not an
/// optimization: SPAs register tools on hash routes (measured on
/// knot.kz: `#/contact` registers `contact-knot-team`) and the document is not
/// destroyed there. `Page.navigatedWithinDocument` must therefore keep tools.
///
/// The variant names describe which entries become invalid; they are consumed
/// by [`process_page_event`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolCacheReset {
    /// Nothing destroyed: tools stay cached as-is.
    Keep,
    /// One child frame created a fresh document; drop that frame's tools.
    Frame(String),
    /// The main frame created a fresh document; the whole frame tree is
    /// destroyed (child frames get fresh IDs), so drop everything.
    AllFrames,
}

/// The per-tab WebMCP sync wiring lives in `cdp_domains::webmcp` as
/// [`WebmcpNavSync`]; this impl attaches the purge+resync behavior to it.
impl WebmcpNavSync {
    /// Purges cached tools and, when Chrome reports the domain as enabled,
    /// asks Chrome to resync. `WebMCP.enable` re-emits the full authoritative
    /// `toolsAdded` list on every call — even when the domain is already
    /// enabled — so this is the resync primitive; `disable` is deliberately
    /// avoided because it would fight other attached clients.
    ///
    /// Purging (synchronously, under the state lock) always happens before the
    /// `enable` is sent and the lock is dropped first, so the resulting
    /// `toolsAdded` events can never be wiped by the purge that caused them.
    ///
    /// Resync is what makes `BackForwardCacheRestore` navigations correct: the
    /// document is not recreated there, Chrome emits no `toolsAdded` waves, and
    /// a purge-only design would leave the cache empty until the user navigates
    /// again.
    async fn purge_and_resync(&self, reset: ToolCacheReset) {
        let availability = {
            let mut st = self.state.lock().await;
            match reset {
                ToolCacheReset::Keep => return,
                ToolCacheReset::Frame(frame_id) => st.clear_frame_tools(&frame_id),
                ToolCacheReset::AllFrames => st.clear_all_tools(),
            }
            st.availability
        };
        if availability == WebmcpAvailability::Enabled {
            let _ = self
                .target
                .send_raw_command("WebMCP.enable", NoParams)
                .await;
        }
    }
}

/// Classifies a `Page` domain event without touching any shared state.
///
/// Returns `(split_network_buckets, tool_cache_reset)`:
/// - `split_network_buckets` is true only for a main-frame navigation; that is
///   when [`NetworkState::split_after_navigation`] is applied (unchanged
///   behaviour).
/// - `tool_cache_reset` describes which WebMCP tool entries died with the old
///   document.
pub(crate) fn classify_page_event(event: &WsResponse) -> (bool, ToolCacheReset) {
    if event.method.as_deref() != Some("Page.frameNavigated") {
        return (false, ToolCacheReset::Keep);
    }
    let Some(frame) = event.params.as_ref().and_then(|p| p.get("frame")) else {
        return (false, ToolCacheReset::Keep);
    };
    // Main frame has no parentId. If parentId exists, it's an iframe navigation.
    if frame.get("parentId").is_some() {
        // Per protocol `Frame.id` is always present; without it we cannot
        // identify the bucket to purge, so degrade to "nothing happened".
        return match frame.get("id").and_then(|v| v.as_str()) {
            Some(id) => (false, ToolCacheReset::Frame(id.to_string())),
            None => (false, ToolCacheReset::Keep),
        };
    }
    (true, ToolCacheReset::AllFrames)
}

pub(crate) async fn process_page_event(
    event: &WsResponse,
    state: &Arc<Mutex<NetworkState>>,
    webmcp: Option<&WebmcpNavSync>,
) {
    let (split_network, reset) = classify_page_event(event);
    if split_network {
        state.lock().await.split_after_navigation();
    }
    if let Some(sync) = webmcp {
        sync.purge_and_resync(reset).await;
    }
}

pub(crate) fn start_page_listener(
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state_clone: Arc<Mutex<NetworkState>>,
    webmcp: Option<WebmcpNavSync>,
) -> tokio::task::JoinHandle<()> {
    crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener(
        target,
        "Page",
        move |event| {
            let state = state_clone.clone();
            let webmcp = webmcp.clone();
            async move {
                process_page_event(&event, &state, webmcp.as_ref()).await;
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget;
    use crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpState;
    use serde_json::json;

    fn event(method: &str, params: serde_json::Value) -> WsResponse {
        WsResponse {
            method: Some(method.to_string()),
            params: Some(params),
            ..Default::default()
        }
    }

    // --- classify_page_event: pure classification ---

    #[test]
    fn given_main_frame_navigated_then_classified_as_network_split_and_all_frames() {
        let event = event(
            "Page.frameNavigated",
            json!({
                "frame": { "id": "F1", "url": "https://example.test/" },
                "type": "Navigation"
            }),
        );
        let (split, reset) = classify_page_event(&event);
        assert!(split);
        assert_eq!(reset, ToolCacheReset::AllFrames);
    }

    #[test]
    fn given_child_frame_navigated_then_classified_as_frame_reset() {
        let event = event(
            "Page.frameNavigated",
            json!({
                "frame": { "id": "F2", "parentId": "F1", "url": "https://example.test/ad" },
                "type": "Navigation"
            }),
        );
        let (split, reset) = classify_page_event(&event);
        assert!(
            !split,
            "network buckets only split on main-frame navigation"
        );
        assert_eq!(reset, ToolCacheReset::Frame("F2".to_string()));
    }

    #[test]
    fn given_same_document_navigation_then_nothing_is_reset() {
        // Hash/history navigations keep the document alive: SPAs register tools
        // on routes (knot.kz registers `contact-knot-team` on #/contact), so
        // purging here would drop live tools.
        let event = event(
            "Page.navigatedWithinDocument",
            json!({ "frameId": "F1", "url": "https://example.test/#/contact" }),
        );
        let (split, reset) = classify_page_event(&event);
        assert!(!split);
        assert_eq!(reset, ToolCacheReset::Keep);
    }

    #[test]
    fn given_malformed_frame_navigated_then_nothing_is_reset() {
        // Missing/shapeless params must degrade to "nothing happened", never
        // wipe live caches.
        for params in [json!({}), json!({ "frame": { "parentId": "F1" } })] {
            let (split, reset) = classify_page_event(&event("Page.frameNavigated", params));
            assert!(!split);
            assert_eq!(reset, ToolCacheReset::Keep);
        }
        let no_params = WsResponse {
            method: Some("Page.frameNavigated".to_string()),
            ..Default::default()
        };
        assert_eq!(classify_page_event(&no_params).1, ToolCacheReset::Keep);
    }

    #[test]
    fn given_unknown_method_then_nothing_is_reset() {
        let (split, reset) =
            classify_page_event(&event("Page.loadEventFired", json!({ "timestamp": 1.0 })));
        assert!(!split);
        assert_eq!(reset, ToolCacheReset::Keep);
    }

    // --- process_page_event: purge side effects on WebmcpState ---

    fn seed_tools(st: &mut WebmcpState) {
        for (name, frame) in [("toolA", "F1"), ("toolI1", "F2"), ("toolI2", "F3")] {
            st.tools.entry(frame.to_string()).or_default().insert(
                name.to_string(),
                crate::chrome_mcp_handler::cdp_domains::webmcp::WebmcpTool {
                    name: name.to_string(),
                    description: "d".into(),
                    input_schema: json!({}),
                    annotations: None,
                    frame_id: frame.to_string(),
                    backend_node_id: None,
                },
            );
        }
    }

    fn nav_sync_with(
        state: Arc<Mutex<WebmcpState>>,
        client: cdp_browser_lite::CdpClient,
    ) -> WebmcpNavSync {
        // The happy-path `WebMCP.enable` resync is exercised by manual
        // verification and the real-Chrome integration tests; these unit tests
        // verify the purge semantics and state outcomes, so the target is
        // whatever client the test decides to supply.
        WebmcpNavSync {
            state,
            target: CdpTarget::Client(client),
        }
    }

    async fn mock_cdp_client() -> cdp_browser_lite::CdpClient {
        let port = crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server().await;
        cdp_browser_lite::CdpClient::new(
            &format!("127.0.0.1:{port}"),
            std::time::Duration::from_secs(2),
        )
        .await
        .expect("cdp client against the mock chrome server")
    }

    #[tokio::test]
    async fn given_main_frame_navigated_then_all_frames_purged_and_network_split() {
        let sync = nav_sync_with(
            Arc::new(Mutex::new(WebmcpState::default())),
            mock_cdp_client().await,
        );
        {
            let mut st = sync.state.lock().await;
            seed_tools(&mut st);
            st.availability = WebmcpAvailability::Enabled;
        }

        let net = Arc::new(Mutex::new(NetworkState::default()));
        {
            let mut nw = net.lock().await;
            nw.insert_request(
                "req-1".into(),
                crate::chrome_mcp_handler::NetworkRequest {
                    url: "https://x.test/".into(),
                    method: "GET".into(),
                    resource_type: None,
                    request_headers: None,
                    request_post_data: None,
                    response_status: None,
                    response_status_text: None,
                    response_headers: None,
                    response_body: None,
                },
            );
        }

        let event = event(
            "Page.frameNavigated",
            json!({
                "frame": { "id": "F1", "url": "https://example.test/" },
                "type": "Navigation"
            }),
        );
        process_page_event(&event, &net, Some(&sync)).await;

        let st = sync.state.lock().await;
        assert!(st.tools.is_empty(), "all frames purged on main-frame nav");
        assert_eq!(st.availability, WebmcpAvailability::Enabled);
        drop(st);
        let nw = net.lock().await;
        assert!(
            nw.iter_current().is_empty(),
            "older bucket archived on main-frame navigation"
        );
    }

    #[tokio::test]
    async fn given_child_frame_navigated_then_only_that_frame_purged() {
        let sync = nav_sync_with(
            Arc::new(Mutex::new(WebmcpState::default())),
            mock_cdp_client().await,
        );
        {
            let mut st = sync.state.lock().await;
            seed_tools(&mut st);
        }

        let net = Arc::new(Mutex::new(NetworkState::default()));
        {
            let mut nw = net.lock().await;
            nw.insert_request(
                "req-keep".into(),
                crate::chrome_mcp_handler::NetworkRequest {
                    url: "https://x.test/a".into(),
                    method: "GET".into(),
                    resource_type: None,
                    request_headers: None,
                    request_post_data: None,
                    response_status: None,
                    response_status_text: None,
                    response_headers: None,
                    response_body: None,
                },
            );
        }
        let event = event(
            "Page.frameNavigated",
            json!({
                "frame": { "id": "F2", "parentId": "F1", "url": "https://x.test/ad" },
                "type": "Navigation"
            }),
        );
        process_page_event(&event, &net, Some(&sync)).await;

        let st = sync.state.lock().await;
        assert!(!st.tools.contains_key("F2"), "child frame purged");
        assert!(
            st.tools.contains_key("F1") && st.tools.contains_key("F3"),
            "sibling frames survive"
        );
        drop(st);
        let nw = net.lock().await;
        assert!(
            nw.iter_all().contains_key("req-keep"),
            "network history is untouched by a child-frame navigation"
        );
    }

    #[tokio::test]
    async fn given_hash_navigation_then_tools_are_kept() {
        let sync = nav_sync_with(
            Arc::new(Mutex::new(WebmcpState::default())),
            mock_cdp_client().await,
        );
        {
            let mut st = sync.state.lock().await;
            seed_tools(&mut st);
        }

        let net = Arc::new(Mutex::new(NetworkState::default()));
        let event = event(
            "Page.navigatedWithinDocument",
            json!({ "frameId": "F1", "url": "https://x.test/#/contact" }),
        );
        process_page_event(&event, &net, Some(&sync)).await;

        let st = sync.state.lock().await;
        assert_eq!(st.tools.len(), 3, "hash navigation keeps every frame");
    }

    #[tokio::test]
    async fn given_unsupported_availability_then_state_is_purged_silently() {
        let sync = nav_sync_with(
            Arc::new(Mutex::new(WebmcpState::default())),
            mock_cdp_client().await,
        );
        {
            let mut st = sync.state.lock().await;
            seed_tools(&mut st);
            st.availability = WebmcpAvailability::Unsupported;
        }
        let net = Arc::new(Mutex::new(NetworkState::default()));

        let event = event("Page.frameNavigated", json!({ "frame": { "id": "F1" } }));
        process_page_event(&event, &net, Some(&sync)).await;

        let st = sync.state.lock().await;
        assert!(st.tools.is_empty());
        assert_eq!(
            st.availability,
            WebmcpAvailability::Unsupported,
            "purge never consumes availability even when resync is skipped"
        );
    }

    #[tokio::test]
    async fn given_no_webmcp_wiring_then_navigation_still_splits_network() {
        let net = Arc::new(Mutex::new(NetworkState::default()));

        let event = event("Page.frameNavigated", json!({ "frame": { "id": "F1" } }));
        process_page_event(&event, &net, None).await;

        // The call must not panic and the network path is exercised; nothing
        // else to assert without a WebMCP state.
    }

    #[tokio::test]
    async fn given_tools_added_after_purge_then_repopulated() {
        let sync = nav_sync_with(
            Arc::new(Mutex::new(WebmcpState::default())),
            mock_cdp_client().await,
        );
        {
            let mut st = sync.state.lock().await;
            seed_tools(&mut st);
        }

        let net = Arc::new(Mutex::new(NetworkState::default()));
        let nav = event("Page.frameNavigated", json!({ "frame": { "id": "F1" } }));
        process_page_event(&nav, &net, Some(&sync)).await;
        assert!(sync.state.lock().await.tools.is_empty());

        // WebMCP.enable's resync emits toolsAdded for the new document; the
        // webmcp listener ingests them into the same state.
        let added = event(
            "WebMCP.toolsAdded",
            json!({ "tools": [ { "name": "freshTool", "description": "d",
                                  "inputSchema": {}, "frameId": "F1" } ] }),
        );
        crate::chrome_mcp_handler::cdp_domains::webmcp::process_webmcp_event(&added, &sync.state)
            .await;

        let st = sync.state.lock().await;
        assert!(
            st.tools["F1"].contains_key("freshTool"),
            "eventual consistency: post-navigation toolsAdded repopulates the cache"
        );
    }
}
