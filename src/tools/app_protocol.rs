use crate::app_protocol::AppProtocolClient;
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SharedClient = Arc<RwLock<Option<AppProtocolClient>>>;

#[derive(Debug, Deserialize)]
pub struct AppConnectParams {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct AppGetTreeParams {
    #[serde(default)]
    pub depth: Option<i32>,
    #[serde(default)]
    pub root_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppQueryParams {
    pub selector: String,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppClickParams {
    pub element_id: String,
    #[serde(default)]
    pub click_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct AppTypeParams {
    pub text: String,
    #[serde(default)]
    pub element_id: Option<String>,
    #[serde(default)]
    pub clear_first: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppPressKeyParams {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppFocusParams {
    pub element_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AppScreenshotParams {
    #[serde(default)]
    pub element_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppGetElementParams {
    pub element_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AppFocusWindowParams {
    pub window_id: String,
}

pub async fn app_connect(params: AppConnectParams, client: SharedClient) -> CallToolResult {
    match AppProtocolClient::connect(&params.url).await {
        Ok(new_client) => {
            // Get app info
            let info = new_client.get_runtime_info().await;

            let mut guard = client.write().await;
            *guard = Some(new_client);

            match info {
                Ok(info) => CallToolResult::success(vec![Content::text(format!(
                    "Connected to app: {}",
                    serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".to_string())
                ))]),
                Err(_) => CallToolResult::success(vec![Content::text(format!(
                    "Connected to {}",
                    params.url
                ))]),
            }
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed to connect: {}", e))]),
    }
}

pub async fn app_disconnect(client: SharedClient) -> CallToolResult {
    let mut guard = client.write().await;
    if guard.is_some() {
        *guard = None;
        CallToolResult::success(vec![Content::text("Disconnected from app")])
    } else {
        CallToolResult::error(vec![Content::text("Not connected to any app")])
    }
}

pub async fn app_get_info(client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.get_runtime_info().await {
        Ok(info) => CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".to_string()),
        )]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_get_tree(params: AppGetTreeParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client
        .get_tree(params.depth, params.root_id.as_deref())
        .await
    {
        Ok(tree) => CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&tree).unwrap_or_else(|_| "{}".to_string()),
        )]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_query(params: AppQueryParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    let result = if params.all {
        client.query_selector_all(&params.selector).await
    } else {
        client.query_selector(&params.selector).await
    };

    match result {
        Ok(elements) => CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&elements).unwrap_or_else(|_| "{}".to_string()),
        )]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_get_element(params: AppGetElementParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.get_element(&params.element_id).await {
        Ok(element) => CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&element).unwrap_or_else(|_| "{}".to_string()),
        )]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_click(params: AppClickParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.click(&params.element_id, params.click_count).await {
        Ok(_) => CallToolResult::success(vec![Content::text(format!(
            "Clicked element: {}",
            params.element_id
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_type(params: AppTypeParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client
        .type_text(
            &params.text,
            params.element_id.as_deref(),
            params.clear_first,
        )
        .await
    {
        Ok(_) => CallToolResult::success(vec![Content::text(format!("Typed: {}", params.text))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_press_key(params: AppPressKeyParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.press_key(&params.key, params.modifiers).await {
        Ok(_) => CallToolResult::success(vec![Content::text(format!("Pressed: {}", params.key))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_focus(params: AppFocusParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.focus(&params.element_id).await {
        Ok(_) => CallToolResult::success(vec![Content::text(format!(
            "Focused element: {}",
            params.element_id
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_screenshot(params: AppScreenshotParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.get_screenshot(params.element_id.as_deref()).await {
        Ok(result) => {
            // Extract base64 data from result
            if let Some(data) = result.get("data").and_then(|v| v.as_str()) {
                let width = result.get("width").and_then(|v| v.as_i64()).unwrap_or(0);
                let height = result.get("height").and_then(|v| v.as_i64()).unwrap_or(0);

                CallToolResult::success(vec![
                    Content::text(format!("Screenshot: {}x{}", width, height)),
                    Content::image(data, "image/png"),
                ])
            } else {
                CallToolResult::error(vec![Content::text("Invalid screenshot response")])
            }
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_list_windows(client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.list_windows().await {
        Ok(windows) => CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&windows).unwrap_or_else(|_| "{}".to_string()),
        )]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}

pub async fn app_focus_window(params: AppFocusWindowParams, client: SharedClient) -> CallToolResult {
    let guard = client.read().await;
    let Some(client) = guard.as_ref() else {
        return CallToolResult::error(vec![Content::text("Not connected. Use app_connect first.")]);
    };

    match client.focus_window(&params.window_id).await {
        Ok(_) => CallToolResult::success(vec![Content::text(format!(
            "Focused window: {}",
            params.window_id
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed: {}", e))]),
    }
}
