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
    description = "Opens a new independent Chrome instance. Side effects: launches a separate Chrome process with its own profile directory and remote-debugging port. Prerequisites: rejected when the server runs in --user-profile mode (Chrome's singleton profile lock). Returns: structured JSON with 'instance_id' (pass it as the 'instance_id' argument of other tools), 'host', 'port' and 'profile_dir'. Use this to isolate browsing sessions, cookies, proxies or WebMCP contexts from one another. Alternatives: 'open_tab' for additional tabs within an existing instance. Parameters: 'features' accepts a closed set - 'WEB_MCP' (enables the experimental WebMCP surface for sites that expose tools to the browser) or 'WEBGL_SOFTWARE' (forces SwiftShader software WebGL for GPU-less environments); 'headless' defaults to false (prefer false so the user can see the browser)."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, macros::JsonSchema)]
pub struct OpenInstanceTool {
    /// Optional label to identify the instance. The label becomes the instance_id returned by this tool and accepted by the 'instance_id' argument of other tools. If omitted, a dynamic label is generated.
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
            tabs: Arc::new(std::sync::RwLock::new(
                crate::chrome_mcp_handler::chrome_instance::tab_registry::TabRegistry::new(16),
            )),
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

        // Resolve the real profile directory from the live browser: with
        // ephemeral profiles the path is randomly generated at launch time and
        // cannot be derived from the port.
        let profile_dir = {
            let mgr = session.chrome_manager.lock().await;
            mgr.profile_dir().await
        };

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
            serde_json::json!({
                "instance_id": desc.id,
                "host": desc.host,
                "port": desc.port,
                "profile_dir": desc.profile_dir,
            })
            .to_string()
            .into(),
        ]))
    }
}
