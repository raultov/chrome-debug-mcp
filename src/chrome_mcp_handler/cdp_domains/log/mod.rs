pub mod get_console_logs;

use crate::chrome_mcp_handler::MAX_CONSOLE_MESSAGES;
use cdp_browser_lite::WsResponse;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
pub struct ConsoleMessage {
    pub source: String,
    pub level: String,
    pub text: String,
    pub timestamp: f64,
    pub url: Option<String>,
    pub line_number: Option<i64>,
}

#[derive(Default)]
pub(crate) struct LogState {
    pub messages: VecDeque<ConsoleMessage>,
}

impl LogState {
    pub(crate) fn push_message(&mut self, msg: ConsoleMessage) {
        self.messages.push_back(msg);
        if self.messages.len() > MAX_CONSOLE_MESSAGES {
            self.messages.pop_front();
        }
    }
}

pub(crate) async fn process_log_event(event: &WsResponse, state: &Arc<Mutex<LogState>>) {
    let method = match event.method.as_deref() {
        Some(m) => m,
        None => return,
    };

    let params = match &event.params {
        Some(p) => p,
        None => return,
    };

    if method == "Log.entryAdded" {
        if let Some(entry) = params.get("entry") {
            let source = entry
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let level = entry
                .get("level")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = entry
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let timestamp = entry
                .get("timestamp")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let url = entry
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let line_number = entry.get("lineNumber").and_then(|v| v.as_i64());

            let mut st = state.lock().await;
            st.push_message(ConsoleMessage {
                source,
                level,
                text,
                timestamp,
                url,
                line_number,
            });
        }
    } else if method == "Runtime.consoleAPICalled" {
        let type_ = params
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timestamp = params
            .get("timestamp")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Extract text from args
        let text = if let Some(args) = params.get("args").and_then(|v| v.as_array()) {
            let mut parts = Vec::new();
            for arg in args {
                if let Some(val) = arg.get("value") {
                    if let Some(s) = val.as_str() {
                        parts.push(s.to_string());
                    } else if val.is_number() || val.is_boolean() {
                        parts.push(val.to_string());
                    } else {
                        // For objects, try to get description
                        if let Some(desc) = arg.get("description").and_then(|v| v.as_str()) {
                            parts.push(desc.to_string());
                        } else {
                            parts.push(val.to_string());
                        }
                    }
                } else if let Some(desc) = arg.get("description").and_then(|v| v.as_str()) {
                    parts.push(desc.to_string());
                }
            }
            parts.join(" ")
        } else {
            String::new()
        };

        let mut st = state.lock().await;
        st.push_message(ConsoleMessage {
            source: "console-api".to_string(),
            level: type_,
            text,
            timestamp,
            url: None, // Could extract from stackTrace if needed
            line_number: None,
        });
    } else if method == "Runtime.exceptionThrown"
        && let Some(details) = params.get("exceptionDetails")
    {
        let text = details
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let url = details
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let line_number = details.get("lineNumber").and_then(|v| v.as_i64());
        let timestamp = params
            .get("timestamp")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let mut full_text = text.clone();
        if let Some(exception) = details.get("exception")
            && let Some(desc) = exception.get("description").and_then(|v| v.as_str())
        {
            full_text = desc.to_string();
        }

        let mut st = state.lock().await;
        st.push_message(ConsoleMessage {
            source: "exception".to_string(),
            level: "error".to_string(),
            text: full_text,
            timestamp,
            url,
            line_number,
        });
    }
}

pub(crate) fn start_log_listener(
    target: &crate::chrome_mcp_handler::cdp_domains::cdp_target::CdpTarget,
    state_clone: Arc<Mutex<LogState>>,
) -> crate::chrome_mcp_handler::cdp_domains::event_pump::ListenerHandles {
    let mut handles =
        crate::chrome_mcp_handler::cdp_domains::event_pump::ListenerHandles::default();

    let state_clone_log = state_clone.clone();
    handles.push(
        crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener(
            target,
            "Log",
            move |event| {
                let state = state_clone_log.clone();
                async move {
                    process_log_event(&event, &state).await;
                }
            },
        ),
    );

    let state_clone_runtime = state_clone.clone();
    handles.push(
        crate::chrome_mcp_handler::cdp_domains::event_pump::spawn_domain_listener(
            target,
            "Runtime",
            move |event| {
                let state = state_clone_runtime.clone();
                async move {
                    process_log_event(&event, &state).await;
                }
            },
        ),
    );

    handles
}
