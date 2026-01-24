use crate::platform;
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ListWindowsParams {
    /// Filter by application name (optional)
    pub app_name: Option<String>,
}

pub fn list_windows(params: ListWindowsParams) -> CallToolResult {
    let windows = if let Some(app_name) = params.app_name {
        match platform::find_windows_by_app(&app_name) {
            Ok(w) => w,
            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
        }
    } else {
        match platform::list_windows() {
            Ok(w) => w,
            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
        }
    };

    match serde_json::to_string_pretty(&windows) {
        Ok(json) => CallToolResult::success(vec![Content::text(json)]),
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "Failed to serialize windows: {}",
            e
        ))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct ListAppsParams {}

pub fn list_apps(_params: ListAppsParams) -> CallToolResult {
    let apps = platform::list_apps();

    match serde_json::to_string_pretty(&apps) {
        Ok(json) => CallToolResult::success(vec![Content::text(json)]),
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "Failed to serialize apps: {}",
            e
        ))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct FocusWindowParams {
    /// Window ID to focus (optional, use with window_id)
    pub window_id: Option<u32>,

    /// Application name to focus (optional, use with app_name)
    pub app_name: Option<String>,

    /// Process ID to focus (optional, use with pid)
    pub pid: Option<i32>,
}

pub fn focus_window(params: FocusWindowParams) -> CallToolResult {
    let success = if let Some(app_name) = params.app_name {
        platform::activate_app(&app_name)
    } else if let Some(pid) = params.pid {
        platform::activate_app_by_pid(pid)
    } else if let Some(window_id) = params.window_id {
        // For window_id, we need to find the app that owns it and activate that
        match platform::find_window_by_id(window_id) {
            Ok(Some(window)) => platform::activate_app_by_pid(window.owner_pid as i32),
            Ok(None) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Window {} not found",
                    window_id
                ))]);
            }
            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
        }
    } else {
        return CallToolResult::error(vec![Content::text(
            "Provide one of: window_id, app_name, or pid",
        )]);
    };

    if success {
        CallToolResult::success(vec![Content::text("Window focused successfully")])
    } else {
        CallToolResult::error(vec![Content::text("Failed to focus window")])
    }
}
