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

/// List all running applications
pub fn list_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let running_apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![running_apps, count];

        for i in 0..count {
            let app: id = msg_send![running_apps, objectAtIndex: i];

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
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let running_apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![running_apps, count];

        for i in 0..count {
            let app: id = msg_send![running_apps, objectAtIndex: i];
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

/// Check if an application is currently running (case-insensitive name match).
pub fn is_app_running(app_name: &str) -> bool {
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let running_apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![running_apps, count];

        let needle = app_name.to_lowercase();
        for i in 0..count {
            let app: id = msg_send![running_apps, objectAtIndex: i];
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
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let running_apps: id = msg_send![workspace, runningApplications];
        let count: usize = msg_send![running_apps, count];

        let needle = app_name.to_lowercase();
        for i in 0..count {
            let app: id = msg_send![running_apps, objectAtIndex: i];
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
