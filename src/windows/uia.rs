//! Windows UI Automation (UIA) text search.
//!
//! Uses the Windows Accessibility tree to find UI elements by name.
//! This is faster and more reliable than OCR for standard UI elements
//! (buttons, labels, menus, etc.).

use super::ocr::TextMatch;

/// Find text in UI elements of the foreground window using UIA.
///
/// Returns matching elements with screen coordinates, or an empty vec if
/// no matches are found. Errors indicate UIA infrastructure failures.
pub fn find_text(_search: &str) -> Result<Vec<TextMatch>, String> {
    // TODO: implement in next commit
    Ok(Vec::new())
}
