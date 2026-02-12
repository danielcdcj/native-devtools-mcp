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
pub struct ListAppsParams {
    /// Filter by application name (case-insensitive substring match)
    pub app_name: Option<String>,
    /// Only return user-facing apps (excludes system agents, helpers, and daemons)
    pub user_apps_only: Option<bool>,
}

pub fn list_apps(params: ListAppsParams) -> CallToolResult {
    let mut apps = platform::list_apps();

    if let Some(ref name) = params.app_name {
        let needle = name.to_lowercase();
        apps.retain(|app| app.name.to_lowercase().contains(&needle));
    }

    if params.user_apps_only.unwrap_or(false) {
        apps.retain(|app| app.is_user_app);
    }

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
    if let Some(app_name) = params.app_name {
        if platform::activate_app(&app_name) {
            return CallToolResult::success(vec![Content::text("Window focused successfully")]);
        }
        // Fallback: NSWorkspace.runningApplications doesn't list some apps (e.g. Catalyst/SwiftUI).
        // Try finding the app via CGWindowList and activating by PID instead.
        if let Ok(windows) = platform::find_windows_by_app(&app_name) {
            if let Some(window) = windows.first() {
                if platform::activate_app_by_pid(window.owner_pid as i32) {
                    return CallToolResult::success(vec![Content::text(
                        "Window focused successfully",
                    )]);
                }
            }
        }
        CallToolResult::error(vec![Content::text(format!(
            "No app found matching '{}'. Use list_apps to find the correct app name.",
            app_name
        ))])
    } else if let Some(pid) = params.pid {
        if platform::activate_app_by_pid(pid) {
            CallToolResult::success(vec![Content::text("Window focused successfully")])
        } else {
            CallToolResult::error(vec![Content::text(format!(
                "No app found with PID {}. Use list_apps to find running apps.",
                pid
            ))])
        }
    } else if let Some(window_id) = params.window_id {
        // For window_id, we need to find the app that owns it and activate that
        match platform::find_window_by_id(window_id) {
            Ok(Some(window)) => {
                if platform::activate_app_by_pid(window.owner_pid as i32) {
                    CallToolResult::success(vec![Content::text("Window focused successfully")])
                } else {
                    CallToolResult::error(vec![Content::text(format!(
                        "Found window {} but failed to activate its owning app (PID {}).",
                        window_id, window.owner_pid
                    ))])
                }
            }
            Ok(None) => CallToolResult::error(vec![Content::text(format!(
                "Window {} not found. Use list_windows to find available windows.",
                window_id
            ))]),
            Err(e) => CallToolResult::error(vec![Content::text(e)]),
        }
    } else {
        CallToolResult::error(vec![Content::text(
            "Provide one of: window_id, app_name, or pid",
        )])
    }
}
