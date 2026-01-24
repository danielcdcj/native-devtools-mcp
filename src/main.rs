// Suppress warnings from cocoa/objc crates (deprecated APIs and cfg warnings)
#![allow(deprecated)]

mod app_protocol;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
mod server;
mod tools;

// Re-export platform module for unified access
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

use rmcp::ServiceExt;
use server::MacOSDevToolsServer;
use tokio::signal;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr (stdout is used for MCP protocol)
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env().add_directive("native_devtools_mcp=info".parse()?))
        .init();

    // Windows: Set DPI awareness before any GDI calls
    #[cfg(target_os = "windows")]
    if let Err(e) = platform::init() {
        tracing::warn!("Failed to initialize Windows platform: {}", e);
    }

    #[cfg(target_os = "macos")]
    tracing::info!("Starting macOS DevTools MCP server");
    #[cfg(target_os = "windows")]
    tracing::info!("Starting Windows DevTools MCP server");

    // Create the server
    let server = MacOSDevToolsServer::new();

    // Run as stdio transport
    let service = server.serve(rmcp::transport::stdio()).await?;

    // Wait for shutdown (either from service or SIGINT)
    tokio::select! {
        result = service.waiting() => {
            result?;
        }
        _ = signal::ctrl_c() => {
            tracing::info!("Received SIGINT, shutting down");
        }
    }

    tracing::info!("Server shut down");

    // Force exit to ensure all background tasks terminate
    std::process::exit(0);
}
