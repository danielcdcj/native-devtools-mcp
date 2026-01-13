use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub name: Option<String>,
    pub owner_name: String,
    pub owner_pid: i64,
    pub bounds: WindowBounds,
    pub layer: i64,
    pub is_on_screen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// List all visible windows on screen using AppleScript
pub fn list_windows() -> Vec<WindowInfo> {
    // Use AppleScript to get window info as it's more reliable
    let script = r#"
    tell application "System Events"
        set windowList to {}
        repeat with proc in (every process whose background only is false)
            try
                set procName to name of proc
                set procPID to unix id of proc
                repeat with w in windows of proc
                    try
                        set winName to name of w
                        set winPos to position of w
                        set winSize to size of w
                        set end of windowList to {procName, procPID, winName, item 1 of winPos, item 2 of winPos, item 1 of winSize, item 2 of winSize}
                    end try
                end repeat
            end try
        end repeat
        return windowList
    end tell
    "#;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output();

    let mut windows = Vec::new();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse the AppleScript output
            // Format: procName, pid, winName, x, y, width, height, ...
            if let Some(parsed) = parse_applescript_output(&stdout) {
                windows = parsed;
            }
        }
    }

    // Fallback: use CGWindowListCopyWindowInfo via a helper
    if windows.is_empty() {
        windows = list_windows_via_cg();
    }

    windows
}

fn parse_applescript_output(output: &str) -> Option<Vec<WindowInfo>> {
    let mut windows = Vec::new();
    let trimmed = output.trim();

    // AppleScript returns a list like: {{"App", 123, "Window", 0, 0, 800, 600}, ...}
    // We need to parse this format
    if trimmed.is_empty() || trimmed == "{}" {
        return Some(windows);
    }

    // Simple parsing - split by }, { patterns
    let content = trimmed.trim_start_matches('{').trim_end_matches('}');

    let mut id_counter = 1u32;
    let mut current_item = String::new();
    let mut brace_depth = 0;

    for ch in content.chars() {
        match ch {
            '{' => {
                brace_depth += 1;
                if brace_depth > 1 {
                    current_item.push(ch);
                }
            }
            '}' => {
                brace_depth -= 1;
                if brace_depth == 0 {
                    // Parse current item
                    if let Some(window) = parse_window_item(&current_item, id_counter) {
                        windows.push(window);
                        id_counter += 1;
                    }
                    current_item.clear();
                } else {
                    current_item.push(ch);
                }
            }
            ',' if brace_depth == 0 => {
                // Skip commas between items
            }
            _ => {
                if brace_depth > 0 {
                    current_item.push(ch);
                }
            }
        }
    }

    Some(windows)
}

fn parse_window_item(item: &str, id: u32) -> Option<WindowInfo> {
    let parts: Vec<&str> = item.split(',').map(|s| s.trim()).collect();
    if parts.len() < 7 {
        return None;
    }

    let owner_name = parts[0].trim_matches('"').to_string();
    let owner_pid = parts[1].parse().ok()?;
    let name = {
        let n = parts[2].trim_matches('"').to_string();
        if n.is_empty() || n == "missing value" {
            None
        } else {
            Some(n)
        }
    };
    let x = parts[3].parse().ok()?;
    let y = parts[4].parse().ok()?;
    let width = parts[5].parse().ok()?;
    let height = parts[6].parse().ok()?;

    Some(WindowInfo {
        id,
        name,
        owner_name,
        owner_pid,
        bounds: WindowBounds {
            x,
            y,
            width,
            height,
        },
        layer: 0,
        is_on_screen: true,
    })
}

/// Fallback: use CGWindowListCopyWindowInfo via Python
fn list_windows_via_cg() -> Vec<WindowInfo> {
    let script = r#"
import Quartz
import json

windows = Quartz.CGWindowListCopyWindowInfo(
    Quartz.kCGWindowListOptionOnScreenOnly | Quartz.kCGWindowListExcludeDesktopElements,
    Quartz.kCGNullWindowID
)

result = []
for w in windows:
    bounds = w.get('kCGWindowBounds', {})
    result.append({
        'id': w.get('kCGWindowNumber', 0),
        'name': w.get('kCGWindowName'),
        'owner_name': w.get('kCGWindowOwnerName', ''),
        'owner_pid': w.get('kCGWindowOwnerPID', 0),
        'layer': w.get('kCGWindowLayer', 0),
        'is_on_screen': w.get('kCGWindowIsOnscreen', 0) == 1,
        'bounds': {
            'x': bounds.get('X', 0),
            'y': bounds.get('Y', 0),
            'width': bounds.get('Width', 0),
            'height': bounds.get('Height', 0),
        }
    })

print(json.dumps(result))
"#;

    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(windows) = serde_json::from_str::<Vec<WindowInfo>>(&stdout) {
                return windows;
            }
        }
    }

    Vec::new()
}

/// Find a window by its ID
pub fn find_window_by_id(window_id: u32) -> Option<WindowInfo> {
    list_windows().into_iter().find(|w| w.id == window_id)
}

/// Find windows by application name
pub fn find_windows_by_app(app_name: &str) -> Vec<WindowInfo> {
    list_windows()
        .into_iter()
        .filter(|w| w.owner_name.to_lowercase().contains(&app_name.to_lowercase()))
        .collect()
}
