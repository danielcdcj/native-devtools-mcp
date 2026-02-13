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
pub struct LaunchAppParams {
    /// Application name to launch (e.g., "Calculator", "Safari")
    pub app_name: String,
}

pub fn launch_app(params: LaunchAppParams) -> CallToolResult {
    match platform::launch_app(&params.app_name) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Launched '{}'",
            params.app_name
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(e)]),
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

fn focused() -> CallToolResult {
    CallToolResult::success(vec![Content::text("Window focused successfully")])
}

fn error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.into())])
}

pub fn focus_window(params: FocusWindowParams) -> CallToolResult {
    if let Some(app_name) = params.app_name {
        focus_by_app_name(&app_name)
    } else if let Some(pid) = params.pid {
        focus_by_pid(pid)
    } else if let Some(window_id) = params.window_id {
        focus_by_window_id(window_id)
    } else {
        error("Provide one of: window_id, app_name, or pid")
    }
}

fn focus_by_app_name(app_name: &str) -> CallToolResult {
    if platform::activate_app(app_name) {
        return focused();
    }

    // Fallback: the primary activate_app path may not find some apps (e.g. Catalyst/SwiftUI
    // on macOS). Try finding the app via the window list and activating by PID instead.
    let pid = platform::find_windows_by_app(app_name)
        .ok()
        .and_then(|w| w.first().map(|win| win.owner_pid as i32));

    if let Some(pid) = pid {
        if platform::activate_app_by_pid(pid) {
            return focused();
        }
    }

    error(format!(
        "No app found matching '{}'. Use list_apps to find the correct app name.",
        app_name
    ))
}

fn focus_by_pid(pid: i32) -> CallToolResult {
    if platform::activate_app_by_pid(pid) {
        focused()
    } else {
        error(format!(
            "No app found with PID {}. Use list_apps to find running apps.",
            pid
        ))
    }
}

fn focus_by_window_id(window_id: u32) -> CallToolResult {
    match platform::find_window_by_id(window_id) {
        Ok(Some(window)) => {
            if platform::activate_app_by_pid(window.owner_pid as i32) {
                focused()
            } else {
                error(format!(
                    "Found window {} but failed to activate its owning app (PID {}).",
                    window_id, window.owner_pid
                ))
            }
        }
        Ok(None) => error(format!(
            "Window {} not found. Use list_windows to find available windows.",
            window_id
        )),
        Err(e) => error(e),
    }
}
