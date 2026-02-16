//! macOS Accessibility API (AXUIElement) helpers.
//!
//! Uses the macOS Accessibility tree for:
//! - Text search: find UI elements by name (faster than OCR for standard controls)
//! - Window raising: bring windows to front via AXRaise (works for bundle-less apps)

use super::ocr::{TextBounds, TextMatch};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_graphics::geometry::{CGPoint, CGSize};
use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::ptr;

// AXUIElement opaque type
type AXUIElementRef = *mut c_void;
// AXValue opaque type
type AXValueRef = *mut c_void;

// AXValueType constants
const K_AX_VALUE_TYPE_CGPOINT: u32 = 1;
const K_AX_VALUE_TYPE_CGSIZE: u32 = 2;

// AX error codes
const K_AX_ERROR_SUCCESS: i32 = 0;

const MAX_DEPTH: u32 = 50;
const MAX_ELEMENTS: usize = 10_000;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: core_foundation::string::CFStringRef,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> i32;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> bool;
    fn AXUIElementPerformAction(
        element: AXUIElementRef,
        action: core_foundation::string::CFStringRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: core_foundation::string::CFStringRef,
        value: core_foundation::base::CFTypeRef,
    ) -> i32;
}

/// Find text in UI elements of an app's accessibility tree.
///
/// Searches the accessibility tree for elements whose AXTitle, AXValue, or
/// AXDescription contains the search string (case-insensitive).
/// Returns matching elements with screen coordinates for clicking.
pub fn find_text(search: &str, window_id: Option<u32>) -> Result<Vec<TextMatch>, String> {
    let debug = std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok();

    let pid = match window_id {
        Some(wid) => pid_for_window(wid)?,
        None => frontmost_pid()?,
    };

    if debug {
        eprintln!(
            "[DEBUG ax::find_text] search='{}', window_id={:?}, pid={}",
            search, window_id, pid
        );
    }

    let app_element = unsafe { AXUIElementCreateApplication(pid) };
    if app_element.is_null() {
        return Err(format!("Failed to create AXUIElement for pid {}", pid));
    }

    let search_lower = search.to_lowercase();
    let mut matches = Vec::new();
    let mut element_count: usize = 0;

    unsafe {
        walk_ax_tree(app_element, &mut element_count, 0, &mut |element| {
            let matched_text = ["AXTitle", "AXValue", "AXDescription"]
                .iter()
                .filter_map(|attr| get_string_attribute(element, attr))
                .find(|s| !s.is_empty() && s.to_lowercase().contains(search_lower.as_str()));

            if let Some(text) = matched_text {
                if let Some((position, size)) = get_position_and_size(element) {
                    if size.width > 0.0 && size.height > 0.0 {
                        let bounds = TextBounds {
                            x: position.x,
                            y: position.y,
                            width: size.width,
                            height: size.height,
                        };
                        matches.push(TextMatch {
                            text,
                            x: bounds.x + bounds.width / 2.0,
                            y: bounds.y + bounds.height / 2.0,
                            confidence: 1.0,
                            bounds,
                        });
                    }
                }
            }
        });
        core_foundation::base::CFRelease(app_element as core_foundation::base::CFTypeRef);
    }

    if debug {
        eprintln!(
            "[DEBUG ax::find_text] found {} matches out of {} elements",
            matches.len(),
            element_count
        );
    }

    Ok(matches)
}

/// Recursively walk the AX element tree and call `visitor` on each element.
///
/// `depth` limits recursion to prevent runaway traversal of deep trees.
unsafe fn walk_ax_tree(
    element: AXUIElementRef,
    element_count: &mut usize,
    depth: u32,
    visitor: &mut dyn FnMut(AXUIElementRef),
) {
    // Guard against excessively deep or large trees
    if depth > MAX_DEPTH || *element_count >= MAX_ELEMENTS {
        return;
    }

    *element_count += 1;
    visitor(element);

    // Recurse into children
    let children_attr = CFString::new("AXChildren");
    let mut children_ref: core_foundation::base::CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(
        element,
        children_attr.as_concrete_TypeRef(),
        &mut children_ref,
    );

    if err != K_AX_ERROR_SUCCESS || children_ref.is_null() {
        return;
    }

    // children_ref is a CFArray of AXUIElementRef
    let children: CFArray<*const c_void> = CFArray::wrap_under_create_rule(children_ref as _);

    for i in 0..children.len() {
        let child = *children.get_unchecked(i) as AXUIElementRef;
        // Retain the child for the duration of our walk since CFArray only gives a get-rule ref
        core_foundation::base::CFRetain(child as core_foundation::base::CFTypeRef);
        walk_ax_tree(child, element_count, depth + 1, visitor);
        core_foundation::base::CFRelease(child as core_foundation::base::CFTypeRef);
    }
}

/// Get a string attribute from an AX element. Returns None if the attribute
/// doesn't exist or isn't a string.
unsafe fn get_string_attribute(element: AXUIElementRef, attr_name: &str) -> Option<String> {
    let attr = CFString::new(attr_name);
    let mut value_ref: core_foundation::base::CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value_ref);

    if err != K_AX_ERROR_SUCCESS || value_ref.is_null() {
        return None;
    }

    // value_ref is owned (create rule) — wrap it so it gets released.
    // downcast_into consumes cf_value, avoiding an extra retain/release cycle.
    let cf_string = CFType::wrap_under_create_rule(value_ref).downcast_into::<CFString>()?;
    Some(cf_string.to_string())
}

/// Get position (CGPoint) and size (CGSize) of an AX element.
/// Returns None if either attribute is missing.
unsafe fn get_position_and_size(element: AXUIElementRef) -> Option<(CGPoint, CGSize)> {
    let position: CGPoint = get_ax_value(element, "AXPosition", K_AX_VALUE_TYPE_CGPOINT)?;
    let size: CGSize = get_ax_value(element, "AXSize", K_AX_VALUE_TYPE_CGSIZE)?;
    Some((position, size))
}

/// Extract a typed value (CGPoint or CGSize) from an AXValue attribute.
unsafe fn get_ax_value<T: Default>(
    element: AXUIElementRef,
    attr_name: &str,
    ax_value_type: u32,
) -> Option<T> {
    let attr = CFString::new(attr_name);
    let mut value_ref: core_foundation::base::CFTypeRef = ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value_ref);

    if err != K_AX_ERROR_SUCCESS || value_ref.is_null() {
        return None;
    }

    let mut result = T::default();
    let ok = AXValueGetValue(
        value_ref as AXValueRef,
        ax_value_type,
        &mut result as *mut T as *mut c_void,
    );

    core_foundation::base::CFRelease(value_ref);

    if ok {
        Some(result)
    } else {
        None
    }
}

/// Get the PID that owns a given window ID, using CGWindowListCopyWindowInfo.
fn pid_for_window(window_id: u32) -> Result<i32, String> {
    let window = super::window::find_window_by_id(window_id)?
        .ok_or_else(|| format!("Window {} not found", window_id))?;
    i32::try_from(window.owner_pid)
        .map_err(|_| format!("PID {} exceeds i32 range", window.owner_pid))
}

/// Get the PID of the frontmost application via NSWorkspace.
fn frontmost_pid() -> Result<i32, String> {
    unsafe {
        let cls = Class::get("NSWorkspace").ok_or("NSWorkspace class not available")?;
        let workspace: *mut Object = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return Err("NSWorkspace.sharedWorkspace returned nil".to_string());
        }
        let app: *mut Object = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return Err("No frontmost application found".to_string());
        }
        let pid: i32 = msg_send![app, processIdentifier];
        Ok(pid)
    }
}

/// Collect all unique non-empty element names from the accessibility tree.
/// Used to provide a list of available elements when a search returns no matches.
pub fn list_element_names(window_id: Option<u32>) -> Result<Vec<String>, String> {
    let pid = match window_id {
        Some(wid) => pid_for_window(wid)?,
        None => frontmost_pid()?,
    };

    let app_element = unsafe { AXUIElementCreateApplication(pid) };
    if app_element.is_null() {
        return Err(format!("Failed to create AXUIElement for pid {}", pid));
    }

    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut element_count: usize = 0;

    unsafe {
        walk_ax_tree(app_element, &mut element_count, 0, &mut |element| {
            for attr in &["AXTitle", "AXValue", "AXDescription"] {
                if let Some(text) = get_string_attribute(element, attr) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                        names.push(trimmed.to_string());
                    }
                }
            }
        });
        core_foundation::base::CFRelease(app_element as core_foundation::base::CFTypeRef);
    }

    Ok(names)
}

/// Raise all windows of an application to the front using the Accessibility API.
///
/// Two-step approach:
/// 1. Set AXFrontmost on the app element (equivalent to System Events `set frontmost`)
/// 2. AXRaise on each window (physically brings windows to front)
///
/// Step 1 is critical for apps without a proper macOS app bundle (e.g. Tauri dev builds)
/// where NSRunningApplication.activate reports success but doesn't bring windows to front.
pub fn raise_windows(pid: i32) -> bool {
    let debug = std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok();

    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        if app_element.is_null() {
            if debug {
                eprintln!(
                    "[DEBUG ax::raise_windows] Failed to create AXUIElement for pid {}",
                    pid
                );
            }
            return false;
        }

        // Step 1: Set AXFrontmost on the app element to make it the frontmost process.
        // This is the programmatic equivalent of AppleScript:
        //   tell application "System Events" to set frontmost of process "X" to true
        let frontmost_attr = CFString::new("AXFrontmost");
        let frontmost_err = AXUIElementSetAttributeValue(
            app_element,
            frontmost_attr.as_concrete_TypeRef(),
            core_foundation::boolean::CFBoolean::true_value().as_CFTypeRef(),
        );
        if debug {
            eprintln!(
                "[DEBUG ax::raise_windows] AXFrontmost set for pid {} (err={})",
                pid, frontmost_err
            );
        }

        // Step 2: AXRaise each window to bring them to front in the window order.
        let windows_attr = CFString::new("AXWindows");
        let mut windows_ref: core_foundation::base::CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(
            app_element,
            windows_attr.as_concrete_TypeRef(),
            &mut windows_ref,
        );

        let mut raised = frontmost_err == K_AX_ERROR_SUCCESS;

        if err == K_AX_ERROR_SUCCESS && !windows_ref.is_null() {
            let windows: CFArray<*const c_void> = CFArray::wrap_under_create_rule(windows_ref as _);
            let raise_action = CFString::new("AXRaise");

            for i in 0..windows.len() {
                let window = *windows.get_unchecked(i) as AXUIElementRef;
                let result = AXUIElementPerformAction(window, raise_action.as_concrete_TypeRef());
                if result == K_AX_ERROR_SUCCESS {
                    raised = true;
                } else if debug {
                    eprintln!(
                        "[DEBUG ax::raise_windows] AXRaise failed for window {} (err={})",
                        i, result
                    );
                }
            }

            if debug {
                eprintln!(
                    "[DEBUG ax::raise_windows] pid={}, windows={}, raised={}",
                    pid,
                    windows.len(),
                    raised
                );
            }
        } else if debug {
            eprintln!(
                "[DEBUG ax::raise_windows] No AXWindows for pid {} (err={})",
                pid, err
            );
        }

        core_foundation::base::CFRelease(app_element as core_foundation::base::CFTypeRef);
        raised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Search for "9" in the frontmost app (Calculator).
    /// Requires Calculator to be running and in the foreground.
    /// Run with: cargo test test_ax_find_text_calculator -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_ax_find_text_calculator() {
        let results = find_text("9", None).expect("find_text should succeed");
        println!("AX find_text results for '9' (no window_id):");
        for m in &results {
            println!(
                "  '{}' at ({:.1}, {:.1}) bounds=({:.1}, {:.1}, {:.1}x{:.1})",
                m.text, m.x, m.y, m.bounds.x, m.bounds.y, m.bounds.width, m.bounds.height
            );
        }
        assert!(
            !results.is_empty(),
            "Should find at least one match for '9'"
        );
        for m in &results {
            assert!(m.x > 0.0, "x coordinate should be positive");
            assert!(m.y > 0.0, "y coordinate should be positive");
        }
    }

    /// Search for "9" using Calculator's window_id.
    /// Requires Calculator to be running.
    /// Run with: cargo test test_ax_find_text_with_window_id -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_ax_find_text_with_window_id() {
        let windows = crate::macos::window::find_windows_by_app("Calculator")
            .expect("find_windows_by_app should succeed");
        let calc_window = windows
            .first()
            .expect("Calculator must be running for this test");

        println!("Calculator window id: {}", calc_window.id);

        let results =
            find_text("9", Some(calc_window.id)).expect("find_text with window_id should succeed");
        println!(
            "AX find_text results for '9' (window_id={}):",
            calc_window.id
        );
        for m in &results {
            println!(
                "  '{}' at ({:.1}, {:.1}) bounds=({:.1}, {:.1}, {:.1}x{:.1})",
                m.text, m.x, m.y, m.bounds.x, m.bounds.y, m.bounds.width, m.bounds.height
            );
        }
        assert!(
            !results.is_empty(),
            "Should find at least one match for '9' with window_id"
        );
    }
}
