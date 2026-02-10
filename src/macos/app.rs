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
