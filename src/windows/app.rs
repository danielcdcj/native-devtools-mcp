//! Application enumeration and focus for Windows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    ShowWindow, GW_OWNER, SW_RESTORE,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>, // Not applicable on Windows, kept for API compatibility
    pub pid: i32,
    pub is_active: bool,
    pub is_hidden: bool,
}

struct AppEnumData {
    apps: HashMap<u32, AppInfo>, // pid -> AppInfo
    foreground_pid: u32,
}

/// List all running applications (processes with visible windows).
///
/// This matches macOS behavior where we list GUI applications rather than
/// all processes. An app is considered running if it has at least one
/// visible top-level window.
pub fn list_apps() -> Vec<AppInfo> {
    // Get the foreground window's PID to determine active app
    let foreground_pid = unsafe {
        let fg = GetForegroundWindow();
        if !fg.is_invalid() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            pid
        } else {
            0
        }
    };

    let mut data = AppEnumData {
        apps: HashMap::new(),
        foreground_pid,
    };

    unsafe {
        let _ = EnumWindows(
            Some(app_enum_callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }

    data.apps.into_values().collect()
}

unsafe extern "system" fn app_enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let data = &mut *(lparam.0 as *mut AppEnumData);

    // Skip invisible windows
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    // Skip windows with no title (not a main window)
    let title_len = GetWindowTextLengthW(hwnd);
    if title_len == 0 {
        return TRUE;
    }

    // Skip windows that have an owner (popup windows, tooltips, etc.)
    if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
        if !owner.is_invalid() {
            return TRUE;
        }
    }

    // Get owner process ID
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    // Skip if we already have this PID
    if data.apps.contains_key(&pid) {
        return TRUE;
    }

    // Get process name
    if let Some(name) = get_process_name(pid) {
        let is_active = pid == data.foreground_pid;

        data.apps.insert(
            pid,
            AppInfo {
                name,
                bundle_id: None, // Windows doesn't have bundle IDs
                pid: pid as i32,
                is_active,
                is_hidden: false, // Windows doesn't have a hidden concept like macOS
            },
        );
    }

    TRUE
}

/// Get the executable name for a process ID.
fn get_process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buf: Vec<u16> = vec![0; 260];
        let mut size = buf.len() as u32;

        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );

        let _ = CloseHandle(handle);

        if result.is_ok() && size > 0 {
            let path = OsString::from_wide(&buf[..size as usize])
                .to_string_lossy()
                .into_owned();
            // Extract just the filename without extension for cleaner display
            path.rsplit('\\')
                .next()
                .map(|s| s.strip_suffix(".exe").unwrap_or(s).to_string())
        } else {
            None
        }
    }
}

/// Activate (focus) an application by name.
///
/// Finds the first window belonging to a process whose name contains
/// the given substring (case-insensitive) and brings it to the foreground.
pub fn activate_app(app_name: &str) -> bool {
    let needle = app_name.to_lowercase();

    struct ActivateData {
        needle: String,
        found: bool,
    }

    let mut data = ActivateData {
        needle,
        found: false,
    };

    unsafe extern "system" fn activate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut ActivateData);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len == 0 {
            return TRUE;
        }

        if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
            if !owner.is_invalid() {
                return TRUE;
            }
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if let Some(name) = get_process_name_static(pid) {
            if name.to_lowercase().contains(&data.needle) {
                focus_hwnd(hwnd);
                data.found = true;
                return BOOL(0); // Stop enumeration
            }
        }

        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(activate_callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }

    data.found
}

// Static version for use in callback
fn get_process_name_static(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buf: Vec<u16> = vec![0; 260];
        let mut size = buf.len() as u32;

        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );

        let _ = CloseHandle(handle);

        if result.is_ok() && size > 0 {
            let path = OsString::from_wide(&buf[..size as usize])
                .to_string_lossy()
                .into_owned();
            path.rsplit('\\')
                .next()
                .map(|s| s.strip_suffix(".exe").unwrap_or(s).to_string())
        } else {
            None
        }
    }
}

/// Activate an application by PID.
///
/// Finds the first visible window belonging to the given process and brings
/// it to the foreground.
pub fn activate_app_by_pid(pid: i32) -> bool {
    struct ActivateByPidData {
        target_pid: u32,
        found: bool,
    }

    let mut data = ActivateByPidData {
        target_pid: pid as u32,
        found: false,
    };

    unsafe extern "system" fn activate_by_pid_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut ActivateByPidData);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len == 0 {
            return TRUE;
        }

        if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
            if !owner.is_invalid() {
                return TRUE;
            }
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if pid == data.target_pid {
            focus_hwnd(hwnd);
            data.found = true;
            return BOOL(0); // Stop enumeration
        }

        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(activate_by_pid_callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }

    data.found
}

/// Focus a window by its handle.
///
/// Uses multiple techniques to bring a window to the foreground since
/// Windows restricts SetForegroundWindow in certain conditions.
fn focus_hwnd(hwnd: HWND) {
    unsafe {
        // Allow this process to set foreground window
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);

        // Restore if minimized
        let _ = ShowWindow(hwnd, SW_RESTORE);

        // Try SetForegroundWindow first
        if !SetForegroundWindow(hwnd).as_bool() {
            // Fallback: bring to top
            let _ = BringWindowToTop(hwnd);
        }
    }
}
