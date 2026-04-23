//! CDP script and snapshot tools: evaluate_script, find_elements,
//! take_dom_snapshot, wait_for.

use crate::cdp::{cdp_error, page_url, CdpClient};
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, DescribeNodeParams, ResolveNodeParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EvaluateParams, ReleaseObjectParams,
};
use chromiumoxide::page::Page;
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
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
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
    let arg_list = match args.as_ref() {
        Some(a) => a,
        None => return cdp_error("args required when passing element references"),
    };
    let mut call_arguments: Vec<CallArgument> = Vec::with_capacity(arg_list.len());

    // Collect (uid, backend_node_id) pairs first to avoid borrow issues.
    let mut uid_backend_pairs: Vec<(String, i64)> = Vec::with_capacity(arg_list.len());
    let current_url = page_url(&page).await;
    for arg in arg_list {
        if let Some(uid) = arg.get("uid").and_then(|v| v.as_str()) {
            let node = match crate::cdp::resolve_uid_from_maps(
                uid,
                client.last_dom_snapshot.as_ref(),
                client.generation,
                &current_url,
            ) {
                Ok(n) => n,
                Err(msg) => return cdp_error(msg),
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

const MAX_WAIT_TIMEOUT_MS: u64 = 60_000;

pub async fn cdp_wait_for(
    texts: Vec<String>,
    timeout_ms: Option<u64>,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let raw_timeout = timeout_ms.unwrap_or(10_000).min(MAX_WAIT_TIMEOUT_MS);
    let timeout = std::time::Duration::from_millis(raw_timeout);
    let poll_interval = std::time::Duration::from_millis(500);
    let start = std::time::Instant::now();

    // Build JS check: resolves true when any of the texts appear in the page body.
    // serde_json::to_string on Vec<String> is infallible.
    let texts_json = serde_json::to_string(&texts).unwrap();
    let check_js = format!(
        "document.body && {}.some(t => document.body.innerText.includes(t))",
        texts_json
    );

    loop {
        let found = {
            let guard = cdp_client.read().await;
            let client = match guard.as_ref() {
                Some(c) => c,
                None => return cdp_error("No CDP connection. Use cdp_connect first."),
            };
            let page = match client.require_page() {
                Ok(p) => p,
                Err(e) => return e,
            };

            let mut eval_params = EvaluateParams::new(&check_js);
            eval_params.return_by_value = Some(true);

            match page.execute(eval_params).await {
                Ok(resp) => resp
                    .result
                    .result
                    .value
                    .as_ref()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                Err(_) => false,
            }
        };

        if found {
            return cdp_take_dom_snapshot(None, cdp_client.clone()).await;
        }

        if start.elapsed() >= timeout {
            return cdp_error(format!(
                "Timed out after {}ms waiting for text: {}",
                timeout.as_millis(),
                texts_json
            ));
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Shared DOM walker + single-pass resolution logic used by both
/// `cdp_find_elements` and `cdp_take_dom_snapshot`.
///
/// Runs the JS walker with `return_by_value=false` to get element references,
/// then iterates to extract metadata and resolve `backendNodeId` atomically
/// via `DOM.describeNode`. Drops candidates where resolution fails or returns
/// `backendNodeId=0`.
async fn resolve_dom_candidates(
    page: &Page,
    walker_js: &str,
) -> Result<
    (
        Vec<crate::cdp::dom_discovery::DomCandidate>,
        serde_json::Value,
    ),
    CallToolResult,
> {
    // Step 1: Evaluate walker with return_by_value=false to get element references
    let mut eval_params = EvaluateParams::new(walker_js);
    eval_params.return_by_value = Some(false);

    let walker_result = match page.execute(eval_params).await {
        Ok(resp) => resp,
        Err(e) => return Err(cdp_error(format!("DOM walker failed: {}", e))),
    };

    let result_object_id = match walker_result.result.result.object_id {
        Some(id) => id,
        None => return Err(cdp_error("DOM walker returned no object reference")),
    };

    // Step 2: Extract inventory (by-value) from the result
    let inventory_js = "function() { return JSON.stringify(this.inventory); }";
    let inv_params = CallFunctionOnParams::builder()
        .function_declaration(inventory_js)
        .object_id(result_object_id.clone())
        .return_by_value(true)
        .build();
    let inventory: serde_json::Value = match page.execute(inv_params.unwrap()).await {
        Ok(resp) => resp
            .result
            .result
            .value
            .and_then(|v| v.as_str().and_then(|s| serde_json::from_str(s).ok()))
            .unwrap_or(serde_json::json!([])),
        Err(_) => serde_json::json!([]),
    };

    // Step 3: Extract all metadata in one bulk call (avoids per-element round-trips)
    let meta_js = "function() { return JSON.stringify(this.metadata); }";
    let meta_params = CallFunctionOnParams::builder()
        .function_declaration(meta_js)
        .object_id(result_object_id.clone())
        .return_by_value(true)
        .build();
    let all_metadata: Vec<crate::cdp::dom_discovery::DomCandidate> =
        match page.execute(meta_params.unwrap()).await {
            Ok(resp) => resp
                .result
                .result
                .value
                .and_then(|v| v.as_str().and_then(|s| serde_json::from_str(s).ok()))
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

    let element_count = all_metadata.len();

    // Step 4: For each element, get a reference and resolve backendNodeId via DOM.describeNode
    let mut candidates = Vec::with_capacity(element_count);
    for (i, mut candidate) in all_metadata.into_iter().enumerate() {
        // Get element reference
        let get_el_js = format!("function() {{ return this.elements[{}]; }}", i);
        let el_params = CallFunctionOnParams::builder()
            .function_declaration(&get_el_js)
            .object_id(result_object_id.clone())
            .return_by_value(false)
            .build();
        let el_object_id = match page.execute(el_params.unwrap()).await {
            Ok(resp) => match resp.result.result.object_id {
                Some(id) => id,
                None => continue,
            },
            Err(_) => continue,
        };

        // Resolve backendNodeId via DOM.describeNode on the element
        let el_oid_for_release = el_object_id.clone();
        let describe = DescribeNodeParams::builder()
            .object_id(el_object_id)
            .build();
        let describe_result = page.execute(describe).await;
        // Release the per-element remote object handle
        let _ = page
            .execute(ReleaseObjectParams::new(el_oid_for_release))
            .await;
        match describe_result {
            Ok(desc_resp) => {
                let id = *desc_resp.result.node.backend_node_id.inner();
                if id == 0 {
                    continue;
                }
                candidate.backend_node_id = id;
            }
            Err(_) => continue,
        };

        candidates.push(candidate);
    }

    // Release the wrapper remote object to avoid memory leaks
    let _ = page
        .execute(ReleaseObjectParams::new(result_object_id))
        .await;

    Ok((candidates, inventory))
}

pub async fn cdp_find_elements(
    query: String,
    role: Option<String>,
    max_results: Option<u32>,
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

    let max = max_results.unwrap_or(10);
    let page_url = page_url(&page).await;
    let generation = client.generation;

    let walker_js = crate::cdp::dom_discovery::dom_walker_js(&query, role.as_deref(), max);

    let (candidates, inventory) = match resolve_dom_candidates(&page, &walker_js).await {
        Ok(result) => result,
        Err(e) => return e,
    };

    // Build snapshot map and format response
    let snapshot_map =
        crate::cdp::dom_discovery::build_dom_snapshot(&candidates, page_url.clone(), generation);

    let matches_json: Vec<serde_json::Value> = candidates
        .iter()
        .enumerate()
        .map(|(i, n)| {
            serde_json::json!({
                "uid": format!("d{}", i + 1),
                "role": n.role,
                "label": n.label,
                "tag": n.tag,
                "disabled": n.disabled,
                "parent_role": n.parent_role,
                "parent_name": n.parent_name,
            })
        })
        .collect();

    client.last_dom_snapshot = Some(snapshot_map);

    let result = serde_json::json!({
        "page_url": page_url,
        "source": "dom",
        "matches": matches_json,
        "inventory": inventory,
    });

    CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&result).unwrap_or_default(),
    )])
}

pub async fn cdp_take_dom_snapshot(
    max_nodes: Option<u32>,
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

    let max = max_nodes.unwrap_or(500);
    let page_url = page_url(&page).await;
    let generation = client.generation;

    // Use empty query to match all interactive elements
    let walker_js = crate::cdp::dom_discovery::dom_walker_js("", None, max);

    let (candidates, _inventory) = match resolve_dom_candidates(&page, &walker_js).await {
        Ok(result) => result,
        Err(e) => return e,
    };

    let snapshot_map =
        crate::cdp::dom_discovery::build_dom_snapshot(&candidates, page_url, generation);

    let output = crate::cdp::dom_discovery::format_dom_snapshot(&candidates);
    client.last_dom_snapshot = Some(snapshot_map);

    CallToolResult::success(vec![Content::text(output)])
}
