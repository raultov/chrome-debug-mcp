pub mod get_invocation;
pub mod invoke_tool;
pub mod list_invocations;
pub mod list_tools;

pub use get_invocation::GetWebmcpInvocationTool;
pub use invoke_tool::InvokeWebmcpToolTool;
pub use list_invocations::ListWebmcpInvocationsTool;
pub use list_tools::ListWebmcpToolsTool;

use cdp_browser_lite::WsResponse;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Everything the `Page` listener needs to keep the per-tab WebMCP tool cache
/// aligned across navigations: the owning tab's `WebmcpState` and a live
/// handle to that same tab's CDP target for the enable-based resync.
///
/// `None` wiring means "no WebMCP for this session or tab": the page listener
/// then skips every purge/resync step for its events.
#[derive(Clone)]
pub(crate) struct WebmcpNavSync {
    pub(crate) state: Arc<Mutex<WebmcpState>>,
    pub(crate) target: crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct WebmcpAnnotation {
    #[serde(rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(rename = "untrustedContent", skip_serializing_if = "Option::is_none")]
    pub untrusted_content: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autosubmit: Option<bool>,
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct WebmcpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<WebmcpAnnotation>,
    #[serde(rename = "frameId")]
    pub frame_id: String,
    #[serde(rename = "backendNodeId", skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i64>,
}

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct WebmcpInvocation {
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "frameId")]
    pub frame_id: String,
    #[serde(rename = "invocationId")]
    pub invocation_id: String,
    pub input: String,
    pub status: Option<String>, // "Completed", "Canceled", "Error"
    pub output: Option<serde_json::Value>,
    #[serde(rename = "errorText", skip_serializing_if = "Option::is_none")]
    pub error_text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize, Default)]
pub enum WebmcpAvailability {
    #[default]
    NotRequested,
    Pending,
    Unsupported,
    Enabled,
}

impl WebmcpAvailability {
    /// Returns initial availability based on whether WebMCP is enabled on the server/session.
    pub fn initial(enable_webmcp: bool) -> Self {
        if enable_webmcp {
            Self::Pending
        } else {
            Self::NotRequested
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct WebmcpState {
    // Maps frame_id -> (tool_name -> Tool)
    pub tools: HashMap<String, HashMap<String, WebmcpTool>>,
    // Maps invocation_id -> Invocation
    pub invocations: HashMap<String, WebmcpInvocation>,
    // Tracks current availability of the WebMCP feature
    pub availability: WebmcpAvailability,
}

impl WebmcpState {
    /// Drops every cached tool registered by `frame_id`.
    ///
    /// Other frames' tools, invocations and availability are untouched.
    /// Invocations must survive document swaps: a page tool whose `execute`
    /// reloads the page still emits `toolResponded` afterwards, and
    /// `webmcp_get_invocation` must keep working.
    pub(crate) fn clear_frame_tools(&mut self, frame_id: &str) {
        self.tools.remove(frame_id);
    }

    /// Drops every cached tool across all frames.
    ///
    /// A cross-document navigation of the main frame destroys the whole frame
    /// tree; child frames get fresh frame IDs, so a single-frame purge would
    /// leave stale iframe tools behind. Invocations survive for the same
    /// reason as in [`Self::clear_frame_tools`].
    pub(crate) fn clear_all_tools(&mut self) {
        self.tools.clear();
    }
}

pub(crate) async fn process_webmcp_event(event: &WsResponse, state: &Arc<Mutex<WebmcpState>>) {
    let method = match event.method.as_deref() {
        Some(m) => m,
        None => return,
    };

    let params = match &event.params {
        Some(p) => p,
        None => return,
    };

    match method {
        "WebMCP.toolsAdded" => {
            if let Some(tools_arr) = params.get("tools").and_then(|v| v.as_array()) {
                let mut st = state.lock().await;
                for t_val in tools_arr {
                    if let Ok(tool) = serde_json::from_value::<WebmcpTool>(t_val.clone()) {
                        st.tools
                            .entry(tool.frame_id.clone())
                            .or_default()
                            .insert(tool.name.clone(), tool);
                    }
                }
            }
        }
        "WebMCP.toolsRemoved" => {
            if let Some(tools_arr) = params.get("tools").and_then(|v| v.as_array()) {
                let mut st = state.lock().await;
                for t_val in tools_arr {
                    if let Some(name) = t_val.get("name").and_then(|v| v.as_str())
                        && let Some(frame_id) = t_val.get("frameId").and_then(|v| v.as_str())
                        && let Some(frame_tools) = st.tools.get_mut(frame_id)
                    {
                        frame_tools.remove(name);
                    }
                }
            }
        }
        "WebMCP.toolInvoked" => {
            if let Ok(invocation) = serde_json::from_value::<WebmcpInvocation>(params.clone()) {
                let mut st = state.lock().await;
                st.invocations
                    .insert(invocation.invocation_id.clone(), invocation);
            }
        }
        "WebMCP.toolResponded" => {
            if let Some(invocation_id) = params.get("invocationId").and_then(|v| v.as_str()) {
                let mut st = state.lock().await;
                if let Some(inv) = st.invocations.get_mut(invocation_id) {
                    inv.status = params
                        .get("status")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    inv.output = params.get("output").cloned();
                    inv.error_text = params
                        .get("errorText")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn start_webmcp_listener(
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state_clone: Arc<Mutex<WebmcpState>>,
) -> tokio::task::JoinHandle<()> {
    let nav_sync = WebmcpNavSync {
        state: state_clone.clone(),
        target: target.clone(),
    };
    crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener_with_recovery(
        target,
        "WebMCP",
        {
            let state = state_clone.clone();
            move |event| {
                let state = state.clone();
                async move {
                    process_webmcp_event(&event, &state).await;
                }
            }
        },
        // A lag on this stream is what the tool cache cannot survive on its
        // own: `toolsAdded` and `toolsRemoved` may both have been dropped.
        move |_skipped| {
            let nav_sync = nav_sync.clone();
            async move {
                nav_sync.resync_after_lag().await;
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_event(method: &str, params: serde_json::Value) -> WsResponse {
        WsResponse {
            method: Some(method.to_string()),
            params: Some(params),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_tools_added_and_removed() {
        let state = Arc::new(Mutex::new(WebmcpState::default()));

        let add_event = make_event(
            "WebMCP.toolsAdded",
            json!({
                "tools": [
                    {
                        "name": "testTool",
                        "description": "A test tool",
                        "inputSchema": { "type": "object" },
                        "frameId": "frame-1"
                    }
                ]
            }),
        );
        process_webmcp_event(&add_event, &state).await;

        {
            let st = state.lock().await;
            assert_eq!(
                st.tools
                    .get("frame-1")
                    .unwrap()
                    .get("testTool")
                    .unwrap()
                    .name,
                "testTool"
            );
        }

        let remove_event = make_event(
            "WebMCP.toolsRemoved",
            json!({
                "tools": [
                    {
                        "name": "testTool",
                        "frameId": "frame-1"
                    }
                ]
            }),
        );
        process_webmcp_event(&remove_event, &state).await;

        {
            let st = state.lock().await;
            assert!(st.tools.get("frame-1").unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn test_tool_invoked_and_responded() {
        let state = Arc::new(Mutex::new(WebmcpState::default()));

        let invoke_event = make_event(
            "WebMCP.toolInvoked",
            json!({
                "toolName": "testTool",
                "frameId": "frame-1",
                "invocationId": "inv-1",
                "input": "{\"foo\":\"bar\"}"
            }),
        );
        process_webmcp_event(&invoke_event, &state).await;

        {
            let st = state.lock().await;
            assert_eq!(st.invocations.get("inv-1").unwrap().tool_name, "testTool");
            assert!(st.invocations.get("inv-1").unwrap().status.is_none());
        }

        let respond_event = make_event(
            "WebMCP.toolResponded",
            json!({
                "invocationId": "inv-1",
                "status": "Completed",
                "output": { "result": 42 }
            }),
        );
        process_webmcp_event(&respond_event, &state).await;

        {
            let st = state.lock().await;
            let inv = st.invocations.get("inv-1").unwrap();
            assert_eq!(inv.status.as_deref(), Some("Completed"));
            assert_eq!(
                inv.output.as_ref().unwrap().get("result").unwrap().as_i64(),
                Some(42)
            );
        }
    }

    fn seed_tools(st: &mut WebmcpState) {
        for (name, frame) in [
            ("toolA", "frame-main"),
            ("toolI1", "frame-iframe"),
            ("toolI2", "frame-iframe"),
        ] {
            st.tools.entry(frame.to_string()).or_default().insert(
                name.to_string(),
                WebmcpTool {
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

    #[tokio::test]
    async fn given_clear_frame_tools_then_only_that_frame_is_dropped() {
        let mut st = WebmcpState::default();
        seed_tools(&mut st);

        st.clear_frame_tools("frame-iframe");

        assert!(!st.tools.contains_key("frame-iframe"), "iframe purged");
        assert!(
            st.tools.contains_key("frame-main"),
            "main frame tools must survive a child-frame purge"
        );
    }

    #[tokio::test]
    async fn given_clear_all_tools_then_tools_go_and_invocations_stay() {
        let mut st = WebmcpState::default();
        seed_tools(&mut st);
        st.availability = WebmcpAvailability::Enabled;
        st.invocations.insert(
            "inv-1".to_string(),
            WebmcpInvocation {
                tool_name: "toolA".to_string(),
                frame_id: "frame-main".to_string(),
                invocation_id: "inv-1".to_string(),
                input: "{}".to_string(),
                status: Some("Completed".to_string()),
                output: None,
                error_text: None,
            },
        );

        st.clear_all_tools();

        assert!(st.tools.is_empty(), "all tools purged");
        assert!(
            !st.invocations.is_empty(),
            "invocations must survive navigation"
        );
        assert_eq!(
            st.availability,
            WebmcpAvailability::Enabled,
            "availability is not touched by a purge"
        );
    }
}
