//! Chrome DevTools Protocol (CDP) client for browser automation.
//!
//! Connects to Chrome/Electron apps via their remote debugging port
//! using the chromiumoxide crate.

pub mod snapshot;
pub mod tools;

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures_util::StreamExt;
use rmcp::model::{CallToolResult, Content};
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
        let (mut browser, mut handler) = Browser::connect(&url)
            .await
            .map_err(|e| format!("Cannot connect to port {}. Is the app running with --remote-debugging-port? Error: {}", port, e))?;

        let handler_handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

        // Discover pre-existing targets (pages opened before we connected).
        // fetch_targets() queues discovery; pages() may not see them immediately.
        // Retry briefly to let target attachment complete.
        let _ = browser.fetch_targets().await;
        let mut pages = browser
            .pages()
            .await
            .map_err(|e| format!("Failed to list pages: {}", e))?;

        if pages.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let _ = browser.fetch_targets().await;
            pages = browser
                .pages()
                .await
                .map_err(|e| format!("Failed to list pages: {}", e))?;
        }

        let selected_page = first_non_extension_page(&pages).await;

        Ok(Self {
            browser,
            selected_page,
            handler_handle,
            last_snapshot: None,
            last_page_list: pages,
        })
    }

    /// Disconnect from the browser by aborting the handler task.
    pub fn disconnect(self) {
        self.handler_handle.abort();
    }

    /// Get the selected page, or return a tool error.
    pub fn require_page(&self) -> Result<Page, CallToolResult> {
        self.selected_page.clone().ok_or_else(|| {
            cdp_error("No page selected. Use cdp_list_pages and cdp_select_page first.")
        })
    }

    /// Get the snapshot map, or return a tool error.
    pub fn require_snapshot(&self) -> Result<&SnapshotMap, CallToolResult> {
        self.last_snapshot
            .as_ref()
            .ok_or_else(|| cdp_error("No snapshot available. Call cdp_take_snapshot first."))
    }

    /// Verify the snapshot is still valid for the given page URL.
    pub fn check_snapshot_staleness(
        &self,
        current_url: &str,
    ) -> Result<&SnapshotMap, CallToolResult> {
        let snapshot = self.require_snapshot()?;
        if current_url != snapshot.page_url {
            return Err(cdp_error(
                "Snapshot is stale \u{2014} page has navigated since last snapshot. Call cdp_take_snapshot again.",
            ));
        }
        Ok(snapshot)
    }
}

/// Return true if the URL belongs to a Chrome extension.
pub(crate) fn is_extension_url(url: &str) -> bool {
    url.starts_with("chrome-extension://")
}

/// Find the first non-extension page from a list of pages.
async fn first_non_extension_page(pages: &[Page]) -> Option<Page> {
    for page in pages {
        let url = page.url().await.ok().flatten().unwrap_or_default();
        if !is_extension_url(&url) {
            return Some(page.clone());
        }
    }
    None
}

/// Shorthand for building a CDP tool error result.
pub fn cdp_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.into())])
}

/// Maps snapshot UIDs to CDP node identifiers for click/eval resolution.
/// Stores page_url for stale snapshot detection.
pub struct SnapshotMap {
    pub uid_to_node: HashMap<String, SnapshotNode>,
    pub page_url: String,
}

pub struct SnapshotNode {
    pub backend_node_id: i64,
    pub role: String,
    pub name: String,
}
