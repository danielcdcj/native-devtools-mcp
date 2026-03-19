//! Windows UI Automation (UIA) text search.
//!
//! Uses the Windows Accessibility tree to find UI elements by name.
//! This is faster and more reliable than OCR for standard UI elements
//! (buttons, labels, menus, etc.).

use super::ocr::{TextBounds, TextMatch};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope, TreeScope_Descendants, TreeScope_Element,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Enumerate all UIA elements in the foreground window, calling `visitor` on each
/// element's name (non-empty names only). Returns early with an empty result if
/// no foreground window is available.
fn for_each_element_name(mut visitor: impl FnMut(&str)) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create IUIAutomation: {}", e))?;

        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(());
        }

        let root = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| format!("Failed to get element from foreground window: {}", e))?;

        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| format!("Failed to create condition: {}", e))?;

        let scope = TreeScope(TreeScope_Element.0 | TreeScope_Descendants.0);
        let elements = root
            .FindAll(scope, &condition)
            .map_err(|e| format!("FindAll failed: {}", e))?;

        let count = elements
            .Length()
            .map_err(|e| format!("Failed to get element count: {}", e))?;

        for i in 0..count {
            let elem = match elements.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let name = match elem.CurrentName() {
                Ok(n) => n.to_string(),
                Err(_) => continue,
            };

            if !name.is_empty() {
                visitor(&name);
            }
        }

        Ok(())
    }
}

/// Check if any of the element's text properties contain the search string (case-insensitive).
/// Returns the first matching text, or None. Checks name, then value, then help text.
fn match_element_text(
    name: Option<&str>,
    value: Option<&str>,
    help: Option<&str>,
    search_lower: &str,
) -> Option<String> {
    [name, value, help]
        .into_iter()
        .flatten()
        .find(|text| text.to_lowercase().contains(search_lower))
        .map(|s| s.to_string())
}

/// Find text in UI elements of the foreground window using UIA.
///
/// Searches the accessibility tree of the foreground window for elements
/// whose Name, Value, or HelpText property contains the search string (case-insensitive).
/// Returns matching elements with screen coordinates for clicking.
pub fn find_text(search: &str) -> Result<Vec<TextMatch>, String> {
    let debug = std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok();
    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create IUIAutomation: {}", e))?;

        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(Vec::new());
        }

        let root = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| format!("Failed to get element from foreground window: {}", e))?;

        if debug {
            let name = root.CurrentName().unwrap_or_default();
            eprintln!(
                "[DEBUG uia::find_text] search='{}', foreground_hwnd={:?}, window_name='{}'",
                search, hwnd, name
            );
        }

        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| format!("Failed to create condition: {}", e))?;

        let scope = TreeScope(TreeScope_Element.0 | TreeScope_Descendants.0);
        let elements = root
            .FindAll(scope, &condition)
            .map_err(|e| format!("FindAll failed: {}", e))?;

        let count = elements
            .Length()
            .map_err(|e| format!("Failed to get element count: {}", e))?;

        if debug {
            eprintln!("[DEBUG uia::find_text] scanning {} elements", count);
        }

        let mut seen_centers: Vec<(f64, f64)> = Vec::new();

        for i in 0..count {
            let elem = match elements.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Get bounding rectangle first; skip zero-size elements.
            let rect = match elem.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => continue,
            };

            let width = (rect.right - rect.left) as f64;
            let height = (rect.bottom - rect.top) as f64;
            if width <= 0.0 || height <= 0.0 {
                continue;
            }

            // Collect the three text properties.
            let name = elem
                .CurrentName()
                .ok()
                .map(|n| n.to_string())
                .filter(|n| !n.is_empty());

            let value = elem
                .GetCurrentPropertyValue(
                    windows::Win32::UI::Accessibility::UIA_ValueValuePropertyId,
                )
                .ok()
                .and_then(|v| {
                    let s = v.to_string();
                    if s.is_empty() { None } else { Some(s) }
                });

            let help = elem
                .CurrentHelpText()
                .ok()
                .map(|h| h.to_string())
                .filter(|h| !h.is_empty());

            // Find the first property that matches the search string.
            let matched_text = match_element_text(
                name.as_deref(),
                value.as_deref(),
                help.as_deref(),
                &search_lower,
            );

            let matched_text = match matched_text {
                Some(t) => t,
                None => continue,
            };

            let cx = rect.left as f64 + width / 2.0;
            let cy = rect.top as f64 + height / 2.0;

            // Deduplicate by center coordinates within 2px tolerance.
            if seen_centers
                .iter()
                .any(|(sx, sy)| (sx - cx).abs() < 2.0 && (sy - cy).abs() < 2.0)
            {
                continue;
            }
            seen_centers.push((cx, cy));

            let role = elem
                .CurrentControlType()
                .ok()
                .and_then(|ct| uia_control_type_name(ct.0));

            let bounds = TextBounds {
                x: rect.left as f64,
                y: rect.top as f64,
                width,
                height,
            };

            matches.push(TextMatch {
                text: matched_text,
                x: cx,
                y: cy,
                confidence: 1.0,
                bounds,
                role,
            });
        }

        if debug {
            eprintln!(
                "[DEBUG uia::find_text] found {} matches out of {} elements",
                matches.len(),
                count
            );
        }
    }

    Ok(matches)
}

/// Collect all unique non-empty element names from the UIA tree of the foreground window.
/// Used to provide a list of available elements when a search returns no matches.
pub fn list_element_names() -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for_each_element_name(|name| {
        let trimmed = name.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            names.push(trimmed.to_string());
        }
    })?;

    Ok(names)
}

/// Get the UI Automation element at the given screen coordinates.
///
/// Uses `IUIAutomation::ElementFromPoint` to find the element at (x, y).
/// Returns a JSON object with the element's attributes.
pub fn element_at_point(x: f64, y: f64) -> Result<serde_json::Value, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create IUIAutomation: {}", e))?;

        let point = windows::Win32::Foundation::POINT {
            x: x as i32,
            y: y as i32,
        };

        let elem = automation
            .ElementFromPoint(point)
            .map_err(|e| format!("No accessibility element found at ({}, {}): {}", x, y, e))?;

        let name = elem
            .CurrentName()
            .ok()
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty());
        let role = elem
            .CurrentControlType()
            .ok()
            .and_then(|ct| uia_control_type_name(ct.0));
        let help = elem
            .CurrentHelpText()
            .ok()
            .map(|h| h.to_string())
            .filter(|h| !h.is_empty());
        let value_pattern = elem
            .GetCurrentPropertyValue(windows::Win32::UI::Accessibility::UIA_ValueValuePropertyId)
            .ok()
            .and_then(|v| {
                let s = v.to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            });

        let rect = elem.CurrentBoundingRectangle().ok();
        let pid = elem.CurrentProcessId().ok();

        // Resolve app name from PID
        let resolved_app_name = pid.and_then(|p| {
            use windows::Win32::System::ProcessStatus::GetProcessImageFileNameW;
            use windows::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            };

            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, p as u32).ok()?;
            let mut buf = [0u16; 260];
            let len = GetProcessImageFileNameW(handle, &mut buf);
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            if len == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit('\\')
                .next()
                .map(|s| s.trim_end_matches(".exe").to_string())
        });

        let mut result = serde_json::Map::new();

        if let Some(r) = role {
            result.insert("role".to_string(), serde_json::Value::String(r));
        }
        if let Some(n) = name {
            result.insert("name".to_string(), serde_json::Value::String(n));
        }
        if let Some(l) = help {
            result.insert("label".to_string(), serde_json::Value::String(l));
        }
        if let Some(v) = value_pattern {
            result.insert("value".to_string(), serde_json::Value::String(v));
        }
        if let Some(r) = rect {
            let width = (r.right - r.left) as f64;
            let height = (r.bottom - r.top) as f64;
            result.insert(
                "bounds".to_string(),
                serde_json::json!({
                    "x": r.left as f64,
                    "y": r.top as f64,
                    "width": width,
                    "height": height,
                }),
            );
        }
        if let Some(p) = pid {
            result.insert("pid".to_string(), serde_json::Value::Number(p.into()));
        }
        if let Some(a) = resolved_app_name {
            result.insert("app_name".to_string(), serde_json::Value::String(a));
        }

        Ok(serde_json::Value::Object(result))
    }
}

/// Map a UIA_*ControlTypeId to a human-readable name.
fn uia_control_type_name(id: i32) -> Option<String> {
    let name = match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        _ => return None,
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_element_text_matches_name() {
        let result = match_element_text(Some("Save"), None, None, "save");
        assert_eq!(result, Some("Save".to_string()));
    }

    #[test]
    fn test_match_element_text_matches_value() {
        let result = match_element_text(None, Some("hello world"), None, "hello");
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_match_element_text_matches_help() {
        let result = match_element_text(None, None, Some("Click to submit"), "submit");
        assert_eq!(result, Some("Click to submit".to_string()));
    }

    #[test]
    fn test_match_element_text_prefers_name_over_value() {
        let result = match_element_text(Some("Save"), Some("Save file"), None, "save");
        assert_eq!(result, Some("Save".to_string()));
    }

    #[test]
    fn test_match_element_text_no_match() {
        let result = match_element_text(Some("Open"), Some("file.txt"), Some("Opens a file"), "save");
        assert_eq!(result, None);
    }

    #[test]
    fn test_match_element_text_case_insensitive() {
        let result = match_element_text(Some("SAVE"), None, None, "save");
        assert_eq!(result, Some("SAVE".to_string()));
    }

    #[test]
    fn test_find_text_returns_empty_for_no_match() {
        let result = find_text("some_unlikely_text_xyz_987654");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
