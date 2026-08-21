use crate::chrome_mcp_handler::CustomState;
use crate::chrome_mcp_handler::DebuggerState;
use crate::chrome_mcp_handler::NetworkState;
use crate::chrome_mcp_handler::cdp_domains;
use crate::chrome_mcp_handler::chrome_instance::ChromeManager;
use crate::chrome_mcp_handler::chrome_instance::cdp_browser_manager::{
    CdpBrowserManager, RealLauncher,
};
use crate::chrome_mcp_handler::chrome_instance::launch::{ChromeFeature, LaunchParams};
use crate::chrome_mcp_handler::chrome_instance::registry::InstanceDescriptor;
use crate::chrome_mcp_handler::{BrowserSession, ChromeMcpHandler};
use rust_mcp_sdk::{
    macros,
    schema::{CallToolError, CallToolRequestParams, CallToolResult},
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[macros::mcp_tool(
    name = "open_instance",
    description = "Opens a new independent Chrome instance. Returns its instance_id, host, port, and profile directory."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct OpenInstanceTool {
    /// Optional label to identify the instance. If omitted, a dynamic label is generated.
    pub label: Option<String>,
    /// Optional headless mode. Encouraged to be set to false so that the user can see what happens with the browser. Defaults to false.
    pub headless: Option<bool>,
    /// Optional proxy server configuration.
    pub proxy: Option<String>,
    /// Optional feature presets (e.g. WEB_MCP, WEBGL_SOFTWARE).
    pub features: Option<Vec<ChromeFeature>>,
}

impl OpenInstanceTool {
    pub async fn handle(
        params: CallToolRequestParams,
        handler: &ChromeMcpHandler,
    ) -> Result<CallToolResult, CallToolError> {
        let args_value = serde_json::Value::Object(params.arguments.unwrap_or_default());
        let args: OpenInstanceTool = serde_json::from_value(args_value)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;

        // Guard: if handler base_params has user_profile = true, we reject second instance
        // because of Chrome's singleton profile lock.
        {
            let desc = handler.registry.list_descriptors();
            if !desc.is_empty() && handler.base_params.user_profile {
                // Actually if user_profile mode is enabled, it uses UserDefault which has the lock.
                return Err(CallToolError::from_message(
                    "Cannot open multiple instances in --user-profile mode due to Chrome's profile lock".to_string()
                ));
            }
        }

        let instance_id = if let Some(lbl) = &args.label {
            lbl.clone()
        } else {
            handler.registry.generate_id()
        };

        // Construct dynamic parameters for this secondary instance
        let mut child_params = LaunchParams::new(
            "127.0.0.1".into(),
            0,     // ephemeral port coordination via pool
            false, // enable_automation default
            args.headless.unwrap_or(false),
            false, // user_profile (must be false to allow multiple profile dirs)
        );
        child_params.secondary = true;
        if let Some(features) = args.features {
            child_params.set_features(features);
        }
        if let Some(proxy) = args.proxy {
            child_params.set_proxy(Some(proxy));
        }

        let chrome_manager: Arc<
            Mutex<dyn crate::chrome_mcp_handler::chrome_instance::ChromeManager>,
        > = if handler.is_test {
            let mock_port = 9000 + handler.registry.list_descriptors().len() as u16;
            let mut mock_mgr =
                crate::chrome_mcp_handler::chrome_instance::MockChromeManager::new(mock_port);
            mock_mgr.set_features(child_params.features().to_vec());
            Arc::new(Mutex::new(mock_mgr))
        } else {
            let manager = CdpBrowserManager::new(
                child_params.clone(),
                Box::new(RealLauncher {
                    pool: handler.pool.clone(),
                }),
            );
            Arc::new(Mutex::new(manager))
        };

        let session = Arc::new(BrowserSession {
            client: Arc::new(Mutex::new(None)),
            debugger_state: Arc::new(Mutex::new(DebuggerState::default())),
            network_state: Arc::new(Mutex::new(NetworkState::default())),
            log_state: Arc::new(Mutex::new(cdp_domains::log::LogState::default())),
            tracing_state: Arc::new(Mutex::new(cdp_domains::tracing::TracingState::default())),
            custom_state: Arc::new(Mutex::new(CustomState::default())),
            webmcp_state: Arc::new(Mutex::new(cdp_domains::webmcp::WebmcpState::default())),
            chrome_manager,
        });

        // Trigger ensure_instance to resolve the port
        {
            let mut mgr = session.chrome_manager.lock().await;
            mgr.ensure_instance().await.map_err(|e| {
                CallToolError::from_message(format!("Failed to spawn Chrome instance: {}", e))
            })?;
        }

        let resolved_port = {
            let mgr = session.chrome_manager.lock().await;
            mgr.get_port()
        };

        // Resolve profile directory path
        let plan = child_params.plan();
        let profile_dir = plan.profile.dir_for_port(resolved_port);

        let desc = InstanceDescriptor {
            id: instance_id.clone(),
            label: args.label.clone(),
            host: "127.0.0.1".to_string(),
            port: resolved_port,
            profile_dir,
            features: child_params
                .features()
                .iter()
                .map(|f| f.as_name().to_string())
                .collect(),
            is_default: false,
        };

        handler
            .registry
            .add_session(desc.clone(), session)
            .map_err(CallToolError::from_message)?;

        Ok(CallToolResult::text_content(vec![
            format!(
                "Opened Chrome instance '{}' on port {}. Profile directory: {:?}",
                desc.id, desc.port, desc.profile_dir
            )
            .into(),
        ]))
    }
}
