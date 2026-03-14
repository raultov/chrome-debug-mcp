pub mod get_console_logs;

use cdp_lite::client::CdpClient;
use cdp_lite::protocol::WsResponse;
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
    pub messages: Vec<ConsoleMessage>,
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
            st.messages.push(ConsoleMessage {
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
        st.messages.push(ConsoleMessage {
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
        st.messages.push(ConsoleMessage {
            source: "exception".to_string(),
            level: "error".to_string(),
            text: full_text,
            timestamp,
            url,
            line_number,
        });
    }
}

pub(crate) fn start_log_listener(client: &mut CdpClient, state_clone: Arc<Mutex<LogState>>) {
    let mut log_events = client.on_domain("Log");
    let state_clone_log = state_clone.clone();
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = log_events.next().await {
            process_log_event(&event, &state_clone_log).await;
        }
    });

    let mut runtime_events = client.on_domain("Runtime");
    let state_clone_runtime = state_clone.clone();
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = runtime_events.next().await {
            process_log_event(&event, &state_clone_runtime).await;
        }
    });
}
