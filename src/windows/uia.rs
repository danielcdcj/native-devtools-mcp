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

/// Find text in UI elements of the foreground window using UIA.
///
/// Searches the accessibility tree of the foreground window for elements
/// whose Name property contains the search string (case-insensitive).
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

        for i in 0..count {
            let elem = match elements.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let name = match elem.CurrentName() {
                Ok(n) => n.to_string(),
                Err(_) => continue,
            };

            if name.is_empty() || !name.to_lowercase().contains(&search_lower) {
                continue;
            }

            let rect = match elem.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => continue,
            };

            let width = (rect.right - rect.left) as f64;
            let height = (rect.bottom - rect.top) as f64;
            if width <= 0.0 || height <= 0.0 {
                continue;
            }

            let bounds = TextBounds {
                x: rect.left as f64,
                y: rect.top as f64,
                width,
                height,
            };

            matches.push(TextMatch {
                text: name,
                x: bounds.x + bounds.width / 2.0,
                y: bounds.y + bounds.height / 2.0,
                confidence: 1.0,
                bounds,
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
