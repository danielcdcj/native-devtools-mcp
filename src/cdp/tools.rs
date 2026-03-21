//! CDP tool implementations (evaluate_script, click, list_pages, select_page).

use crate::cdp::{cdp_error, is_extension_url, CdpClient};
use crate::tools::ax_snapshot::format_snapshot;
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, GetBoxModelParams, ResolveNodeParams, ScrollIntoViewIfNeededParams,
};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EvaluateParams,
};
use rmcp::model::{CallToolResult, Content};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Format a JS execution response into a tool result, handling exceptions.
fn format_js_result(
    result: &chromiumoxide::cdp::js_protocol::runtime::EvaluateReturns,
) -> CallToolResult {
    if let Some(exc) = &result.exception_details {
        return cdp_error(format!("JavaScript exception: {}", exc.text));
    }
    let value = result
        .result
        .value
        .as_ref()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string()),
    )])
}

pub async fn cdp_evaluate_script(
    function: String,
    args: Option<Vec<serde_json::Value>>,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Determine if we need to resolve element args.
    let has_uid_args = args
        .as_ref()
        .is_some_and(|a| a.iter().any(|v| v.get("uid").is_some()));

    if !has_uid_args {
        // Simple case: evaluate the expression directly.
        // If it looks like a function declaration or arrow function, wrap as an IIFE
        // so `() => document.title` returns the title, not the function object.
        // Use `=>` presence to detect arrow functions — avoids false positives on
        // parenthesized expressions like `(1 + 2)` or `({ title: document.title })`.
        let trimmed = function.trim_start();
        let is_function = trimmed.starts_with("function")
            || trimmed.starts_with("async function")
            || function.contains("=>");
        let expression = if is_function {
            format!("({})()", function)
        } else {
            function
        };

        let mut eval_params = EvaluateParams::new(expression);
        eval_params.return_by_value = Some(true);
        eval_params.await_promise = Some(true);

        return match page.execute(eval_params).await {
            Ok(resp) => format_js_result(&resp.result),
            Err(e) => cdp_error(format!("Failed to evaluate script: {}", e)),
        };
    }

    // Args case: resolve UIDs to remote objects and call function with them.
    let current_url = page.url().await.ok().flatten().unwrap_or_default();
    let snapshot_map = match client.check_snapshot_staleness(&current_url) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let arg_list = match args.as_ref() {
        Some(a) => a,
        None => return cdp_error("args required when passing element references"),
    };
    let mut call_arguments: Vec<CallArgument> = Vec::with_capacity(arg_list.len());

    // Collect (uid, backend_node_id) pairs first to avoid borrow issues.
    let mut uid_backend_pairs: Vec<(String, i64)> = Vec::with_capacity(arg_list.len());
    for arg in arg_list {
        if let Some(uid) = arg.get("uid").and_then(|v| v.as_str()) {
            let node = match snapshot_map.uid_to_node.get(uid) {
                Some(n) => n,
                None => {
                    return cdp_error(format!(
                        "uid={} not found. Call cdp_take_snapshot to refresh.",
                        uid
                    ));
                }
            };
            uid_backend_pairs.push((uid.to_string(), node.backend_node_id));
        }
    }

    // Resolve each backend node ID to a remote object ID.
    // Track the first element's objectId to use as the execution context for callFunctionOn.
    let mut first_object_id = None;
    for (uid, backend_node_id) in &uid_backend_pairs {
        let resolve_params = ResolveNodeParams::builder()
            .backend_node_id(BackendNodeId::new(*backend_node_id))
            .build();

        let remote_object = match page.execute(resolve_params).await {
            Ok(resp) => resp.result.object,
            Err(_) => {
                return cdp_error(format!(
                    "Element uid={} could not be resolved to a DOM node.",
                    uid
                ));
            }
        };

        let object_id = match remote_object.object_id {
            Some(id) => id,
            None => {
                return cdp_error(format!(
                    "Element uid={} could not be resolved to a DOM node.",
                    uid
                ));
            }
        };

        if first_object_id.is_none() {
            first_object_id = Some(object_id.clone());
        }
        call_arguments.push(CallArgument::builder().object_id(object_id).build());
    }

    // CDP callFunctionOn requires objectId or executionContextId.
    // Use the first element's objectId so the function executes in that element's context.
    let target_object_id = match first_object_id {
        Some(id) => id,
        None => return cdp_error("No element arguments could be resolved."),
    };

    // Call the function with the resolved element arguments.
    let call_params = match CallFunctionOnParams::builder()
        .function_declaration(function)
        .object_id(target_object_id)
        .arguments(call_arguments)
        .return_by_value(true)
        .await_promise(true)
        .build()
    {
        Ok(p) => p,
        Err(e) => return cdp_error(format!("Failed to build call params: {}", e)),
    };

    match page.execute(call_params).await {
        Ok(resp) => {
            // CallFunctionOn returns CallFunctionOnReturns, not EvaluateReturns.
            // Extract the same fields manually.
            if let Some(exc) = &resp.result.exception_details {
                return cdp_error(format!("JavaScript exception: {}", exc.text));
            }
            let value = resp
                .result
                .result
                .value
                .as_ref()
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string()),
            )])
        }
        Err(e) => cdp_error(format!("Failed to call function: {}", e)),
    }
}

pub async fn cdp_click(
    uid: String,
    dbl_click: bool,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let current_url = page.url().await.ok().flatten().unwrap_or_default();
    let snapshot_map = match client.check_snapshot_staleness(&current_url) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let node = match snapshot_map.uid_to_node.get(&uid) {
        Some(n) => n,
        None => {
            return cdp_error(format!(
                "uid={} not found. Call cdp_take_snapshot to get current elements.",
                uid
            ));
        }
    };

    let backend_node_id = BackendNodeId::new(node.backend_node_id);
    let node_role = node.role.clone();
    let node_name = node.name.clone();

    // Drop the read lock before doing async CDP calls.
    drop(guard);

    // Scroll the element into view.
    let scroll_params = ScrollIntoViewIfNeededParams::builder()
        .backend_node_id(backend_node_id)
        .build();

    if let Err(e) = page.execute(scroll_params).await {
        return cdp_error(format!(
            "Failed to scroll element uid={} into view: {}",
            uid, e
        ));
    }

    // Get box model to find element coordinates.
    let box_params = GetBoxModelParams::builder()
        .backend_node_id(backend_node_id)
        .build();

    let box_result = match page.execute(box_params).await {
        Ok(r) => r,
        Err(e) => {
            return cdp_error(format!(
                "Element uid={} is no longer in the DOM: {}",
                uid, e
            ));
        }
    };

    // Compute center from content quad (8 floats: x1,y1, x2,y2, x3,y3, x4,y4).
    let quad = box_result.result.model.content.inner();
    if quad.len() < 8 {
        return cdp_error(format!(
            "Element uid={} returned an invalid box model (expected 8 quad values, got {}).",
            uid,
            quad.len()
        ));
    }
    let cx = (quad[0] + quad[2] + quad[4] + quad[6]) / 4.0;
    let cy = (quad[1] + quad[3] + quad[5] + quad[7]) / 4.0;

    let click_count = if dbl_click { 2_i64 } else { 1_i64 };

    // Dispatch mouse events: Move -> Press -> Release.
    let move_event = DispatchMouseEventParams::new(DispatchMouseEventType::MouseMoved, cx, cy);

    let mut press_event =
        DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, cx, cy);
    press_event.button = Some(MouseButton::Left);
    press_event.buttons = Some(1);
    press_event.click_count = Some(click_count);

    let mut release_event =
        DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, cx, cy);
    release_event.button = Some(MouseButton::Left);
    release_event.click_count = Some(click_count);

    for event in [move_event, press_event, release_event] {
        if let Err(e) = page.execute(event).await {
            return cdp_error(format!("Click failed on uid={}: {}", uid, e));
        }
    }

    let dbl_note = if dbl_click { " (double-click)" } else { "" };
    CallToolResult::success(vec![Content::text(format!(
        "Clicked uid={} '{}' ({}) at ({:.1}, {:.1}){}",
        uid, node_name, node_role, cx, cy, dbl_note
    ))])
}

pub async fn cdp_list_pages(cdp_client: Arc<RwLock<Option<CdpClient>>>) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let pages = match client.browser.pages().await {
        Ok(p) => p,
        Err(e) => return cdp_error(format!("Failed to list pages: {}", e)),
    };

    // Filter out chrome-extension:// pages.
    let mut filtered: Vec<chromiumoxide::page::Page> = Vec::new();
    for page in pages {
        let url = page.url().await.ok().flatten().unwrap_or_default();
        if !is_extension_url(&url) {
            filtered.push(page);
        }
    }

    // Determine which page is currently selected (by comparing URLs).
    let selected_url = match &client.selected_page {
        Some(p) => p.url().await.ok().flatten().unwrap_or_default(),
        None => String::new(),
    };

    let total = filtered.len();
    let mut output = format!("Pages ({} total):\n", total);
    for (i, page) in filtered.iter().enumerate() {
        let url = page.url().await.ok().flatten().unwrap_or_default();
        let marker = if url == selected_url && !url.is_empty() {
            " *"
        } else {
            ""
        };
        output.push_str(&format!("  [{}]{} {}\n", i, marker, url));
    }

    client.last_page_list = filtered;

    CallToolResult::success(vec![Content::text(output.trim_end().to_string())])
}

pub async fn cdp_select_page(
    page_idx: usize,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    if client.last_page_list.is_empty() {
        return cdp_error("No page list available. Call cdp_list_pages first.");
    }

    if page_idx >= client.last_page_list.len() {
        return cdp_error(format!(
            "Page index {} is out of range (0..{}). Call cdp_list_pages to refresh.",
            page_idx,
            client.last_page_list.len()
        ));
    }

    let page = client.last_page_list[page_idx].clone();

    if let Err(e) = page.bring_to_front().await {
        return cdp_error(format!("Failed to bring page {} to front: {}", page_idx, e));
    }

    let url = page.url().await.ok().flatten().unwrap_or_default();
    client.selected_page = Some(page);
    client.last_snapshot = None;

    CallToolResult::success(vec![Content::text(format!(
        "Selected page [{}]: {}",
        page_idx, url
    ))])
}

pub async fn cdp_take_snapshot(cdp_client: Arc<RwLock<Option<CdpClient>>>) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let result = page
        .execute(
            chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams::default(),
        )
        .await;

    match result {
        Ok(response) => {
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
        Err(e) => cdp_error(format!("Failed to get accessibility tree: {}", e)),
    }
}
