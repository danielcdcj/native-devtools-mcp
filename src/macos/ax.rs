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
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut AXUIElementRef,
    ) -> i32;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
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
                        let role = get_string_attribute(element, "AXRole");
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
                            role,
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

/// Get the application name for a PID via NSRunningApplication.
fn app_name_for_pid(pid: i32) -> Option<String> {
    unsafe {
        let app: *mut Object = msg_send![
            Class::get("NSRunningApplication")?,
            runningApplicationWithProcessIdentifier: pid
        ];
        if app.is_null() {
            return None;
        }
        let name_ns: *mut Object = msg_send![app, localizedName];
        if name_ns.is_null() {
            return None;
        }
        let utf8_ptr: *const std::ffi::c_char = msg_send![name_ns, UTF8String];
        if utf8_ptr.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(utf8_ptr)
                .to_string_lossy()
                .into_owned(),
        )
    }
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

/// Resolve app_name to PID by finding the first matching window.
fn pid_for_app_name(app_name: &str) -> Result<i32, String> {
    let windows = super::window::find_windows_by_app(app_name)
        .map_err(|e| format!("Failed to find windows: {}", e))?;
    let win = windows.first().ok_or_else(|| {
        format!(
            "No app found matching '{}'. Use list_apps to find the correct app name.",
            app_name
        )
    })?;
    i32::try_from(win.owner_pid).map_err(|_| format!("PID {} exceeds i32 range", win.owner_pid))
}

/// Get the PID of the process that owns an AX element.
unsafe fn get_pid_for_element(element: AXUIElementRef) -> Option<i32> {
    let mut pid: i32 = 0;
    if AXUIElementGetPid(element, &mut pid) == K_AX_ERROR_SUCCESS {
        Some(pid)
    } else {
        None
    }
}

/// Get the accessibility element at the given screen coordinates.
///
/// Uses `AXUIElementCopyElementAtPosition` to find the deepest element
/// at (x, y). If `app_name` is provided, scopes the lookup to that app;
/// otherwise uses a system-wide lookup.
pub fn element_at_point(
    x: f64,
    y: f64,
    app_name: Option<&str>,
) -> Result<serde_json::Value, String> {
    let root = if let Some(name) = app_name {
        let pid = pid_for_app_name(name)?;
        let el = unsafe { AXUIElementCreateApplication(pid) };
        if el.is_null() {
            return Err(format!("Failed to create AXUIElement for app '{}'", name));
        }
        el
    } else {
        let el = unsafe { AXUIElementCreateSystemWide() };
        if el.is_null() {
            return Err("Failed to create system-wide AXUIElement".to_string());
        }
        el
    };

    let mut element: AXUIElementRef = ptr::null_mut();
    let err = unsafe { AXUIElementCopyElementAtPosition(root, x as f32, y as f32, &mut element) };

    unsafe {
        core_foundation::base::CFRelease(root as core_foundation::base::CFTypeRef);
    }

    if err != K_AX_ERROR_SUCCESS || element.is_null() {
        return Err(format!("No accessibility element found at ({}, {})", x, y));
    }

    // Read attributes from the element
    let name = unsafe { get_string_attribute(element, "AXTitle") };
    let role = unsafe { get_string_attribute(element, "AXRole") };
    let label = unsafe { get_string_attribute(element, "AXDescription") };
    let value = unsafe { get_string_attribute(element, "AXValue") };
    let bounds = unsafe { get_position_and_size(element) };

    // Get PID from the element
    let pid = unsafe { get_pid_for_element(element) };

    unsafe {
        core_foundation::base::CFRelease(element as core_foundation::base::CFTypeRef);
    }

    // Resolve app name from PID
    let resolved_app_name = pid.and_then(app_name_for_pid);

    // Build response, omitting null fields
    let mut result = serde_json::Map::new();

    if let Some(r) = role {
        result.insert("role".to_string(), serde_json::Value::String(r));
    }
    if let Some(n) = name {
        result.insert("name".to_string(), serde_json::Value::String(n));
    }
    if let Some(l) = label {
        result.insert("label".to_string(), serde_json::Value::String(l));
    }
    if let Some(v) = value {
        result.insert("value".to_string(), serde_json::Value::String(v));
    }
    if let Some((pos, size)) = bounds {
        result.insert(
            "bounds".to_string(),
            serde_json::json!({
                "x": pos.x,
                "y": pos.y,
                "width": size.width,
                "height": size.height,
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

    /// Test element_at_point with Calculator.
    /// Requires Calculator to be running and visible.
    /// Run with: cargo test test_ax_element_at_point_calculator -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_ax_element_at_point_calculator() {
        // First find Calculator's "5" button to get known coordinates
        let matches = find_text("5", None).expect("find_text should succeed");
        let button = matches
            .iter()
            .find(|m| m.text == "5")
            .expect("Should find the '5' button");

        // Now query element_at_point at those coordinates
        let result = element_at_point(button.x, button.y, Some("Calculator"))
            .expect("element_at_point should succeed");

        println!(
            "element_at_point result: {}",
            serde_json::to_string_pretty(&result).unwrap()
        );

        // Verify we got a meaningful result
        assert!(result.get("role").is_some(), "Should have a role");
        assert!(result.get("bounds").is_some(), "Should have bounds");
        assert!(result.get("pid").is_some(), "Should have a pid");
        assert!(result.get("app_name").is_some(), "Should have an app_name");
    }

    /// Test element_at_point with system-wide lookup (no app_name).
    /// Requires Calculator to be running and in the foreground.
    /// Run with: cargo test test_ax_element_at_point_system_wide -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_ax_element_at_point_system_wide() {
        let matches = find_text("5", None).expect("find_text should succeed");
        let button = matches
            .iter()
            .find(|m| m.text == "5")
            .expect("Should find the '5' button");

        let result =
            element_at_point(button.x, button.y, None).expect("element_at_point should succeed");

        println!(
            "element_at_point (system-wide): {}",
            serde_json::to_string_pretty(&result).unwrap()
        );

        assert!(result.get("role").is_some(), "Should have a role");
    }
}
