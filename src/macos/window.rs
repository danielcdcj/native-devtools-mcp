use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    kCGNullWindowID, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer,
    kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowName,
    kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID, CGWindowListCopyWindowInfo,
};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;

type CFDict = CFDictionary<*const c_void, *const c_void>;

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// List all visible windows on screen using CGWindowListCopyWindowInfo.
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let ptr = unsafe { CGWindowListCopyWindowInfo(options, kCGNullWindowID) };
    if ptr.is_null() {
        return Err("CGWindowListCopyWindowInfo failed".to_string());
    }

    let list: CFArray<*const c_void> = unsafe { CFArray::wrap_under_create_rule(ptr) };
    let mut windows = Vec::new();

    for i in 0..list.len() {
        let dict: CFDict =
            unsafe { CFDictionary::wrap_under_get_rule(*list.get_unchecked(i) as *const _) };

        windows.push(WindowInfo {
            id: get_i64(&dict, unsafe { kCGWindowNumber }).unwrap_or(0) as u32,
            name: get_string(&dict, unsafe { kCGWindowName }),
            owner_name: get_string(&dict, unsafe { kCGWindowOwnerName }).unwrap_or_default(),
            owner_pid: get_i64(&dict, unsafe { kCGWindowOwnerPID }).unwrap_or(0),
            layer: get_i64(&dict, unsafe { kCGWindowLayer }).unwrap_or(0),
            is_on_screen: get_i64(&dict, unsafe { kCGWindowIsOnscreen }).unwrap_or(0) != 0,
            bounds: get_bounds(&dict, unsafe { kCGWindowBounds }).unwrap_or_default(),
        });
    }

    Ok(windows)
}

/// Find a window by its ID (enumerates all windows — use `find_window_by_id_direct`
/// on hot paths).
pub fn find_window_by_id(window_id: u32) -> Result<Option<WindowInfo>, String> {
    Ok(list_windows()?.into_iter().find(|w| w.id == window_id))
}

/// Find a window by its ID using a direct single-window query.
///
/// Much faster than `find_window_by_id` — asks the window server for just
/// the one window instead of enumerating all visible windows.
pub fn find_window_by_id_direct(window_id: u32) -> Result<Option<WindowInfo>, String> {
    use core_graphics::window::kCGWindowListOptionIncludingWindow;

    let ptr = unsafe { CGWindowListCopyWindowInfo(kCGWindowListOptionIncludingWindow, window_id) };
    if ptr.is_null() {
        return Err("CGWindowListCopyWindowInfo failed".to_string());
    }

    let list: CFArray<*const c_void> = unsafe { CFArray::wrap_under_create_rule(ptr) };
    if list.is_empty() {
        return Ok(None);
    }

    let dict: CFDict =
        unsafe { CFDictionary::wrap_under_get_rule(*list.get_unchecked(0) as *const _) };

    Ok(Some(WindowInfo {
        id: get_i64(&dict, unsafe { kCGWindowNumber }).unwrap_or(0) as u32,
        name: get_string(&dict, unsafe { kCGWindowName }),
        owner_name: get_string(&dict, unsafe { kCGWindowOwnerName }).unwrap_or_default(),
        owner_pid: get_i64(&dict, unsafe { kCGWindowOwnerPID }).unwrap_or(0),
        layer: get_i64(&dict, unsafe { kCGWindowLayer }).unwrap_or(0),
        is_on_screen: get_i64(&dict, unsafe { kCGWindowIsOnscreen }).unwrap_or(0) != 0,
        bounds: get_bounds(&dict, unsafe { kCGWindowBounds }).unwrap_or_default(),
    }))
}

/// Find windows by application name (case-insensitive substring match).
pub fn find_windows_by_app(app_name: &str) -> Result<Vec<WindowInfo>, String> {
    let needle = app_name.to_lowercase();
    Ok(list_windows()?
        .into_iter()
        .filter(|w| w.owner_name.to_lowercase().contains(&needle))
        .collect())
}

fn get_value(dict: &CFDict, key: *const c_void) -> Option<CFType> {
    dict.find(key)
        .map(|v| unsafe { CFType::wrap_under_get_rule(*v as *const _) })
}

fn get_string(dict: &CFDict, key: *const core_foundation::string::__CFString) -> Option<String> {
    get_value(dict, key as *const c_void)?
        .downcast::<CFString>()
        .map(|s| s.to_string())
}

fn get_i64(dict: &CFDict, key: *const core_foundation::string::__CFString) -> Option<i64> {
    get_value(dict, key as *const c_void)?
        .downcast::<CFNumber>()?
        .to_i64()
}

fn get_bounds(
    dict: &CFDict,
    key: *const core_foundation::string::__CFString,
) -> Option<WindowBounds> {
    let bounds: CFDict =
        unsafe { CFDictionary::wrap_under_get_rule(*dict.find(key as *const c_void)? as *const _) };

    let get_f64 = |k: &str| -> Option<f64> {
        let cf_key = CFString::new(k);
        get_value(&bounds, cf_key.as_concrete_TypeRef() as *const c_void)?
            .downcast::<CFNumber>()?
            .to_f64()
    };

    Some(WindowBounds {
        x: get_f64("X")?,
        y: get_f64("Y")?,
        width: get_f64("Width")?,
        height: get_f64("Height")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_windows() {
        let windows = list_windows().expect("list_windows should succeed");
        assert!(!windows.is_empty(), "Should find at least one window");
        for w in &windows {
            assert!(!w.owner_name.is_empty(), "Window should have owner_name");
        }
    }

    #[test]
    fn test_find_windows_by_app() {
        let windows = find_windows_by_app("Finder").expect("find_windows_by_app should succeed");
        for w in &windows {
            assert!(w.owner_name.to_lowercase().contains("finder"));
        }
    }
}
