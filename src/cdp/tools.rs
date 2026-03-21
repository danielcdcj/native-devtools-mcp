//! CDP tool implementations (evaluate_script, click, list_pages, select_page).

use crate::cdp::CdpClient;
use crate::tools::ax_snapshot::format_snapshot;
use chromiumoxide::cdp::browser_protocol::dom::{BackendNodeId, ResolveNodeParams};
use chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EvaluateParams,
};
use rmcp::model::{CallToolResult, Content};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn cdp_evaluate_script(
    function: String,
    args: Option<Vec<serde_json::Value>>,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
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
        Some(p) => p.clone(),
        None => {
            return CallToolResult::error(vec![Content::text(
                "No page selected. Use cdp_list_pages and cdp_select_page first.",
            )]);
        }
    };

    // Determine if we need to resolve element args.
    let has_uid_args = args
        .as_ref()
        .map_or(false, |a| a.iter().any(|v| v.get("uid").is_some()));

    if has_uid_args {
        // Args case: resolve UIDs to remote objects and call function with them.
        let snapshot_map = match &client.last_snapshot {
            Some(m) => m,
            None => {
                return CallToolResult::error(vec![Content::text(
                    "No snapshot available. Call cdp_take_snapshot first.",
                )]);
            }
        };

        let arg_list = args.as_ref().unwrap();
        let mut call_arguments: Vec<CallArgument> = Vec::with_capacity(arg_list.len());

        // Collect (uid, backend_node_id) pairs first to avoid borrow issues.
        let mut uid_backend_pairs: Vec<(String, i64)> = Vec::with_capacity(arg_list.len());
        for arg in arg_list {
            if let Some(uid) = arg.get("uid").and_then(|v| v.as_str()) {
                let node = match snapshot_map.uid_to_node.get(uid) {
                    Some(n) => n,
                    None => {
                        return CallToolResult::error(vec![Content::text(format!(
                            "uid={} not found. Call cdp_take_snapshot to refresh.",
                            uid
                        ))]);
                    }
                };
                uid_backend_pairs.push((uid.to_string(), node.backend_node_id));
            }
        }

        // Resolve each backend node ID to a remote object ID.
        for (uid, backend_node_id) in &uid_backend_pairs {
            let resolve_params = ResolveNodeParams::builder()
                .backend_node_id(BackendNodeId::new(*backend_node_id))
                .build();

            let resolve_result = page.execute(resolve_params).await;
            let remote_object = match resolve_result {
                Ok(resp) => resp.result.object,
                Err(_) => {
                    return CallToolResult::error(vec![Content::text(format!(
                        "Element uid={} could not be resolved to a DOM node.",
                        uid
                    ))]);
                }
            };

            let object_id = match remote_object.object_id {
                Some(id) => id,
                None => {
                    return CallToolResult::error(vec![Content::text(format!(
                        "Element uid={} could not be resolved to a DOM node.",
                        uid
                    ))]);
                }
            };

            let call_arg = CallArgument::builder().object_id(object_id).build();
            call_arguments.push(call_arg);
        }

        // Call the function with the resolved element arguments.
        let call_params = CallFunctionOnParams::builder()
            .function_declaration(function)
            .arguments(call_arguments)
            .return_by_value(true)
            .await_promise(true)
            .build();

        let call_params = match call_params {
            Ok(p) => p,
            Err(e) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Failed to build call params: {}",
                    e
                ))]);
            }
        };

        match page.execute(call_params).await {
            Ok(resp) => {
                if let Some(exc) = resp.result.exception_details {
                    return CallToolResult::error(vec![Content::text(format!(
                        "JavaScript exception: {}",
                        exc.text
                    ))]);
                }
                let value = resp.result.result.value.unwrap_or(serde_json::Value::Null);
                CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string()),
                )])
            }
            Err(e) => CallToolResult::error(vec![Content::text(format!(
                "Failed to call function: {}",
                e
            ))]),
        }
    } else {
        // Simple case: evaluate the expression directly.
        let mut eval_params = EvaluateParams::new(function);
        eval_params.return_by_value = Some(true);
        eval_params.await_promise = Some(true);

        match page.execute(eval_params).await {
            Ok(resp) => {
                if let Some(exc) = resp.result.exception_details {
                    return CallToolResult::error(vec![Content::text(format!(
                        "JavaScript exception: {}",
                        exc.text
                    ))]);
                }
                let value = resp.result.result.value.unwrap_or(serde_json::Value::Null);
                CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string()),
                )])
            }
            Err(e) => CallToolResult::error(vec![Content::text(format!(
                "Failed to evaluate script: {}",
                e
            ))]),
        }
    }
}

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
