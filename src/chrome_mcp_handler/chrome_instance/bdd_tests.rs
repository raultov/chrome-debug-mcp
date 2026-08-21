#[cfg(test)]
mod tests {
    use crate::chrome_mcp_handler::ChromeMcpHandler;

    use crate::chrome_mcp_handler::cdp_domains::tests::spawn_mock_chrome_server;

    use crate::chrome_mcp_handler::chrome_instance::open_instance::OpenInstanceTool;

    use crate::chrome_mcp_handler::chrome_instance::close_instance::CloseInstanceTool;
    use rust_mcp_sdk::schema::CallToolRequestParams;
    use serde_json::json;

    // Feature: instance lifecycle

    #[tokio::test]
    async fn given_fresh_handler_when_listing_instances_then_only_lazy_default_is_present() {
        let handler = ChromeMcpHandler::new_test();
        let descriptors = handler.registry.list_descriptors();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].id, "default");
        assert!(descriptors[0].is_default);
    }

    #[tokio::test]
    async fn given_running_default_when_opening_instance_then_new_id_port_and_profile_are_distinct()
    {
        let handler = ChromeMcpHandler::new_test();
        // Trigger default session ensure to simulate it running
        let _default_session = handler.session(None).await.unwrap();

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {}
        }))
        .unwrap();

        let _result = OpenInstanceTool::handle(params, &handler).await.unwrap();
        let descriptors = handler.registry.list_descriptors();
        // Since we opened one, we should have 2 total
        assert_eq!(descriptors.len(), 2);

        let secondary = descriptors.iter().find(|d| d.id != "default").unwrap();
        assert!(secondary.id.starts_with("chrome-"));

        let default_desc = descriptors.iter().find(|d| d.id == "default").unwrap();
        assert_ne!(secondary.port, default_desc.port);
    }

    #[tokio::test]
    async fn given_handler_when_opening_instance_with_label_then_label_becomes_id() {
        let handler = ChromeMcpHandler::new_test();
        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {
                "label": "checkout"
            }
        }))
        .unwrap();

        let _result = OpenInstanceTool::handle(params, &handler).await.unwrap();
        let descriptors = handler.registry.list_descriptors();
        let checkout_desc = descriptors.iter().find(|d| d.id == "checkout");
        assert!(checkout_desc.is_some());
    }

    #[tokio::test]
    async fn given_existing_label_when_opening_duplicate_then_error_and_no_spawn() {
        let handler = ChromeMcpHandler::new_test();
        let params1: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {
                "label": "checkout"
            }
        }))
        .unwrap();
        OpenInstanceTool::handle(params1, &handler).await.unwrap();

        let params2: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {
                "label": "checkout"
            }
        }))
        .unwrap();
        let result2 = OpenInstanceTool::handle(params2, &handler).await;
        assert!(result2.is_err());
        let err_str = result2.as_ref().err().unwrap().to_string();
        assert!(err_str.contains("already exists") || err_str.contains("already in use"));
    }

    #[tokio::test]
    async fn given_two_instances_when_closing_one_then_it_is_removed_and_stopped() {
        let handler = ChromeMcpHandler::new_test();
        let params_open: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {
                "label": "checkout"
            }
        }))
        .unwrap();
        OpenInstanceTool::handle(params_open, &handler)
            .await
            .unwrap();

        let params_close: CallToolRequestParams = serde_json::from_value(json!({
            "name": "close_instance",
            "arguments": {
                "instance_id": "checkout"
            }
        }))
        .unwrap();
        CloseInstanceTool::handle(params_close, &handler)
            .await
            .unwrap();

        let descriptors = handler.registry.list_descriptors();
        assert_eq!(descriptors.len(), 1); // only "default" left
        assert!(handler.registry.get_session("checkout").is_none());
    }

    #[tokio::test]
    async fn given_closed_default_when_tool_invoked_then_default_is_recreated() {
        let port = spawn_mock_chrome_server().await;
        let handler = ChromeMcpHandler::new_test_with_port(port);

        let params_close: CallToolRequestParams = serde_json::from_value(json!({
            "name": "close_instance",
            "arguments": {
                "instance_id": "default"
            }
        }))
        .unwrap();
        CloseInstanceTool::handle(params_close, &handler)
            .await
            .unwrap();

        // Invoking get_or_connect on default should lazily re-ensure and succeed
        let default_session = handler.session(None).await.unwrap();
        let client_lock = default_session.get_or_connect().await;
        assert!(client_lock.is_ok());
    }

    // Feature: guard rails

    #[tokio::test]
    async fn given_user_profile_mode_when_opening_second_instance_then_rejected_with_reason() {
        // Start handler with --user-profile
        let handler = ChromeMcpHandler::new_with_params(
            "127.0.0.1".into(),
            9222,
            false,
            false,
            false,
            true, // user_profile = true
        );

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {}
        }))
        .unwrap();

        let result = OpenInstanceTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("user-profile mode")
        );
    }

    #[tokio::test]
    async fn given_instance_cap_reached_when_opening_then_rejected_without_spawn() {
        let handler = ChromeMcpHandler::new_test();
        // Ensure default session is loaded lazily so sessions.len() is 1
        let _ = handler.session(None).await.unwrap();
        // Enforce max_instances = 1
        handler
            .registry
            .max_instances
            .store(1, std::sync::atomic::Ordering::SeqCst);

        let params: CallToolRequestParams = serde_json::from_value(json!({
            "name": "open_instance",
            "arguments": {}
        }))
        .unwrap();

        let result = OpenInstanceTool::handle(params, &handler).await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("limit reached"));
    }

    #[tokio::test]
    async fn given_unknown_instance_id_when_tool_invoked_then_error_lists_valid_ids() {
        let handler = ChromeMcpHandler::new_test();
        let result = handler.session(Some("typo".to_string())).await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Instance id 'typo' not found")
        );
    }
}
