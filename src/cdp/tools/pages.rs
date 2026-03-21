//! CDP page management tools: list, select, navigate, new, close, handle_dialog.

use crate::cdp::{cdp_error, is_extension_url, CdpClient};
use chromiumoxide::cdp::browser_protocol::page::{
    GetNavigationHistoryParams, HandleJavaScriptDialogParams, NavigateParams,
    NavigateToHistoryEntryParams, ReloadParams,
};
use rmcp::model::{CallToolResult, Content};
use std::sync::Arc;
use tokio::sync::RwLock;

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

    // Filter out chrome-extension:// pages, collecting URLs to avoid double fetch.
    let mut filtered: Vec<chromiumoxide::page::Page> = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    for page in pages {
        let url = page.url().await.ok().flatten().unwrap_or_default();
        if !is_extension_url(&url) {
            filtered.push(page);
            urls.push(url);
        }
    }

    let selected_target_id = client.selected_page.as_ref().map(|p| p.target_id().clone());

    let total = filtered.len();
    let mut output = format!("Pages ({} total):\n", total);
    for (i, page) in filtered.iter().enumerate() {
        let marker = if selected_target_id
            .as_ref()
            .is_some_and(|id| id == page.target_id())
        {
            " *"
        } else {
            ""
        };
        output.push_str(&format!("  [{}]{} {}\n", i, marker, urls[i]));
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

pub async fn cdp_handle_dialog(
    action: String,
    prompt_text: Option<String>,
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

    drop(guard);

    let accept = match action.as_str() {
        "accept" => true,
        "dismiss" => false,
        _ => {
            return cdp_error(format!(
                "Invalid action '{}'. Use 'accept' or 'dismiss'.",
                action
            ))
        }
    };

    let detail = if let Some(text) = &prompt_text {
        format!(" with text '{}'", text)
    } else {
        String::new()
    };

    let mut params = HandleJavaScriptDialogParams::new(accept);
    params.prompt_text = prompt_text;

    match page.execute(params).await {
        Ok(_) => CallToolResult::success(vec![Content::text(format!(
            "Dialog {}ed{}",
            action, detail
        ))]),
        Err(e) => cdp_error(format!("Failed to handle dialog: {}", e)),
    }
}

pub async fn cdp_navigate(
    url: Option<String>,
    nav_type: Option<String>,
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

    let action = nav_type.as_deref().unwrap_or("url");

    match action {
        "url" => {
            let target_url = match &url {
                Some(u) => u.clone(),
                None => return cdp_error("'url' parameter is required when type is 'url'."),
            };
            match page.execute(NavigateParams::new(&target_url)).await {
                Ok(_) => {
                    client.last_snapshot = None;
                    CallToolResult::success(vec![Content::text(format!(
                        "Navigated to {}",
                        target_url
                    ))])
                }
                Err(e) => cdp_error(format!("Navigation failed: {}", e)),
            }
        }
        "reload" => match page.execute(ReloadParams::default()).await {
            Ok(_) => {
                client.last_snapshot = None;
                CallToolResult::success(vec![Content::text("Page reloaded")])
            }
            Err(e) => cdp_error(format!("Reload failed: {}", e)),
        },
        "back" | "forward" => {
            let history = match page.execute(GetNavigationHistoryParams::default()).await {
                Ok(r) => r.result,
                Err(e) => return cdp_error(format!("Failed to get navigation history: {}", e)),
            };

            let target_idx = if action == "back" {
                history.current_index - 1
            } else {
                history.current_index + 1
            };

            if target_idx < 0 || target_idx as usize >= history.entries.len() {
                return cdp_error(format!("No {} history entry available.", action));
            }

            let entry = &history.entries[target_idx as usize];
            let entry_id = entry.id;
            let entry_url = entry.url.clone();

            match page
                .execute(NavigateToHistoryEntryParams::new(entry_id))
                .await
            {
                Ok(_) => {
                    client.last_snapshot = None;
                    CallToolResult::success(vec![Content::text(format!(
                        "Navigated {}: {}",
                        action, entry_url
                    ))])
                }
                Err(e) => cdp_error(format!("Navigation {} failed: {}", action, e)),
            }
        }
        _ => cdp_error(format!(
            "Invalid navigation type '{}'. Use 'url', 'back', 'forward', or 'reload'.",
            action
        )),
    }
}

pub async fn cdp_new_page(
    url: String,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let mut guard = cdp_client.write().await;
    let client = match guard.as_mut() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.browser.new_page(&url).await {
        Ok(p) => p,
        Err(e) => return cdp_error(format!("Failed to create new page: {}", e)),
    };

    let page_url = page.url().await.ok().flatten().unwrap_or_default();
    client.selected_page = Some(page);
    client.last_snapshot = None;

    CallToolResult::success(vec![Content::text(format!(
        "Created and selected new page: {}",
        page_url
    ))])
}

pub async fn cdp_close_page(
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

    if client.last_page_list.len() <= 1 {
        return cdp_error("Cannot close the last open page.");
    }

    if page_idx >= client.last_page_list.len() {
        return cdp_error(format!(
            "Page index {} is out of range (0..{}). Call cdp_list_pages to refresh.",
            page_idx,
            client.last_page_list.len()
        ));
    }

    let page_to_close = client.last_page_list.remove(page_idx);
    let url = page_to_close.url().await.ok().flatten().unwrap_or_default();

    let is_selected = client
        .selected_page
        .as_ref()
        .is_some_and(|selected| selected.target_id() == page_to_close.target_id());

    if let Err(e) = page_to_close.close().await {
        return cdp_error(format!("Failed to close page [{}]: {}", page_idx, e));
    }

    if is_selected {
        if let Some(replacement) = client.last_page_list.first() {
            client.selected_page = Some(replacement.clone());
            client.last_snapshot = None;
        }
    }

    CallToolResult::success(vec![Content::text(format!(
        "Closed page [{}]: {}",
        page_idx, url
    ))])
}
