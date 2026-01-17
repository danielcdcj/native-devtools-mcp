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

/// List all visible windows on screen using CGWindowListCopyWindowInfo.
///
/// This function uses the CG API via Python/PyObjC to ensure consistent window IDs
/// (CGWindowNumber) that are compatible with system operations like `screencapture -l`.
///
/// Returns an error if python3 or PyObjC (Quartz module) is not available.
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    list_windows_via_cg()
}

/// Use CGWindowListCopyWindowInfo via Python to get actual CGWindowNumbers
fn list_windows_via_cg() -> Result<Vec<WindowInfo>, String> {
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
        .output()
        .map_err(|e| format!("Failed to execute python3: {}. Is python3 installed?", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("No module named") || stderr.contains("ModuleNotFoundError") {
            return Err(
                "PyObjC (Quartz module) not found. Install with: pip3 install pyobjc-framework-Quartz"
                    .to_string(),
            );
        }
        return Err(format!("python3 script failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Vec<WindowInfo>>(&stdout)
        .map_err(|e| format!("Failed to parse window list: {}", e))
}

/// Find a window by its ID
pub fn find_window_by_id(window_id: u32) -> Result<Option<WindowInfo>, String> {
    Ok(list_windows()?.into_iter().find(|w| w.id == window_id))
}

/// Find windows by application name
pub fn find_windows_by_app(app_name: &str) -> Result<Vec<WindowInfo>, String> {
    Ok(list_windows()?
        .into_iter()
        .filter(|w| {
            w.owner_name
                .to_lowercase()
                .contains(&app_name.to_lowercase())
        })
        .collect())
}
