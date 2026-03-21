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

    /// Chrome remote debugging port
    #[arg(long, default_value_t = 9222)]
    port: u16,
}

// TODO for version 1.0 the mcp will be able to run inside a docker container starting up a headless chrome inside the container, and optionally it will be able to manage a host machine browser if it started with debugging enabled.
// TODO render a sourrounding frame in the browser view when running any tool.

#[tokio::main]
async fn main() -> SdkResult<()> {
    let args = Args::parse();

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
    let handler = ChromeMcpHandler::new_with_port(args.port, args.local).to_mcp_server_handler();
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
