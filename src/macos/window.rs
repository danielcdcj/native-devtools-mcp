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
    // Use AppleScript to get window info, output as JSON for reliable parsing
    let script = r#"
    use framework "Foundation"
    use scripting additions

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
                        set windowData to "WINDOW:" & procName & "|" & procPID & "|" & winName & "|" & (item 1 of winPos) & "|" & (item 2 of winPos) & "|" & (item 1 of winSize) & "|" & (item 2 of winSize)
                        set end of windowList to windowData
                    end try
                end repeat
            end try
        end repeat
        return windowList
    end tell
    "#;

    let output = Command::new("osascript").arg("-e").arg(script).output();

    let mut windows = Vec::new();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse the AppleScript output
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

    if trimmed.is_empty() {
        return Some(windows);
    }

    // Parse output format: WINDOW:app|pid|name|x|y|w|h, WINDOW:app|pid|name|x|y|w|h, ...
    let mut id_counter = 1u32;

    for part in trimmed.split(", WINDOW:") {
        let item = part.trim_start_matches("WINDOW:");
        if let Some(window) = parse_window_item(item, id_counter) {
            windows.push(window);
            id_counter += 1;
        }
    }

    Some(windows)
}

fn parse_window_item(item: &str, id: u32) -> Option<WindowInfo> {
    let parts: Vec<&str> = item.split('|').collect();
    if parts.len() < 7 {
        return None;
    }

    let owner_name = parts[0].to_string();
    let owner_pid = parts[1].parse().ok()?;
    let name = {
        let n = parts[2].to_string();
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

    let output = Command::new("python3").arg("-c").arg(script).output();

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
        .filter(|w| {
            w.owner_name
                .to_lowercase()
                .contains(&app_name.to_lowercase())
        })
        .collect()
}
