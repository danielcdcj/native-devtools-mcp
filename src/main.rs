// Suppress warnings from cocoa/objc crates (deprecated APIs and cfg warnings)
#![allow(deprecated)]

mod app_protocol;
mod macos;
mod server;
mod tools;

use rmcp::ServiceExt;
use server::MacOSDevToolsServer;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr (stdout is used for MCP protocol)
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env().add_directive("macos_devtools_mcp=info".parse()?))
        .init();

    tracing::info!("Starting macOS DevTools MCP server");

    // Create the server
    let server = MacOSDevToolsServer::new();

    // Run as stdio transport
    let service = server.serve(rmcp::transport::stdio()).await?;

    // Wait for shutdown
    service.waiting().await?;

    tracing::info!("Server shut down");
    Ok(())
}
