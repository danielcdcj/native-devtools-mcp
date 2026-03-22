//! Windows UI Automation (UIA) helpers.
//!
//! Uses the Windows Accessibility tree for:
//! - Text search: find UI elements by name (faster than OCR for standard controls)
//! - Snapshot: collect the full UIA tree as a flat list of snapshot nodes

use super::ocr::{TextBounds, TextMatch};
use crate::tools::ax_snapshot::{map_uia_control_type, AXSnapshotNode};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker, TreeScope,
    TreeScope_Descendants, TreeScope_Element,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

const MAX_DEPTH: u32 = 50;
const MAX_ELEMENTS: usize = 10_000;

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

        let mut seen_centers: std::collections::HashSet<(i32, i32)> =
            std::collections::HashSet::new();

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
            let (name, value, help) = element_text_properties(&elem);

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

            // Deduplicate by center coordinates (quantized to 2px grid).
            let key = ((cx / 2.0) as i32, (cy / 2.0) as i32);
            if !seen_centers.insert(key) {
                continue;
            }

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

/// Container control types that warrant a descendant search for a more specific element.
const CONTAINER_TYPES: &[i32] = &[
    50032, // Window
    50033, // Pane
    50026, // Group
    50014, // ScrollBar
];

/// Get the UI Automation element at the given screen coordinates.
///
/// Uses `IUIAutomation::ElementFromPoint` to find the element at (x, y).
/// When `app_name` is provided, verifies the element belongs to that app by PID;
/// if not, walks descendants filtered by PID.
/// When the result is a container type (Window, Pane, Group, ScrollBar), walks
/// descendants to find the smallest-area element containing the point.
/// Returns a JSON object with the element's attributes.
pub fn element_at_point(
    x: f64,
    y: f64,
    app_name: Option<&str>,
) -> Result<serde_json::Value, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create IUIAutomation: {}", e))?;

        let point = windows::Win32::Foundation::POINT {
            x: x as i32,
            y: y as i32,
        };

        let mut elem = automation
            .ElementFromPoint(point)
            .map_err(|e| format!("No accessibility element found at ({}, {}): {}", x, y, e))?;

        // Step 1: App-name scoping — verify the element belongs to the target app.
        if let Some(name) = app_name {
            let target_pids = resolve_app_pids(name);
            if !target_pids.is_empty() {
                let elem_pid = elem.CurrentProcessId().unwrap_or(0);
                if !target_pids.contains(&elem_pid) {
                    // Element doesn't belong to target app — walk descendants to find one that does.
                    if let Some(scoped) =
                        find_smallest_element_at_point(&automation, &elem, x, y, Some(&target_pids))
                    {
                        elem = scoped;
                    }
                }
            }
        }

        // Step 2: Container fallback — if the element is a container, find a more specific child.
        let control_type = elem.CurrentControlType().map(|ct| ct.0).unwrap_or(0);
        if CONTAINER_TYPES.contains(&control_type) {
            if let Some(deeper) = find_smallest_element_at_point(&automation, &elem, x, y, None) {
                elem = deeper;
            }
        }

        build_element_json(&elem)
    }
}

/// Resolve an app name to a list of PIDs by matching against running applications.
fn resolve_app_pids(app_name: &str) -> Vec<i32> {
    let needle = app_name.to_lowercase();
    crate::windows::app::list_apps()
        .into_iter()
        .filter(|app| app.name.to_lowercase().contains(&needle))
        .map(|app| app.pid)
        .collect()
}

/// Search descendants of `root` for the smallest-area element containing the point (x, y).
/// When `pid_filter` is `Some`, only elements belonging to one of the listed PIDs are considered.
unsafe fn find_smallest_element_at_point(
    automation: &IUIAutomation,
    root: &IUIAutomationElement,
    x: f64,
    y: f64,
    pid_filter: Option<&[i32]>,
) -> Option<IUIAutomationElement> {
    let condition = automation.CreateTrueCondition().ok()?;
    let scope = TreeScope(TreeScope_Descendants.0);
    let elements = root.FindAll(scope, &condition).ok()?;
    let count = elements.Length().ok()?;

    let mut best: Option<(IUIAutomationElement, f64)> = None;

    for i in 0..count {
        let child = match elements.GetElement(i) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if let Some(pids) = pid_filter {
            let child_pid = child.CurrentProcessId().unwrap_or(0);
            if !pids.contains(&child_pid) {
                continue;
            }
        }

        if let Some(area) = check_element_contains_point(&child, x, y) {
            if best
                .as_ref()
                .map_or(true, |(_, best_area)| area < *best_area)
            {
                best = Some((child, area));
            }
        }
    }

    best.map(|(elem, _)| elem)
}

/// Check if an element's bounding rectangle contains the point (x, y).
/// Returns the area of the bounding rectangle if it does, or None if it doesn't.
unsafe fn check_element_contains_point(elem: &IUIAutomationElement, x: f64, y: f64) -> Option<f64> {
    let rect = elem.CurrentBoundingRectangle().ok()?;
    let left = rect.left as f64;
    let top = rect.top as f64;
    let right = rect.right as f64;
    let bottom = rect.bottom as f64;

    let width = right - left;
    let height = bottom - top;

    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    if x >= left && x <= right && y >= top && y <= bottom {
        Some(width * height)
    } else {
        None
    }
}

/// Extract the three text properties (name, value, help) from a UIA element.
unsafe fn element_text_properties(
    elem: &IUIAutomationElement,
) -> (Option<String>, Option<String>, Option<String>) {
    let name = elem
        .CurrentName()
        .ok()
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty());
    let value = elem
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
    let help = elem
        .CurrentHelpText()
        .ok()
        .map(|h| h.to_string())
        .filter(|h| !h.is_empty());
    (name, value, help)
}

/// Build a JSON object from a UIA element's properties.
unsafe fn build_element_json(elem: &IUIAutomationElement) -> Result<serde_json::Value, String> {
    let (name, value_pattern, help) = element_text_properties(elem);
    let role = elem
        .CurrentControlType()
        .ok()
        .and_then(|ct| uia_control_type_name(ct.0));

    let rect = elem.CurrentBoundingRectangle().ok();
    let pid = elem.CurrentProcessId().ok();

    let resolved_app_name = pid.and_then(|p| crate::windows::app::get_process_name(p as u32));

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
        50039 => "SemanticZoom",
        50040 => "AppBar",
        _ => return None,
    };
    Some(name.to_string())
}

/// Walk the UIA tree of an application and return a flat, depth-first
/// snapshot of all elements as [`AXSnapshotNode`] values.
///
/// If `app_name` is `Some`, the tree is rooted at that application's window;
/// otherwise the foreground window is used.
pub fn collect_uia_tree(app_name: Option<&str>) -> Result<Vec<AXSnapshotNode>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("Failed to create IUIAutomation: {}", e))?;

        let root = uia_root_element(&automation, app_name)?;

        let walker = automation
            .ControlViewWalker()
            .map_err(|e| format!("Failed to create ControlViewWalker: {}", e))?;

        let mut nodes = Vec::new();
        let mut element_count: usize = 0;
        let mut next_uid: u32 = 1;

        collect_uia_tree_recursive(
            &walker,
            &root,
            &mut element_count,
            0,
            &mut next_uid,
            &mut nodes,
        );

        Ok(nodes)
    }
}

/// Resolve the root UIA element for tree collection.
///
/// If `app_name` is provided, finds the first matching window by app name.
/// Otherwise uses the foreground window.
unsafe fn uia_root_element(
    automation: &IUIAutomation,
    app_name: Option<&str>,
) -> Result<IUIAutomationElement, String> {
    let hwnd = match app_name {
        Some(name) => {
            let windows = super::window::find_windows_by_app(name)?;
            let win = windows.first().ok_or_else(|| {
                format!(
                    "No app found matching '{}'. Use list_apps to find the correct app name.",
                    name
                )
            })?;
            super::window::hwnd_from_id(win.id)
        }
        None => {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return Err("No foreground window available".to_string());
            }
            hwnd
        }
    };

    automation
        .ElementFromHandle(hwnd)
        .map_err(|e| format!("Failed to get UIA element from window: {}", e))
}

/// Recursively walk the UIA control view and collect [`AXSnapshotNode`] entries.
///
/// Uses `IUIAutomationTreeWalker` to traverse children depth-first.
/// UIDs are assigned sequentially via `next_uid` (starts at 1).
unsafe fn collect_uia_tree_recursive(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    element_count: &mut usize,
    depth: u32,
    next_uid: &mut u32,
    nodes: &mut Vec<AXSnapshotNode>,
) {
    if depth > MAX_DEPTH || *element_count >= MAX_ELEMENTS {
        return;
    }

    *element_count += 1;

    let uid = *next_uid;
    *next_uid += 1;

    let role = element
        .CurrentControlType()
        .map(|ct| map_uia_control_type(ct.0))
        .unwrap_or_else(|_| "unknown".to_string());

    let name = element
        .CurrentName()
        .ok()
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty());

    let value = element
        .GetCurrentPropertyValue(
            windows::Win32::UI::Accessibility::UIA_ValueValuePropertyId,
        )
        .ok()
        .and_then(|v| {
            let s = v.to_string();
            if s.is_empty() { None } else { Some(s) }
        });

    let focused = element.CurrentHasKeyboardFocus().unwrap_or_default().as_bool();

    let disabled = element
        .CurrentIsEnabled()
        .map(|b| !b.as_bool())
        .unwrap_or(false);

    // ExpandCollapseState: 0=Collapsed, 1=Expanded, 2=PartiallyExpanded, 3=LeafNode
    let expanded = element
        .GetCurrentPropertyValue(
            windows::Win32::UI::Accessibility::UIA_ExpandCollapseExpandCollapseStatePropertyId,
        )
        .ok()
        .and_then(|v| {
            let state: i32 = (&v).try_into().ok()?;
            // Only report expanded/collapsed, not leaf nodes
            if state == 3 { None } else { Some(state == 1 || state == 2) }
        });

    let selected = element
        .GetCurrentPropertyValue(
            windows::Win32::UI::Accessibility::UIA_SelectionItemIsSelectedPropertyId,
        )
        .ok()
        .and_then(|v| {
            let b: bool = (&v).try_into().ok()?;
            Some(b)
        });

    nodes.push(AXSnapshotNode {
        uid,
        role,
        name,
        value,
        focused,
        disabled,
        expanded,
        selected,
        depth,
    });

    // Walk children via the tree walker
    let mut child = match walker.GetFirstChildElement(element) {
        Ok(c) => c,
        Err(_) => return,
    };

    loop {
        collect_uia_tree_recursive(walker, &child, element_count, depth + 1, next_uid, nodes);

        child = match walker.GetNextSiblingElement(&child) {
            Ok(next) => next,
            Err(_) => break,
        };
    }
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
        let result =
            match_element_text(Some("Open"), Some("file.txt"), Some("Opens a file"), "save");
        assert_eq!(result, None);
    }

    #[test]
    fn test_match_element_text_case_insensitive() {
        let result = match_element_text(Some("SAVE"), None, None, "save");
        assert_eq!(result, Some("SAVE".to_string()));
    }

    #[test]
    fn test_find_text_returns_empty_for_no_match() {
        // May fail in headless/no-foreground-window environments — that's OK
        if let Ok(result) = find_text("some_unlikely_text_xyz_987654") {
            assert!(result.is_empty());
        }
    }

    #[test]
    fn test_element_at_point_returns_json() {
        // May fail in headless/no-foreground-window environments — that's OK
        if let Ok(value) = element_at_point(100.0, 10.0, None) {
            assert!(value.get("role").is_some() || value.get("name").is_some());
        }
    }

    #[test]
    fn test_element_at_point_with_nonexistent_app() {
        // With a nonexistent app name, resolve_app_pids returns empty so scoping is
        // skipped. The call should not panic; it may return Ok (element at point) or
        // Err (no element / COM threading issue in parallel tests).
        let result = element_at_point(100.0, 10.0, Some("nonexistent_app_xyz"));
        // If it succeeds, the result should still have role or name from the element at point.
        if let Ok(value) = result {
            assert!(value.get("role").is_some() || value.get("name").is_some());
        }
    }

    #[test]
    fn test_collect_uia_tree_returns_nodes() {
        // Uses the foreground window. May fail in headless environments — that's OK.
        if let Ok(nodes) = collect_uia_tree(None) {
            assert!(!nodes.is_empty(), "Should find at least one node");
            // First node should be uid=1 at depth 0
            assert_eq!(nodes[0].uid, 1);
            assert_eq!(nodes[0].depth, 0);
            // UIDs should be sequential
            for (i, node) in nodes.iter().enumerate() {
                assert_eq!(node.uid, (i + 1) as u32);
            }
        }
    }

    #[test]
    fn test_collect_uia_tree_nonexistent_app() {
        let result = collect_uia_tree(Some("nonexistent_app_xyz_987654"));
        assert!(result.is_err());
    }
}
