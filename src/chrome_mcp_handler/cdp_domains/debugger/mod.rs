pub mod evaluate_on_call_frame;
pub mod pause_on_load;
pub mod remove_breakpoint;
pub mod resume;
pub mod search_scripts;
pub mod set_breakpoint;
pub mod step_over;

use crate::chrome_mcp_handler::{DebuggerState, ScriptInfo, extract_from_value};
use cdp_lite::client::CdpClient;
use cdp_lite::protocol::WsResponse;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Processes a single CDP debugger event, updating the shared state accordingly.
/// This function is extracted from the listener loop to enable isolated unit testing.
pub(crate) async fn process_debugger_event(event: &WsResponse, state: &Arc<Mutex<DebuggerState>>) {
    if let Some(method) = event.method.as_deref() {
        if method == "Debugger.scriptParsed" {
            if let (Some(script_id), Some(hash)) = (
                extract_from_value(&event.params, "scriptId"),
                extract_from_value(&event.params, "hash"),
            ) {
                let start_line = extract_from_value(&event.params, "startLine")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);
                let start_column = extract_from_value(&event.params, "startColumn")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);

                let mut st = state.lock().await;
                st.scripts.insert(
                    script_id.to_string(),
                    ScriptInfo {
                        hash: hash.to_string(),
                        start_line,
                        start_column,
                    },
                );
            }
        } else if method == "Debugger.paused"
            && let Some(call_first_frame_id) = event
                .params
                .as_ref()
                .and_then(|p: &serde_json::Value| p.get("callFrames"))
                .and_then(|frames: &serde_json::Value| frames.as_array())
                .and_then(|frames: &Vec<serde_json::Value>| frames.first())
                .and_then(|first_frame: &serde_json::Value| first_frame.get("callFrameId"))
                .and_then(|id: &serde_json::Value| id.as_str())
        {
            state.lock().await.paused_call_frame_id = Some(call_first_frame_id.to_string());
        }
    }
}

pub(crate) fn start_debugger_listener(
    client: &mut CdpClient,
    state_clone: Arc<Mutex<DebuggerState>>,
) {
    let mut debug_events = client.on_domain("Debugger");
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = debug_events.next().await {
            process_debugger_event(&event, &state_clone).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    fn make_event(method: &str, params: serde_json::Value) -> WsResponse {
        WsResponse {
            id: None,
            result: None,
            error: None,
            method: Some(method.to_string()),
            params: Some(params),
        }
    }

    #[tokio::test]
    async fn test_script_parsed_inserts_script_info() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.scriptParsed",
            json!({
                "scriptId": "42",
                "hash": "abc123",
                "startLine": "10",
                "startColumn": "5"
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert_eq!(st.scripts.len(), 1);
        let script = st.scripts.get("42").unwrap();
        assert_eq!(script.hash, "abc123");
        assert_eq!(script.start_line, 10);
        assert_eq!(script.start_column, 5);
    }

    #[tokio::test]
    async fn test_script_parsed_defaults_line_column() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.scriptParsed",
            json!({
                "scriptId": "1",
                "hash": "def456"
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        let script = st.scripts.get("1").unwrap();
        assert_eq!(script.start_line, 0);
        assert_eq!(script.start_column, 0);
    }

    #[tokio::test]
    async fn test_script_parsed_missing_hash_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.scriptParsed",
            json!({
                "scriptId": "99"
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.scripts.is_empty());
    }

    #[tokio::test]
    async fn test_script_parsed_missing_script_id_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.scriptParsed",
            json!({
                "hash": "abc"
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.scripts.is_empty());
    }

    #[tokio::test]
    async fn test_paused_stores_call_frame_id() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.paused",
            json!({
                "callFrames": [
                    { "callFrameId": "frame-0", "functionName": "main" },
                    { "callFrameId": "frame-1", "functionName": "helper" }
                ]
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert_eq!(st.paused_call_frame_id.as_deref(), Some("frame-0"));
    }

    #[tokio::test]
    async fn test_paused_empty_call_frames_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event(
            "Debugger.paused",
            json!({
                "callFrames": []
            }),
        );

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.paused_call_frame_id.is_none());
    }

    #[tokio::test]
    async fn test_paused_missing_call_frames_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event("Debugger.paused", json!({}));

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.paused_call_frame_id.is_none());
    }

    #[tokio::test]
    async fn test_unknown_method_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = make_event("Debugger.resumed", json!({}));

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.scripts.is_empty());
        assert!(st.paused_call_frame_id.is_none());
    }

    #[tokio::test]
    async fn test_event_without_method_ignored() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));
        let event = WsResponse {
            id: None,
            result: None,
            error: None,
            method: None,
            params: Some(json!({"scriptId": "1", "hash": "abc"})),
        };

        process_debugger_event(&event, &state).await;

        let st = state.lock().await;
        assert!(st.scripts.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_script_parsed_events() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));

        for i in 0..5 {
            let event = make_event(
                "Debugger.scriptParsed",
                json!({
                    "scriptId": format!("script-{}", i),
                    "hash": format!("hash-{}", i),
                    "startLine": i.to_string(),
                    "startColumn": "0"
                }),
            );
            process_debugger_event(&event, &state).await;
        }

        let st = state.lock().await;
        assert_eq!(st.scripts.len(), 5);
        for i in 0..5 {
            let key = format!("script-{}", i);
            let script = st.scripts.get(&key).unwrap();
            assert_eq!(script.hash, format!("hash-{}", i));
            assert_eq!(script.start_line, i);
        }
    }

    #[tokio::test]
    async fn test_paused_overwrites_previous_frame_id() {
        let state = Arc::new(Mutex::new(DebuggerState::default()));

        let event1 = make_event(
            "Debugger.paused",
            json!({
                "callFrames": [{ "callFrameId": "old-frame" }]
            }),
        );
        process_debugger_event(&event1, &state).await;
        assert_eq!(
            state.lock().await.paused_call_frame_id.as_deref(),
            Some("old-frame")
        );

        let event2 = make_event(
            "Debugger.paused",
            json!({
                "callFrames": [{ "callFrameId": "new-frame" }]
            }),
        );
        process_debugger_event(&event2, &state).await;
        assert_eq!(
            state.lock().await.paused_call_frame_id.as_deref(),
            Some("new-frame")
        );
    }
}
