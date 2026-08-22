mod chrome_mcp_handler;

use chrome_mcp_handler::ChromeMcpHandler;
use clap::Parser;
use rust_mcp_sdk::{error::SdkResult, mcp_server::server_runtime, schema::*, *};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Restricted to local addresses only (localhost, 127.0.0.1, 192.168.x.x, *.local)
    #[arg(long)]
    local: bool,

    /// Enables the 'Chrome is being controlled by automated test software' infobar
    #[arg(long)]
    enable_automation: bool,

    /// Use the default user profile (sessions, cookies, etc.) instead of a fresh one.
    /// This starts Chrome without --user-data-dir.
    #[arg(long)]
    user_profile: bool,

    /// Run Chrome in headless mode (no GUI). Required for Docker environments.
    #[arg(long)]
    headless: bool,

    /// Target host for Chrome remote debugging (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Chrome remote debugging port
    #[arg(long, default_value_t = 9222)]
    port: u16,

    /// Maximum number of concurrent Chrome instances (default: 8)
    #[arg(long, default_value_t = 8)]
    max_instances: usize,
}

#[tokio::main]
async fn main() -> SdkResult<()> {
    let args = Args::parse();
    eprintln!("[DEBUG] Starting with args: {:?}", args);

    let instructions = concat!(
        "Chrome Debug MCP Server Instructions:\n",
        "- Chrome instances are launched lazily on the first tool call that requires connection.\n",
        "- Instances: a 'default' instance always exists. To run additional isolated browser sessions, call `open_instance` with a unique 'label'; that label becomes the returned instance_id. Pass the instance_id as the 'instance_id' argument of any tool to address that instance (omit it for the default). Use `list_instances` to enumerate them and `close_instance` to stop one.\n",
        "- Tabs: within an instance, create extra tabs with `open_tab` and enumerate them with `list_tabs`. Every tool accepts a 'tab_id' argument; omit it to target the active tab, and use `switch_tab` to change which tab is active. Per-tab state (console logs, breakpoints, network traffic) is isolated between tabs.\n",
        "- WebMCP (interaction with page-provided tools) is an opt-in feature. To enable it on the default instance, call `restart_chrome` with `features: [\"WEB_MCP\"]` and reload the target page. For new instances, pass `features: [\"WEB_MCP\"]` to `open_instance`.\n",
        "- When a page exposes WebMCP tools, use `webmcp_list_tools` and `webmcp_invoke_tool` to interact with them dynamically. This is faster and more reliable than raw DOM scraping.\n",
        "- If `webmcp_list_tools` returns an empty array, check the warning text in the output content. The 'WEB_MCP' preset might be disabled on that instance, or the page hasn't finished loading."
    );

    let server_info = InitializeResult {
        server_info: Implementation {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some("Chrome Debug MCP".into()),
            description: Some("Inspect and debug frontend code at runtime using CDP. Enable breakpoints and live code inspection to debug complex issues like race conditions in 'vibe coding' projects, providing LLMs with runtime state access.".into()),
            icons: vec![] as Vec<Icon>,
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        instructions: Some(instructions.to_string()),
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default())?;

    // Create handler with max_instances support
    let handler = ChromeMcpHandler::new_with_params(
        args.host,
        args.port,
        args.local,
        args.enable_automation,
        args.headless,
        args.user_profile,
    );
    handler
        .registry
        .max_instances
        .store(args.max_instances, std::sync::atomic::Ordering::SeqCst);
    let handler = handler.to_mcp_server_handler();
    let server = server_runtime::create_server(rust_mcp_sdk::mcp_server::McpServerOptions {
        server_details: server_info,
        transport,
        handler,
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    if let Err(e) = server.start().await {
        eprintln!("Server error: {:?}", e);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_parsing_defaults() {
        let args = Args::parse_from(["chrome-debug-mcp"]);
        assert_eq!(args.port, 9222);
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.max_instances, 8);
        assert!(!args.local);
        assert!(!args.enable_automation);
        assert!(!args.headless);
        assert!(!args.user_profile);
    }

    #[test]
    fn test_args_parsing_enable_automation() {
        let args = Args::parse_from(["chrome-debug-mcp", "--enable-automation"]);
        assert!(args.enable_automation);
    }

    #[test]
    fn test_args_parsing_user_profile() {
        let args = Args::parse_from(["chrome-debug-mcp", "--user-profile"]);
        assert!(args.user_profile);
    }

    #[test]
    fn test_args_parsing_local() {
        let args = Args::parse_from(["chrome-debug-mcp", "--local"]);
        assert!(args.local);
    }

    #[test]
    fn test_args_parsing_custom_port() {
        let args = Args::parse_from(["chrome-debug-mcp", "--port", "8080"]);
        assert_eq!(args.port, 8080);
    }

    #[test]
    fn test_args_parsing_headless() {
        let args = Args::parse_from(["chrome-debug-mcp", "--headless"]);
        assert!(args.headless);
    }

    #[test]
    fn test_args_parsing_custom_host() {
        let args = Args::parse_from(["chrome-debug-mcp", "--host", "host.docker.internal"]);
        assert_eq!(args.host, "host.docker.internal");
    }

    #[test]
    fn test_args_parsing_max_instances() {
        let args = Args::parse_from(["chrome-debug-mcp", "--max-instances", "16"]);
        assert_eq!(args.max_instances, 16);
    }
}
