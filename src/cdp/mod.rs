//! Chrome DevTools Protocol (CDP) client for browser automation.
//!
//! Connects to Chrome/Electron apps via their remote debugging port
//! using the chromiumoxide crate.

pub mod snapshot;
pub mod tools;

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
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
