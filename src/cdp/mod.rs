//! Chrome DevTools Protocol (CDP) client for browser automation.
//!
//! Connects to Chrome/Electron apps via their remote debugging port
//! using the chromiumoxide crate.

pub mod snapshot;
pub mod tools;

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures_util::StreamExt;
use std::collections::HashMap;
use tokio::task::JoinHandle;

/// CDP client state, owned by the MCP server.
pub struct CdpClient {
    pub browser: Browser,
    pub selected_page: Option<Page>,
    pub handler_handle: JoinHandle<()>,
    pub last_snapshot: Option<SnapshotMap>,
    pub last_page_list: Vec<Page>,
}

impl CdpClient {
    /// Connect to a Chrome/Electron instance via its remote debugging port.
    ///
    /// Resolves the WebSocket URL from `http://127.0.0.1:{port}`, spawns the
    /// chromiumoxide handler loop, and auto-selects the first non-extension page.
    pub async fn connect(port: u16) -> Result<Self, String> {
        let url = format!("http://127.0.0.1:{}", port);
        let (browser, mut handler) = Browser::connect(&url)
            .await
            .map_err(|e| format!("Failed to connect to Chrome on port {}: {}", port, e))?;

        // The handler must be continuously polled in a background task.
        let handler_handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

        // Fetch pages and auto-select the first non-extension page.
        let pages = browser
            .pages()
            .await
            .map_err(|e| format!("Failed to list pages: {}", e))?;

        // Auto-select the first non-extension page.
        let mut selected_page = None;
        for page in &pages {
            let url = page.url().await.ok().flatten().unwrap_or_default();
            if !url.starts_with("chrome-extension://") {
                selected_page = Some(page.clone());
                break;
            }
        }

        let last_page_list = pages;

        Ok(Self {
            browser,
            selected_page,
            handler_handle,
            last_snapshot: None,
            last_page_list,
        })
    }

    /// Disconnect from the browser by aborting the handler task.
    pub fn disconnect(self) {
        self.handler_handle.abort();
        // Browser and other fields drop naturally.
    }
}

/// Maps snapshot UIDs to CDP node identifiers for click/eval resolution.
/// Stores page_url and navigation_id for stale snapshot detection.
pub struct SnapshotMap {
    pub uid_to_node: HashMap<String, SnapshotNode>,
    pub page_url: String,
    pub navigation_id: Option<String>,
}

pub struct SnapshotNode {
    pub backend_node_id: i64,
    pub role: String,
    pub name: String,
}
