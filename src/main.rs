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

    /// Run Chrome in headless mode (no GUI). Required for Docker environments.
    #[arg(long)]
    headless: bool,

    /// Target host for Chrome remote debugging (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Chrome remote debugging port
    #[arg(long, default_value_t = 9222)]
    port: u16,
}

#[tokio::main]
async fn main() -> SdkResult<()> {
    let args = Args::parse();
    eprintln!("[DEBUG] Starting with args: {:?}", args);

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
        instructions: None,
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default())?;
    let handler = ChromeMcpHandler::new_with_params(
        args.host,
        args.port,
        args.local,
        args.enable_automation,
        args.headless,
    )
    .to_mcp_server_handler();
    let server = server_runtime::create_server(rust_mcp_sdk::mcp_server::McpServerOptions {
        server_details: server_info,
        transport,
        handler,
        task_store: None,
        client_task_store: None,
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
        assert!(!args.local);
        assert!(!args.enable_automation);
        assert!(!args.headless);
    }

    #[test]
    fn test_args_parsing_enable_automation() {
        let args = Args::parse_from(["chrome-debug-mcp", "--enable-automation"]);
        assert!(args.enable_automation);
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
}
