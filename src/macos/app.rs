use cocoa::base::{id, nil};
use objc::{class, msg_send, sel, sel_impl};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>,
    pub pid: i32,
    pub is_active: bool,
    pub is_hidden: bool,
    /// Whether this is a regular user-facing app (appears in Dock).
    /// Background agents and accessory apps are not user-facing.
    #[serde(skip)]
    pub is_user_app: bool,
}

/// Returns all NSRunningApplication handles from NSWorkspace.
unsafe fn get_running_apps() -> Vec<id> {
    let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
    let running_apps: id = msg_send![workspace, runningApplications];
    let count: usize = msg_send![running_apps, count];

    (0..count)
        .map(|i| {
            let app: id = msg_send![running_apps, objectAtIndex: i];
            app
        })
        .collect()
}

/// Checks if the process behind an NSRunningApplication is still alive.
///
/// macOS keeps stale entries in `runningApplications` for several seconds after
/// termination. `NSRunningApplication.isTerminated` requires run loop events to
/// update, which our server doesn't process, so we check liveness via `kill(pid, 0)`.
unsafe fn is_app_alive(app: id) -> bool {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let pid: i32 = msg_send![app, processIdentifier];
    kill(pid, 0) == 0
}

/// List all running applications
pub fn list_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    unsafe {
        for app in get_running_apps() {
            if !is_app_alive(app) {
                continue;
            }

            // Get localized name
            let name_ns: id = msg_send![app, localizedName];
            let name = if name_ns != nil {
                nsstring_to_string(name_ns)
            } else {
                continue;
            };

            // Get bundle identifier
            let bundle_id_ns: id = msg_send![app, bundleIdentifier];
            let bundle_id = if bundle_id_ns != nil {
                Some(nsstring_to_string(bundle_id_ns))
            } else {
                None
            };

            // Get PID
            let pid: i32 = msg_send![app, processIdentifier];

            // Get active state
            let is_active: bool = msg_send![app, isActive];

            // Get hidden state
            let is_hidden: bool = msg_send![app, isHidden];

            // Get activation policy to distinguish user-facing apps from background agents
            // NSApplicationActivationPolicyRegular = 0
            let activation_policy: i64 = msg_send![app, activationPolicy];
            let is_user_app = activation_policy == 0;

            // Filter to user-facing apps (those with a name)
            if !name.is_empty() {
                apps.push(AppInfo {
                    name,
                    bundle_id,
                    pid,
                    is_active,
                    is_hidden,
                    is_user_app,
                });
            }
        }
    }

    apps
}

/// Activate (focus) an application by name
pub fn activate_app(app_name: &str) -> bool {
    unsafe {
        for app in get_running_apps() {
            let name_ns: id = msg_send![app, localizedName];

            if name_ns != nil {
                let name = nsstring_to_string(name_ns);
                if name.to_lowercase().contains(&app_name.to_lowercase()) {
                    let _: bool = msg_send![app, activateWithOptions: 1u64]; // NSApplicationActivateIgnoringOtherApps
                    return true;
                }
            }
        }
    }

    false
}

/// Activate an application by PID
pub fn activate_app_by_pid(pid: i32) -> bool {
    unsafe {
        let app: id = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];

        if app != nil {
            let _: bool = msg_send![app, activateWithOptions: 1u64];
            return true;
        }
    }

    false
}

/// Check if a user-facing application is currently running (case-insensitive name match).
///
/// Only considers apps with NSApplicationActivationPolicyRegular (i.e., apps that
/// appear in the Dock). Ignores background agents, helpers, and accessory processes.
pub fn is_app_running(app_name: &str) -> bool {
    unsafe {
        let needle = app_name.to_lowercase();
        for app in get_running_apps() {
            if !is_app_alive(app) {
                continue;
            }

            // Only consider user-facing apps (NSApplicationActivationPolicyRegular = 0)
            let activation_policy: i64 = msg_send![app, activationPolicy];
            if activation_policy != 0 {
                continue;
            }

            let name_ns: id = msg_send![app, localizedName];
            if name_ns != nil {
                let name = nsstring_to_string(name_ns);
                if name.to_lowercase().contains(&needle) {
                    return true;
                }
            }
        }
    }
    false
}

/// Launch an application by name using `open -a`.
///
/// Finds the app in standard locations (/Applications, ~/Applications, etc.)
/// and launches it. If args is non-empty, passes them via `--args`.
/// If the app is already running and no args are given, it is brought to the front.
pub fn launch_app(app_name: &str, args: &[String]) -> Result<(), String> {
    let mut cmd = std::process::Command::new("open");
    cmd.arg("-a").arg(app_name);

    if !args.is_empty() {
        cmd.arg("--args");
        cmd.args(args);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run 'open' command: {}", e))?;

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

/// Quit an application by name.
///
/// By default performs a graceful termination (NSRunningApplication.terminate).
/// If `force` is true, uses forceTerminate which kills immediately without cleanup.
/// Returns the number of app instances terminated.
pub fn quit_app(app_name: &str, force: bool) -> Result<u32, String> {
    let mut terminated = 0u32;

    unsafe {
        let needle = app_name.to_lowercase();
        for app in get_running_apps() {
            if !is_app_alive(app) {
                continue;
            }

            let name_ns: id = msg_send![app, localizedName];
            if name_ns != nil {
                let name = nsstring_to_string(name_ns);
                if name.to_lowercase().contains(&needle) {
                    let success: bool = if force {
                        msg_send![app, forceTerminate]
                    } else {
                        msg_send![app, terminate]
                    };
                    if success {
                        terminated += 1;
                    }
                }
            }
        }
    }

    if terminated > 0 {
        Ok(terminated)
    } else {
        Err(format!(
            "No running app found matching '{}'. Use list_apps to find the correct app name.",
            app_name
        ))
    }
}

unsafe fn nsstring_to_string(ns_string: id) -> String {
    let utf8_ptr: *const i8 = msg_send![ns_string, UTF8String];
    if utf8_ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(utf8_ptr)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that quit_app + list_apps doesn't return stale entries.
    ///
    /// Requires Calculator.app — run with: cargo test test_no_stale_entries -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_no_stale_entries_after_quit() {
        let app = "Calculator";

        // Ensure Calculator is running
        launch_app(app, &[]).ok();
        std::thread::sleep(std::time::Duration::from_secs(2));

        let apps = list_apps();
        let found = apps.iter().find(|a| a.name == app);
        assert!(
            found.is_some(),
            "Calculator should appear in list_apps after launch"
        );
        let old_pid = found.unwrap().pid;
        println!("Calculator running at PID {}", old_pid);

        // Force quit to ensure immediate termination
        let result = quit_app(app, true);
        assert!(result.is_ok(), "quit_app should succeed");
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Stale entry should be filtered by is_app_alive
        let apps = list_apps();
        let stale = apps.iter().find(|a| a.name == app && a.pid == old_pid);
        assert!(
            stale.is_none(),
            "Stale Calculator entry (PID {}) should be filtered from list_apps",
            old_pid
        );

        // is_app_running should return false
        assert!(
            !is_app_running(app),
            "is_app_running should return false after quit"
        );

        // quit_app on an already-dead app should return an error
        let result = quit_app(app, false);
        assert!(
            result.is_err(),
            "quit_app should error when app is not running"
        );

        // Cleanup
        quit_app(app, true).ok();
    }
}
