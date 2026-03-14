use crate::chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;

#[macros::mcp_tool(
    name = "profile_page_performance",
    description = "Record and analyze a performance trace of the page. It automatically calculates Core Web Vitals (FCP, LCP, DCL, Load) and identifies the top Long Tasks (main thread blocking operations). You can optionally reload the page with cache disabled to simulate a cold start."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct ProfilePagePerformanceTool {
    /// Duration to record the trace in milliseconds. Defaults to 3000ms. Keep it between 1000 and 10000.
    #[serde(default)]
    pub duration_ms: Option<u64>,

    /// Action to perform right after starting the trace. Can be "none" (default) or "reload".
    #[serde(default)]
    pub action: Option<String>,

    /// If true, disables the network cache before profiling and restores it after. Useful with action="reload" to simulate a cold start.
    #[serde(default)]
    pub disable_cache: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct LongTask {
    name: String,
    duration_ms: f64,
    start_time_ms: f64,
    args: Option<serde_json::Value>,
}

#[derive(Debug, Default, serde::Serialize)]
struct PerformanceSummary {
    profile_duration_ms: u64,
    first_contentful_paint_ms: Option<f64>,
    largest_contentful_paint_ms: Option<f64>,
    dom_content_loaded_ms: Option<f64>,
    load_event_ms: Option<f64>,
    total_blocking_time_ms: f64,
    top_long_tasks: Vec<LongTask>,
}

impl ProfilePagePerformanceTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: ProfilePagePerformanceTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        let duration = args.duration_ms.unwrap_or(3000).clamp(500, 15000);
        let action = args.action.unwrap_or_else(|| "none".to_string());
        let disable_cache = args.disable_cache.unwrap_or(false);

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().ok_or_else(|| {
            CallToolError::from_message("Chrome connection is not established".to_string())
        })?;

        // 1. Setup cache
        if disable_cache {
            let _ = cdp_client
                .send_raw_command("Network.setCacheDisabled", json!({ "cacheDisabled": true }))
                .await;
        }

        // 2. Prepare to listen for completion
        let (tx, mut rx) = mpsc::channel(1);
        {
            let mut st = handler.tracing_state.lock().await;
            st.completion_channel = Some(tx);
        }

        // 3. Start tracing
        let start_res = cdp_client
            .send_raw_command(
                "Tracing.start",
                json!({
                    "transferMode": "ReturnAsStream",
                    "categories": "devtools.timeline,v8.execute,blink.user_timing,blink,cc,gpu,toplevel"
                }),
            )
            .await;

        if let Err(e) = start_res {
            return Err(CallToolError::from_message(format!(
                "Failed to start tracing: {:?}",
                e
            )));
        }

        // 4. Perform action
        if action == "reload" {
            let _ = cdp_client
                .send_raw_command("Page.reload", json!({ "ignoreCache": disable_cache }))
                .await;
        }

        // 5. Wait
        tokio::time::sleep(Duration::from_millis(duration)).await;

        // 6. Stop tracing
        let _ = cdp_client.send_raw_command("Tracing.end", json!({})).await;

        // 7. Wait for stream handle
        let stream_handle = match tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(handle)) => handle,
            _ => {
                return Err(CallToolError::from_message(
                    "Timeout waiting for Tracing.tracingComplete".to_string(),
                ));
            }
        };

        // 8. Read the stream via IO domain
        let mut raw_json_data = String::new();
        loop {
            let read_res = cdp_client
                .send_raw_command(
                    "IO.read",
                    json!({
                        "handle": stream_handle
                    }),
                )
                .await;

            if let Ok(resp) = read_res {
                if let Some(res_obj) = resp.result {
                    if let Some(data) = res_obj.get("data").and_then(|v| v.as_str()) {
                        raw_json_data.push_str(data);
                    }
                    if let Some(eof) = res_obj.get("eof").and_then(|v| v.as_bool())
                        && eof
                    {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // 9. Close IO stream
        let _ = cdp_client
            .send_raw_command("IO.close", json!({ "handle": stream_handle }))
            .await;

        // 10. Restore cache if disabled
        if disable_cache {
            let _ = cdp_client
                .send_raw_command(
                    "Network.setCacheDisabled",
                    json!({ "cacheDisabled": false }),
                )
                .await;
        }

        // Drop lock before intensive parsing
        drop(client_lock);

        // 11. Parse and Analyze
        let summary = Self::analyze_trace(&raw_json_data, duration);

        let result_json = serde_json::to_value(&summary).unwrap_or_default();
        Ok(CallToolResult::text_content(vec![
            serde_json::to_string_pretty(&result_json)
                .unwrap_or_default()
                .into(),
        ]))
    }

    fn analyze_trace(raw_json: &str, duration_ms: u64) -> PerformanceSummary {
        let mut summary = PerformanceSummary {
            profile_duration_ms: duration_ms,
            ..Default::default()
        };

        let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);
        if let Ok(root) = parsed {
            // Chrome trace is usually a top-level array OR an object with "traceEvents" array
            let events_opt = if root.is_array() {
                root.as_array()
            } else {
                root.get("traceEvents").and_then(|v| v.as_array())
            };

            if let Some(events) = events_opt {
                let mut long_tasks = Vec::new();
                let mut navigation_start_ts = None;

                // First pass: find navigationStart to normalize timestamps (ts is in microseconds)
                for ev in events {
                    if let (Some(name), Some(ts)) = (
                        ev.get("name").and_then(|v| v.as_str()),
                        ev.get("ts").and_then(|v| v.as_f64()),
                    ) && name == "navigationStart"
                        && navigation_start_ts.is_none()
                    {
                        navigation_start_ts = Some(ts);
                    }
                }

                let base_ts = navigation_start_ts.unwrap_or(0.0);

                for ev in events {
                    let name = ev.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let ph = ev.get("ph").and_then(|v| v.as_str()).unwrap_or("");
                    let ts = ev.get("ts").and_then(|v| v.as_f64()).unwrap_or(0.0);

                    // Web Vitals (Marks)
                    if ph == "R" || ph == "I" || ph == "O" {
                        let ms = (ts - base_ts) / 1000.0;
                        if ms > 0.0 {
                            if name == "firstContentfulPaint" {
                                summary.first_contentful_paint_ms = Some(ms);
                            } else if name == "largestContentfulPaint::Candidate" {
                                // LCP can have multiple candidates, keep the latest
                                summary.largest_contentful_paint_ms = Some(ms);
                            } else if name == "domContentLoadedEventEnd" {
                                summary.dom_content_loaded_ms = Some(ms);
                            } else if name == "loadEventEnd" {
                                summary.load_event_ms = Some(ms);
                            }
                        }
                    }

                    // Long Tasks (Complete events with duration)
                    if ph == "X" {
                        let dur = ev.get("dur").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let dur_ms = dur / 1000.0;

                        // Consider task long if > 50ms and it's a RunTask or EvaluateScript
                        if dur_ms > 50.0
                            && (name == "RunTask"
                                || name == "EvaluateScript"
                                || name == "FunctionCall")
                        {
                            let start_ms = (ts - base_ts) / 1000.0;
                            if start_ms > 0.0 {
                                summary.total_blocking_time_ms += dur_ms - 50.0;

                                long_tasks.push(LongTask {
                                    name: name.to_string(),
                                    duration_ms: dur_ms,
                                    start_time_ms: start_ms,
                                    args: ev.get("args").cloned(),
                                });
                            }
                        }
                    }
                }

                // Sort long tasks by duration descending and keep top 10
                long_tasks.sort_by(|a, b| {
                    b.duration_ms
                        .partial_cmp(&a.duration_ms)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                long_tasks.truncate(10);
                summary.top_long_tasks = long_tasks;
            }
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_profile_page_performance_tool_deserialization() {
        let json = json!({
            "duration_ms": 5000,
            "action": "reload",
            "disable_cache": true
        });
        let tool: ProfilePagePerformanceTool = serde_json::from_value(json).unwrap();
        assert_eq!(tool.duration_ms, Some(5000));
        assert_eq!(tool.action, Some("reload".to_string()));
        assert_eq!(tool.disable_cache, Some(true));
    }

    #[test]
    fn test_analyze_trace() {
        let trace_data = json!({
            "traceEvents": [
                { "name": "navigationStart", "ph": "R", "ts": 1000000 },
                { "name": "firstContentfulPaint", "ph": "R", "ts": 1200000 },
                { "name": "largestContentfulPaint::Candidate", "ph": "R", "ts": 1500000 },
                { "name": "domContentLoadedEventEnd", "ph": "R", "ts": 1800000 },
                { "name": "loadEventEnd", "ph": "R", "ts": 2000000 },
                // Long task (RunTask) - 100ms duration (dur is in micros)
                { "name": "RunTask", "ph": "X", "ts": 1300000, "dur": 100000, "args": { "src": "test.js" } },
                // Short task (ignored)
                { "name": "RunTask", "ph": "X", "ts": 1450000, "dur": 10000 },
                // Another long task (EvaluateScript) - 200ms
                { "name": "EvaluateScript", "ph": "X", "ts": 1600000, "dur": 200000 }
            ]
        });

        let summary = ProfilePagePerformanceTool::analyze_trace(&trace_data.to_string(), 3000);

        assert_eq!(summary.first_contentful_paint_ms, Some(200.0)); // (1.2s - 1.0s) = 200ms
        assert_eq!(summary.largest_contentful_paint_ms, Some(500.0));
        assert_eq!(summary.dom_content_loaded_ms, Some(800.0));
        assert_eq!(summary.load_event_ms, Some(1000.0));

        // TBT: (100-50) + (200-50) = 50 + 150 = 200ms
        assert_eq!(summary.total_blocking_time_ms, 200.0);
        assert_eq!(summary.top_long_tasks.len(), 2);
        assert_eq!(summary.top_long_tasks[0].duration_ms, 200.0);
        assert_eq!(summary.top_long_tasks[1].duration_ms, 100.0);
    }
}
