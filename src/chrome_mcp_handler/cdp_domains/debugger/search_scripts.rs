use crate::chrome_mcp_handler::{ChromeMcpHandler, extract_from_value, find_line_column};
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "search_scripts",
    description = "Searches all cached script sources for a text pattern and returns matching locations (line and column numbers). Side effects: none (read-only query). Prerequisites: scripts must have been parsed and cached by the debugger. Returns: JSON array of matches with script ID, line/column numbers, and line preview. Use this to locate code before setting breakpoints. Alternatives: 'set_breakpoint' for direct breakpoint placement, 'evaluate_js' for runtime code discovery."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct SearchScriptsTool {
    /// Chrome instance id from open_instance/list_instances. Omit for the default instance.
    pub instance_id: Option<String>,
    /// The Tab ID of the target tab. Omit to use the active tab.
    pub tab_id: Option<String>,
    /// Text pattern or special command to search for. Constraints: non-empty string (empty string returns cached script count). Interactions: '@source' returns first 1000 chars of each script; 'debug' returns script lengths and errors. Defaults to: None (required).
    pub query: String,
}

impl SearchScriptsTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: SearchScriptsTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let session = handler.session(args.instance_id.clone()).await?;
        let debugger_state = session.debugger_state(args.tab_id.clone())?;

        // Check empty query BEFORE connecting — this is a pure state query
        if args.query.is_empty() {
            let scripts = debugger_state.lock().await.scripts.clone();
            return Ok(CallToolResult::text_content(vec![
                format!("Total cached scripts: {}", scripts.len()).into(),
            ]));
        }

        let target = session.target(args.tab_id.clone()).await?;

        let scripts = debugger_state.lock().await.scripts.clone();

        let mut results = vec![];
        let mut errors = vec![];
        for (script_id, script_hash) in scripts {
            match target
                .send_raw_command("Debugger.getScriptSource", json!({"scriptId": script_id}))
                .await
            {
                Ok(script_result) => {
                    if let Some(source) = extract_from_value(&script_result.result, "scriptSource")
                    {
                        if args.query == "@source" {
                            results.push(json!({"id": script_id, "source": source.chars().take(1000).collect::<String>()}));
                        } else if args.query == "debug" {
                            results.push(json!({"id": script_id, "source_len": source.len()}));
                        } else if let Some((line_number, column_number)) =
                            find_line_column(source, &args.query)
                        {
                            results.push(json!({
                                    "scriptId": script_id,
                                    "scriptHash": script_hash.hash,
                                    "lineNumber": line_number,
                                    "columnNumber": column_number,
                                    "linePreview": source.lines().nth(line_number as usize).unwrap_or("").trim()
                                }));
                        }
                    } else {
                        errors.push(format!(
                            "No scriptSource for {}: {:?}",
                            script_id, script_result.result
                        ));
                    }
                }
                Err(e) => {
                    let error_message = format!("{:?}", e);
                    if !error_message.contains("No script for id") {
                        errors.push(format!("Err for {}: {}", script_id, error_message));
                    }
                }
            }
        }

        Ok(CallToolResult::text_content(vec![
            if args.query == "debug" {
                format!("Results: {:?}\nErrors: {:?}", results, errors).into()
            } else if results.is_empty() {
                if !errors.is_empty() {
                    format!("No matches found. Errors encountered: {:?}", errors).into()
                } else {
                    "No matches found.".into()
                }
            } else {
                serde_json::to_string_pretty(&results)
                    .unwrap_or_default()
                    .into()
            },
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_mcp_handler::ScriptInfo;
    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;
    use crate::chrome_mcp_handler::chrome_instance::MockChromeManager;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_search_scripts_empty_query_returns_cached_count() {
        let handler = ChromeMcpHandler::new_test();

        // Prepopulate 3 scripts in state
        {
            let mut st = handler.default_session.debugger_state.lock().await;
            for i in 0..3 {
                st.scripts.insert(
                    format!("script-{}", i),
                    ScriptInfo {
                        hash: format!("hash-{}", i),
                        start_line: 0,
                        start_column: 0,
                    },
                );
            }
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": {
                "query": ""
            }
        }))
        .unwrap();

        let result = SearchScriptsTool::handle(params, &handler).await;
        assert!(result.is_ok());
        let res = result.unwrap();
        let res_json = serde_json::to_value(&res).unwrap();
        let text = res_json["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Total cached scripts: 3"));
    }

    #[tokio::test]
    async fn test_search_scripts_empty_query_with_no_scripts() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": {
                "query": ""
            }
        }))
        .unwrap();

        let result = SearchScriptsTool::handle(params, &handler).await;
        assert!(result.is_ok());
        let res = result.unwrap();
        let res_json = serde_json::to_value(&res).unwrap();
        let text = res_json["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Total cached scripts: 0"));
    }

    #[tokio::test]
    async fn test_search_scripts_missing_query_fails_deserialization() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": {}
        }))
        .unwrap();

        let result = SearchScriptsTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing field `query`")
        );
    }

    #[tokio::test]
    async fn test_search_scripts_handle() {
        let port = spawn_mock_chrome_server().await;

        let mut handler = ChromeMcpHandler::new_test();
        Arc::get_mut(&mut handler.default_session)
            .unwrap()
            .chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        {
            let mut st = handler.default_session.debugger_state.lock().await;
            st.scripts.insert(
                "mock-script-id".to_string(),
                ScriptInfo {
                    hash: "mock-hash".to_string(),
                    start_line: 0,
                    start_column: 0,
                },
            );
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": {
                "query": "something"
            }
        }))
        .unwrap();

        let result = SearchScriptsTool::handle(params, &handler).await;
        assert!(result.is_ok(), "Handle should succeed: {:?}", result.err());

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
    }

    /// Registers a real tab over the mock DevTools server and returns its id,
    /// so tests can exercise `session.debugger_state(tab_id)` resolution.
    async fn register_mock_tab(handler: &crate::chrome_mcp_handler::ChromeMcpHandler) -> String {
        use cdp_browser_lite::BrowserClient;
        use std::time::Duration;

        let session = handler.session(None).await.expect("default session");
        let port = crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server().await;
        let browser =
            BrowserClient::connect(&format!("127.0.0.1:{}", port), Duration::from_secs(5))
                .await
                .expect("BrowserClient connect to mock");
        let tab = browser.attach("T-page-1").await.expect("attach to mock");
        session
            .tabs
            .write()
            .unwrap()
            .register_tab(tab, None, "https://example.test".into())
            .expect("register tab")
    }

    #[tokio::test]
    async fn test_search_scripts_reads_tab_state_not_session_state() {
        let handler = ChromeMcpHandler::new_test();
        let session = handler.session(None).await.unwrap();

        // Session-level decoy scripts that must NOT be picked up.
        {
            let st = session.debugger_state(None).unwrap();
            let mut st = st.lock().await;
            st.scripts.insert(
                "SESSION-script".to_string(),
                ScriptInfo {
                    hash: "session-hash".to_string(),
                    start_line: 0,
                    start_column: 0,
                },
            );
        }

        // Tab-level script that MUST be picked up.
        let tab_id = register_mock_tab(&handler).await;
        let tab_state = session.debugger_state(Some(tab_id.clone())).unwrap();
        {
            let mut st = tab_state.lock().await;
            st.scripts.insert(
                "TAB-script".to_string(),
                ScriptInfo {
                    hash: "tab-hash".to_string(),
                    start_line: 0,
                    start_column: 0,
                },
            );
        }

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": {
                "tab_id": tab_id,
                "query": ""
            }
        }))
        .unwrap();

        let result = SearchScriptsTool::handle(params, &handler)
            .await
            .expect("empty query is a pure state read");
        let res_json = serde_json::to_value(&result).unwrap();
        let text = res_json["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("Total cached scripts: 1"),
            "must count only the TAB script, got: {text}"
        );
    }

    #[tokio::test]
    async fn test_search_scripts_unknown_tab_errors() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "search_scripts",
            "arguments": { "query": "", "tab_id": "tab-nope" }
        }))
        .unwrap();
        let err = SearchScriptsTool::handle(params, &handler)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected tab-not-found error, got: {err}"
        );
    }
}
