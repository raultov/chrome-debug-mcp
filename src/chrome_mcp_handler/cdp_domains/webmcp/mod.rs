pub mod get_invocation;
pub mod invoke_tool;
pub mod list_invocations;
pub mod list_tools;

pub use get_invocation::GetWebmcpInvocationTool;
pub use invoke_tool::InvokeWebmcpToolTool;
pub use list_invocations::ListWebmcpInvocationsTool;
pub use list_tools::ListWebmcpToolsTool;

use cdp_browser_lite::{CdpClient, WsResponse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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

#[derive(Default, Debug, Clone)]
pub struct WebmcpState {
    // Maps frame_id -> (tool_name -> Tool)
    pub tools: HashMap<String, HashMap<String, WebmcpTool>>,
    // Maps invocation_id -> Invocation
    pub invocations: HashMap<String, WebmcpInvocation>,
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

pub(crate) fn start_webmcp_listener(client: &mut CdpClient, state_clone: Arc<Mutex<WebmcpState>>) {
    let mut webmcp_events = client.on_domain("WebMCP");
    tokio::spawn(async move {
        use tokio_stream::StreamExt;
        while let Some(Ok(event)) = webmcp_events.next().await {
            process_webmcp_event(&event, &state_clone).await;
        }
    });
}

#[cfg(test)]
mod tests {
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
}
