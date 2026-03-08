use crate::chrome_mcp_handler::{ChromeMcpHandler, ScriptInfo, extract_from_value};
use cdp_lite::client::CdpClient;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use std::time::Duration;

#[macros::mcp_tool(
    name = "connect_chrome",
    description = "Connect to Chrome CDP (e.g. 127.0.0.1:9222)"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
#[allow(dead_code)]
pub struct ConnectChromeTool {
    pub url: String,
}

impl ConnectChromeTool {
    #[allow(dead_code)]
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ConnectChromeTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let cdp_client = CdpClient::new(&args.url, Duration::from_secs(15))
            .await
            .map_err(|e| CallToolError::from_message(format!("Failed to connect to CDP: {}", e)))?;

        let _ = cdp_client
            .send_raw_command("Runtime.enable", cdp_lite::protocol::NoParams)
            .await;
        let _ = cdp_client
            .send_raw_command("Page.enable", cdp_lite::protocol::NoParams)
            .await;

        let mut debug_events = cdp_client.on_domain("Debugger");
        let state_clone = handler.debugger_state.clone();
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

                            eprintln!(
                                "Parsed script Id: {}, Hash: {}, StartLine: {}, StartColumn: {}",
                                script_id, hash, start_line, start_column
                            );
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
                    } else if method == "Debugger.paused" {
                        if let Some(call_frame_id) = event
                            .params
                            .as_ref()
                            .and_then(|p: &serde_json::Value| p.get("callFrames"))
                            .and_then(|frames: &serde_json::Value| frames.as_array())
                            .and_then(|frames: &Vec<serde_json::Value>| frames.first())
                            .and_then(|first_frame: &serde_json::Value| {
                                first_frame.get("callFrameId")
                            })
                            .and_then(|id: &serde_json::Value| id.as_str())
                        {
                            eprintln!(
                                "Captured paused event, setting call_frame_id to {}",
                                call_frame_id
                            );
                            state_clone.lock().await.paused_call_frame_id =
                                Some(call_frame_id.to_string());
                        } else {
                            eprintln!(
                                "Paused event missing callFrameId payload: {:?}",
                                event.params
                            );
                        }
                    }
                }
            }
        });

        let _ = cdp_client
            .send_raw_command("Debugger.enable", cdp_lite::protocol::NoParams)
            .await;

        *handler.client.lock().await = Some(cdp_client);

        Ok(CallToolResult::text_content(vec![
            format!("Successfully connected to Chrome at {}", args.url).into(),
        ]))
    }
}
