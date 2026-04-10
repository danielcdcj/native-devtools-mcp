use std::sync::Arc;
use tokio::sync::RwLock;

use chromiumoxide::cdp::browser_protocol::dom::{DescribeNodeParams, GetNodeForLocationParams};
use rmcp::model::{CallToolResult, Content};

use crate::cdp::CdpClient;

/// Resolve screen coordinates to a CDP accessibility snapshot UID.
pub async fn cdp_element_at_point(
    x: f64,
    y: f64,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let client_guard = cdp_client.read().await;
    let client = match client_guard.as_ref() {
        Some(c) => c,
        None => return CallToolResult::error(vec![Content::text("No CDP connection active")]),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Step 1: Query window geometry and scroll offsets.
    let geo = match query_window_geometry(&page).await {
        Ok(g) => g,
        Err(e) => return CallToolResult::error(vec![Content::text(e)]),
    };

    // Step 2: Convert screen coords to viewport and page coords.
    let chrome_height = geo.outer_height - geo.inner_height;
    let viewport_x = x - geo.screen_x;
    let viewport_y = y - geo.screen_y - chrome_height;

    if viewport_x < 0.0
        || viewport_y < 0.0
        || viewport_x >= geo.inner_width
        || viewport_y >= geo.inner_height
    {
        return CallToolResult::error(vec![Content::text(format!(
            "Screen point ({}, {}) maps to viewport ({:.0}, {:.0}) which is outside \
             content area ({}x{}). The point may be in the title bar or outside the window.",
            x, y, viewport_x, viewport_y, geo.inner_width, geo.inner_height,
        ))]);
    }

    let page_x = viewport_x + geo.scroll_x;
    let page_y = viewport_y + geo.scroll_y;

    // Step 3: Hit-test via DOM.getNodeForLocation (page coords).
    let backend_node_id = match get_node_for_location(&page, page_x, page_y).await {
        Ok(id) => id,
        Err(_) => {
            // Step 4: Fallback via document.elementFromPoint (viewport coords).
            match element_from_point_fallback(&page, viewport_x, viewport_y).await {
                Ok(id) => id,
                Err(e) => {
                    return CallToolResult::error(vec![Content::text(format!(
                        "No element found at screen ({}, {}) / viewport ({:.0}, {:.0}): {}",
                        x, y, viewport_x, viewport_y, e,
                    ))]);
                }
            }
        }
    };

    // Step 5: Reverse-lookup in snapshot map.
    drop(client_guard);
    let result = reverse_lookup_uid(cdp_client.clone(), backend_node_id).await;
    match result {
        Ok((uid, role, name)) => {
            let json = serde_json::json!({
                "uid": uid,
                "role": role,
                "name": name,
                "backend_node_id": backend_node_id,
            });
            CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            )])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "Element found (backendNodeId={}) but not in accessibility snapshot: {}",
            backend_node_id, e,
        ))]),
    }
}

struct WindowGeometry {
    screen_x: f64,
    screen_y: f64,
    outer_height: f64,
    inner_width: f64,
    inner_height: f64,
    scroll_x: f64,
    scroll_y: f64,
}

async fn query_window_geometry(page: &chromiumoxide::Page) -> Result<WindowGeometry, String> {
    use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;

    let js = "JSON.stringify([window.screenX, window.screenY, window.outerHeight, \
              window.innerWidth, window.innerHeight, window.scrollX, window.scrollY])";

    let mut params = EvaluateParams::new(js);
    params.return_by_value = Some(true);
    let result = page
        .execute(params)
        .await
        .map_err(|e| format!("Failed to query window geometry: {}", e))?;

    let raw = result
        .result
        .result
        .value
        .as_ref()
        .and_then(|v| v.as_str())
        .ok_or("Empty geometry response")?;

    let vals: Vec<f64> =
        serde_json::from_str(raw).map_err(|e| format!("Failed to parse geometry: {}", e))?;

    if vals.len() < 7 {
        return Err(format!("Expected 7 geometry values, got {}", vals.len()));
    }

    Ok(WindowGeometry {
        screen_x: vals[0],
        screen_y: vals[1],
        outer_height: vals[2],
        inner_width: vals[3],
        inner_height: vals[4],
        scroll_x: vals[5],
        scroll_y: vals[6],
    })
}

async fn get_node_for_location(
    page: &chromiumoxide::Page,
    page_x: f64,
    page_y: f64,
) -> Result<i64, String> {
    let params = GetNodeForLocationParams::new(page_x as i64, page_y as i64);
    let result = page
        .execute(params)
        .await
        .map_err(|e| format!("DOM.getNodeForLocation failed: {}", e))?;

    Ok(*result.result.backend_node_id.inner())
}

async fn element_from_point_fallback(
    page: &chromiumoxide::Page,
    viewport_x: f64,
    viewport_y: f64,
) -> Result<i64, String> {
    use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;

    let js = format!(
        "document.elementFromPoint({}, {})",
        viewport_x as i64, viewport_y as i64
    );
    let params = EvaluateParams::new(js);
    let eval_result = page
        .execute(params)
        .await
        .map_err(|e| format!("elementFromPoint failed: {}", e))?;

    let object_id = eval_result
        .result
        .result
        .object_id
        .ok_or("elementFromPoint returned null (no element at coordinates)")?;

    let describe_params = DescribeNodeParams::builder().object_id(object_id).build();

    let describe_result = page
        .execute(describe_params)
        .await
        .map_err(|e| format!("DOM.describeNode failed: {}", e))?;

    Ok(*describe_result.result.node.backend_node_id.inner())
}

async fn reverse_lookup_uid(
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
    backend_node_id: i64,
) -> Result<(String, String, String), String> {
    // First try with existing snapshot, but only if it matches the current page URL.
    {
        let client_guard = cdp_client.read().await;
        let client = client_guard.as_ref().ok_or("No CDP client")?;
        if let Some(ref snapshot) = client.last_ax_snapshot {
            let page = client
                .require_page()
                .map_err(|_| "No selected page".to_string())?;
            let current_url = page.url().await.ok().flatten().unwrap_or_default();
            if current_url == snapshot.page_url {
                if let Some(result) = lookup_in_snapshot(snapshot, backend_node_id) {
                    return Ok(result);
                }
            }
        }
    }

    // Take a fresh snapshot and retry once.
    {
        let mut client_guard = cdp_client.write().await;
        let client = client_guard.as_mut().ok_or("No CDP client")?;
        let page = client
            .require_page()
            .map_err(|_| "No selected page".to_string())?;

        // Take fresh snapshot using the same logic as cdp_take_snapshot.
        let result = page
            .execute(
                chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams::default(),
            )
            .await
            .map_err(|e| format!("Failed to get accessibility tree: {}", e))?;

        let nodes_json: Vec<serde_json::Value> = result
            .result
            .nodes
            .iter()
            .map(|n| serde_json::to_value(n).unwrap_or_default())
            .collect();

        let page_url = page.url().await.ok().flatten().unwrap_or_default();
        let (_, snapshot_map) = crate::cdp::snapshot::convert_cdp_ax_tree(&nodes_json, &page_url);

        let found = lookup_in_snapshot(&snapshot_map, backend_node_id);
        client.last_ax_snapshot = Some(snapshot_map);

        if let Some(result) = found {
            return Ok(result);
        }
    }

    Err(format!(
        "backendNodeId {} not found in accessibility snapshot after refresh",
        backend_node_id
    ))
}

fn lookup_in_snapshot(
    snapshot: &crate::cdp::SnapshotMap,
    backend_node_id: i64,
) -> Option<(String, String, String)> {
    let uids = snapshot.backend_to_uids.get(&backend_node_id)?;
    if uids.len() == 1 {
        let uid = &uids[0];
        let node = snapshot.uid_to_node.get(uid)?;
        return Some((uid.clone(), node.role.clone(), node.name.clone()));
    }
    // Multiple UIDs for same backendNodeId: pick the one with a non-empty name.
    for uid in uids {
        if let Some(node) = snapshot.uid_to_node.get(uid) {
            if !node.name.is_empty() {
                return Some((uid.clone(), node.role.clone(), node.name.clone()));
            }
        }
    }
    // Fall back to first UID.
    let uid = &uids[0];
    let node = snapshot.uid_to_node.get(uid)?;
    Some((uid.clone(), node.role.clone(), node.name.clone()))
}
