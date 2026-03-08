mod chrome_mcp_handler;

use chrome_mcp_handler::ChromeMcpHandler;
use rust_mcp_sdk::{error::SdkResult, mcp_server::server_runtime, schema::*, *};

// TODO for the version 0.3.0 we will add the ability to click on buttons, follow links,...

#[tokio::main]
async fn main() -> SdkResult<()> {
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
    let handler = ChromeMcpHandler::default().to_mcp_server_handler();
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
