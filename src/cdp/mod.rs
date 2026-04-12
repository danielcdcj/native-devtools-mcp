//! Chrome DevTools Protocol (CDP) client for browser automation.
//!
//! Connects to Chrome/Electron apps via their remote debugging port
//! using the chromiumoxide crate.

pub mod dom_discovery;
pub mod snapshot;
pub mod tools;

use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
use futures_util::StreamExt;
use rmcp::model::{CallToolResult, Content};
use std::collections::HashMap;
use tokio::task::JoinHandle;

pub const AX_UID_PREFIX: &str = "a";
pub const DOM_UID_PREFIX: &str = "d";

/// CDP client state, owned by the MCP server.
pub struct CdpClient {
    pub browser: Browser,
    pub selected_page: Option<Page>,
    pub handler_handle: JoinHandle<()>,
    pub last_ax_snapshot: Option<SnapshotMap>,
    pub last_dom_snapshot: Option<SnapshotMap>,
    pub last_page_list: Vec<Page>,
    /// Monotonic counter bumped on every page-lifecycle event that could
    /// invalidate the `backendNodeId` space (navigate, reload, select/new/close
    /// page). Stamped onto each [`SnapshotMap`] at creation time so lookups
    /// can detect stale snapshots even when the page URL hasn't changed
    /// (same-URL reload, SPA pushState/replaceState, switching to another tab
    /// with an identical URL).
    pub generation: u64,
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
        // Chrome 136+ with chromiumoxide: fetch_targets() queues discovery but
        // Page objects are NOT guaranteed to be ready when it returns. We must
        // poll until at least one real page appears or we time out.
        let selected_page = poll_for_page(&mut browser, std::time::Duration::from_secs(10)).await?;

        Ok(Self {
            browser,
            selected_page,
            handler_handle,
            last_ax_snapshot: None,
            last_dom_snapshot: None,
            last_page_list: Vec::new(),
            generation: 0,
        })
    }

    /// Disconnect from the browser by aborting the handler task.
    pub fn disconnect(self) {
        self.handler_handle.abort();
    }

    /// Mark the current `backendNodeId` space as invalidated.
    ///
    /// Bumps [`Self::generation`] and clears both snapshot caches. Call
    /// after any navigation, reload, or page switch that invalidates
    /// element UIDs.
    pub fn invalidate_snapshots(&mut self) {
        self.last_ax_snapshot = None;
        self.last_dom_snapshot = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Get the selected page, or return a tool error.
    pub fn require_page(&self) -> Result<Page, CallToolResult> {
        self.selected_page.clone().ok_or_else(|| {
            cdp_error("No page selected. Use cdp_list_pages and cdp_select_page first.")
        })
    }
}

/// Convenience helper to get the URL of a page, returning an empty string on failure.
pub async fn page_url(page: &Page) -> String {
    page.url().await.ok().flatten().unwrap_or_default()
}

/// Return true if the URL belongs to a Chrome extension.
pub(crate) fn is_extension_url(url: &str) -> bool {
    url.starts_with("chrome-extension://")
}

/// Find the first non-extension page from a list of pages.
async fn first_non_extension_page(pages: &[Page]) -> Option<Page> {
    for page in pages {
        let url = page_url(page).await;
        if !is_extension_url(&url) {
            return Some(page.clone());
        }
    }
    None
}

/// Discover pre-existing targets and wait for at least one page to appear.
///
/// `fetch_targets()` sends `Target.getTargets` and triggers `AttachToTarget`
/// for each discovered target. The attach is asynchronous — the handler must
/// process the responses before `pages()` can see them. We call `fetch_targets`
/// once, then poll `pages()` until a non-extension page appears or we time out.
async fn poll_for_page(
    browser: &mut Browser,
    timeout: std::time::Duration,
) -> Result<Option<Page>, String> {
    // Kick off target discovery once. This triggers AttachToTarget for each
    // existing target, which the handler processes asynchronously.
    let _ = browser.fetch_targets().await;

    let interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    loop {
        let pages = browser
            .pages()
            .await
            .map_err(|e| format!("Failed to list pages: {}", e))?;

        if let Some(page) = first_non_extension_page(&pages).await {
            return Ok(Some(page));
        }

        if start.elapsed() >= timeout {
            return Ok(None);
        }

        tokio::time::sleep(interval).await;
    }
}

/// Shorthand for building a CDP tool error result.
pub fn cdp_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.into())])
}

/// Maps snapshot UIDs to CDP node identifiers for click/eval resolution.
///
/// Stale-snapshot detection uses `generation`, which is bumped on every
/// page-lifecycle event (see [`CdpClient::invalidate_snapshots`]). Same-URL
/// reloads and SPA navigations that don't change the URL still invalidate
/// via the generation counter.
pub struct SnapshotMap {
    pub uid_to_node: HashMap<String, SnapshotNode>,
    /// Reverse map: backendNodeId → list of snapshot UIDs.
    /// Skips entries where backendNodeId is 0 (no DOM backing).
    pub backend_to_uids: HashMap<i64, Vec<String>>,
    /// Value of [`CdpClient::generation`] at the moment this snapshot was taken.
    pub generation: u64,
}

pub struct SnapshotNode {
    pub backend_node_id: i64,
    pub role: String,
    pub name: String,
}

/// Resolve a prefixed UID to its SnapshotNode from the correct map.
///
/// UIDs prefixed with "a" resolve from the AX snapshot map.
/// UIDs prefixed with "d" resolve from the DOM snapshot map.
/// Returns an error if the prefix is unknown, the map is missing,
/// the snapshot's generation doesn't match `current_generation`
/// (page has navigated), or the UID is not found.
pub fn resolve_uid_from_maps<'a>(
    uid: &str,
    ax_snapshot: Option<&'a SnapshotMap>,
    dom_snapshot: Option<&'a SnapshotMap>,
    current_generation: u64,
) -> Result<&'a SnapshotNode, String> {
    let (map, label, tool_hint) = if uid.starts_with(AX_UID_PREFIX) {
        (ax_snapshot, "AX", "cdp_take_ax_snapshot")
    } else if uid.starts_with(DOM_UID_PREFIX) {
        (
            dom_snapshot,
            "DOM",
            "cdp_take_dom_snapshot or cdp_find_elements",
        )
    } else {
        return Err(format!(
            "Unknown UID prefix in '{}'. Expected 'a<N>' (AX) or 'd<N>' (DOM).",
            uid
        ));
    };

    let snapshot =
        map.ok_or_else(|| format!("No {} snapshot available. Call {} first.", label, tool_hint))?;

    if current_generation != snapshot.generation {
        return Err(format!(
            "Snapshot is stale — page has navigated since last snapshot. Call {} again.",
            tool_hint,
        ));
    }

    snapshot.uid_to_node.get(uid).ok_or_else(|| {
        format!(
            "uid={} not found in {} snapshot. Take a fresh snapshot.",
            uid, label,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ax_map(generation: u64, uid: &str, backend_node_id: i64) -> SnapshotMap {
        let mut map = SnapshotMap {
            uid_to_node: HashMap::new(),
            backend_to_uids: HashMap::new(),
            generation,
        };
        map.uid_to_node.insert(
            uid.to_string(),
            SnapshotNode {
                backend_node_id,
                role: "button".to_string(),
                name: "Submit".to_string(),
            },
        );
        map
    }

    #[test]
    fn resolve_uid_ax_prefix() {
        let ax_map = make_ax_map(0, "a1", 42);

        let result = resolve_uid_from_maps("a1", Some(&ax_map), None, 0);
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.backend_node_id, 42);
        assert_eq!(node.role, "button");
    }

    #[test]
    fn resolve_uid_dom_prefix() {
        let mut dom_map = SnapshotMap {
            uid_to_node: HashMap::new(),
            backend_to_uids: HashMap::new(),
            generation: 3,
        };
        dom_map.uid_to_node.insert(
            "d5".to_string(),
            SnapshotNode {
                backend_node_id: 99,
                role: "textbox".to_string(),
                name: "Search".to_string(),
            },
        );

        let result = resolve_uid_from_maps("d5", None, Some(&dom_map), 3);
        assert!(result.is_ok());
        let node = result.unwrap();
        assert_eq!(node.backend_node_id, 99);
    }

    #[test]
    fn resolve_uid_unknown_prefix_fails() {
        let result = resolve_uid_from_maps("x1", None, None, 0);
        assert!(result.is_err());
    }

    fn expect_stale(result: Result<&SnapshotNode, String>) {
        match result {
            Err(msg) => assert!(msg.contains("stale"), "expected stale error, got: {}", msg),
            Ok(_) => panic!("expected stale-snapshot error, got Ok"),
        }
    }

    #[test]
    fn resolve_uid_stale_generation_fails() {
        let ax_map = make_ax_map(1, "a1", 1);

        expect_stale(resolve_uid_from_maps("a1", Some(&ax_map), None, 2));
    }

    /// Same-URL reload bumps the generation, so a snapshot taken before
    /// the reload must be rejected even though `page.url()` hasn't changed.
    #[test]
    fn same_url_reload_invalidates_snapshot() {
        let ax_map = make_ax_map(0, "a1", 42);

        expect_stale(resolve_uid_from_maps("a1", Some(&ax_map), None, 1));
    }

    /// A generation advance past either snapshot must mark both as stale.
    #[test]
    fn both_maps_stale_when_generation_advanced() {
        // Both snapshots stamped at generation=5, but client is now at 7.
        let ax_map = make_ax_map(5, "a1", 1);
        let mut dom_map = SnapshotMap {
            uid_to_node: HashMap::new(),
            backend_to_uids: HashMap::new(),
            generation: 5,
        };
        dom_map.uid_to_node.insert(
            "d1".to_string(),
            SnapshotNode {
                backend_node_id: 2,
                role: "textbox".to_string(),
                name: "".to_string(),
            },
        );

        expect_stale(resolve_uid_from_maps(
            "a1",
            Some(&ax_map),
            Some(&dom_map),
            7,
        ));
        expect_stale(resolve_uid_from_maps(
            "d1",
            Some(&ax_map),
            Some(&dom_map),
            7,
        ));
    }

    /// A snapshot looked up at its stamped generation succeeds; bumping
    /// the generation causes the same snapshot to be rejected as stale.
    #[test]
    fn snapshot_taken_before_navigation_is_stale_after_bump() {
        let ax_map = make_ax_map(0, "a1", 42);

        assert!(resolve_uid_from_maps("a1", Some(&ax_map), None, 0).is_ok());

        expect_stale(resolve_uid_from_maps("a1", Some(&ax_map), None, 1));
    }
}
