//! CDP tool implementations (evaluate_script, click, list_pages, select_page).

use crate::cdp::CdpClient;
use crate::tools::ax_snapshot::format_snapshot;
use rmcp::model::{CallToolResult, Content};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn cdp_take_snapshot(cdp_client: Arc<RwLock<Option<CdpClient>>>) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => {
            return CallToolResult::error(vec![Content::text(
                "No CDP connection. Use cdp_connect first.",
            )]);
        }
    };

    let page = match &client.selected_page {
        Some(p) => p,
        None => {
            return CallToolResult::error(vec![Content::text(
                "No page selected. Use cdp_list_pages and cdp_select_page first.",
            )]);
        }
    };

    // Call Accessibility.getFullAXTree via chromiumoxide.
    let result = page
        .execute(
            chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams::default(),
        )
        .await;

    match result {
        Ok(response) => {
            // Serialize each AxNode to serde_json::Value for the converter.
            let nodes_json: Vec<serde_json::Value> = response
                .result
                .nodes
                .iter()
                .map(|n| serde_json::to_value(n).unwrap_or_default())
                .collect();

            let page_url = page.url().await.ok().flatten().unwrap_or_default();

            let (snapshot_nodes, snapshot_map) =
                crate::cdp::snapshot::convert_cdp_ax_tree(&nodes_json, &page_url);

            let output = format_snapshot(&snapshot_nodes);
            client.last_snapshot = Some(snapshot_map);

            CallToolResult::success(vec![Content::text(output)])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "Failed to get accessibility tree: {}",
            e
        ))]),
    }
}
