use crate::chrome_mcp_handler::{ChromeMcpHandler, extract_from_value, find_line_column};
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use serde_json::json;

#[macros::mcp_tool(
    name = "search_scripts",
    description = "Search all parsed scripts for a specific text/query and get the line and column number for setting breakpoints"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct SearchScriptsTool {
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

        // Check empty query BEFORE connecting — this is a pure state query
        if args.query.is_empty() {
            let scripts = {
                let state = handler.debugger_state.lock().await;
                state.scripts.clone()
            };
            return Ok(CallToolResult::text_content(vec![
                format!("Total cached scripts: {}", scripts.len()).into(),
            ]));
        }

        let mut client_lock = handler.get_or_connect().await?;
        let cdp_client = client_lock.as_mut().unwrap();

        let scripts = {
            let state = handler.debugger_state.lock().await;
            state.scripts.clone()
        };

        let mut results = vec![];
        let mut errors = vec![];
        for (script_id, script_hash) in scripts {
            match cdp_client
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
    use crate::chrome_mcp_handler::cdp_domains::debugger::tests::spawn_mock_chrome_server;
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
            let mut st = handler.debugger_state.lock().await;
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
        handler.chrome_manager = Arc::new(Mutex::new(MockChromeManager::new(port)));

        {
            let mut st = handler.debugger_state.lock().await;
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
}
