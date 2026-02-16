//! Application enumeration and focus for Windows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER,
    SW_RESTORE, SW_SHOW,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>, // Not applicable on Windows, kept for API compatibility
    pub pid: i32,
    pub is_active: bool,
    pub is_hidden: bool,
    /// Whether this is a regular user-facing app. On Windows, all enumerated
    /// apps have visible windows, so this is always true.
    #[serde(skip)]
    pub is_user_app: bool,
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
                is_user_app: true, // All enumerated apps have visible windows
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

/// Launch an application by name.
///
/// Uses `cmd /c start "" "app_name"` which searches PATH and App Paths registry.
/// For apps not in PATH, provide the full executable path.
pub fn launch_app(app_name: &str) -> Result<(), String> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "start", "", app_name])
        .output()
        .map_err(|e| format!("Failed to run start command: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "Failed to launch '{}': {}",
            app_name,
            stderr.trim()
        ))
    }
}

/// Raise windows by PID. On Windows this is a no-op because `activate_app_by_pid`
/// already uses `BringWindowToTop` + `SetForegroundWindow` which reliably raise windows.
pub fn raise_windows(_pid: i32) -> bool {
    true
}

/// Focus a window by its handle.
///
/// Uses multiple techniques to bring a window to the foreground since
/// Windows restricts SetForegroundWindow in certain conditions.
fn focus_hwnd(hwnd: HWND) {
    unsafe {
        // Get thread IDs for attachment
        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        let current_thread = GetCurrentThreadId();

        // Attach to foreground thread to bypass focus-stealing prevention
        let attached_to_foreground =
            if foreground_thread != current_thread && foreground_thread != 0 {
                AttachThreadInput(current_thread, foreground_thread, true).as_bool()
            } else {
                false
            };

        let attached_to_target =
            if target_thread != current_thread && target_thread != foreground_thread {
                AttachThreadInput(current_thread, target_thread, true).as_bool()
            } else {
                false
            };

        // Restore if minimized
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            // Ensure window is visible
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        // Bring to top and set foreground
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);

        // Detach threads
        if attached_to_target {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        if attached_to_foreground {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
    }
}
