pub mod evaluate_on_call_frame;
pub mod pause_on_load;
pub mod remove_breakpoint;
pub mod resume;
pub mod search_scripts;
pub mod set_breakpoint;
pub mod step_over;

use crate::chrome_mcp_handler::{DebuggerState, ScriptInfo, extract_from_value};
use cdp_lite::client::CdpClient;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) fn start_debugger_listener(
    client: &mut CdpClient,
    state_clone: Arc<Mutex<DebuggerState>>,
) {
    let mut debug_events = client.on_domain("Debugger");
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = debug_events.next().await {
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

                        let mut st = state_clone.lock().await;
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
                    state_clone.lock().await.paused_call_frame_id =
                        Some(call_first_frame_id.to_string());
                }
            }
        }
    });
}
