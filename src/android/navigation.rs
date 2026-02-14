use serde::Serialize;

use super::device::AndroidDevice;

#[derive(Debug, Clone, Serialize)]
pub struct AndroidAppInfo {
    pub package_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AndroidDisplayInfo {
    pub width: u32,
    pub height: u32,
    pub density: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AndroidActivity {
    pub package: String,
    pub activity: String,
}

/// List installed packages. When `user_apps_only` is true, only third-party apps are returned.
pub fn list_apps(
    device: &mut AndroidDevice,
    user_apps_only: bool,
) -> Result<Vec<AndroidAppInfo>, String> {
    let output = if user_apps_only {
        device.shell_args(&["pm", "list", "packages", "-3"])?
    } else {
        device.shell_args(&["pm", "list", "packages"])?
    };

    Ok(parse_package_list(&output))
}

pub fn launch_app(device: &mut AndroidDevice, package_name: &str) -> Result<(), String> {
    // Force-stop first so the app starts fresh (avoids resuming a stale activity).
    device.shell_args(&["am", "force-stop", package_name]).ok();

    let output = device.shell_args(&[
        "monkey",
        "-p",
        package_name,
        "-c",
        "android.intent.category.LAUNCHER",
        "1",
    ])?;

    if output.contains("No activities found") {
        return Err(format!(
            "No launchable activity found for package '{}'",
            package_name
        ));
    }

    Ok(())
}

pub fn get_display_info(device: &mut AndroidDevice) -> Result<AndroidDisplayInfo, String> {
    let size_output = device.shell("wm size")?;
    let density_output = device.shell("wm density")?;

    let (width, height) = parse_wm_size(&size_output)
        .ok_or_else(|| format!("Failed to parse display size from: {}", size_output.trim()))?;

    let density = parse_wm_density(&density_output).ok_or_else(|| {
        format!(
            "Failed to parse display density from: {}",
            density_output.trim()
        )
    })?;

    Ok(AndroidDisplayInfo {
        width,
        height,
        density,
    })
}

pub fn get_current_activity(device: &mut AndroidDevice) -> Result<AndroidActivity, String> {
    let output = device.shell("dumpsys activity activities")?;

    parse_resumed_activity(&output)
        .ok_or_else(|| "No resumed activity found in dumpsys output".to_string())
}

fn parse_package_list(output: &str) -> Vec<AndroidAppInfo> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("package:").map(|pkg| AndroidAppInfo {
                package_name: pkg.trim().to_string(),
            })
        })
        .collect()
}

/// Parse `wm size` output, e.g. "Physical size: 1080x1920". Last match wins (override takes precedence).
fn parse_wm_size(output: &str) -> Option<(u32, u32)> {
    let mut result = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(size_part) = trimmed.split(':').nth(1) {
            let size_str = size_part.trim();
            if let Some((w, h)) = size_str.split_once('x') {
                if let (Ok(width), Ok(height)) = (w.trim().parse(), h.trim().parse()) {
                    result = Some((width, height));
                }
            }
        }
    }
    result
}

/// Parse `wm density` output, e.g. "Physical density: 420". Last match wins.
fn parse_wm_density(output: &str) -> Option<u32> {
    let mut result = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(density_part) = trimmed.split(':').nth(1) {
            if let Ok(density) = density_part.trim().parse() {
                result = Some(density);
            }
        }
    }
    result
}

/// Find the resumed activity from `dumpsys activity activities` output.
/// Matches common field names across Android variants:
/// - `mResumedActivity` (AOSP/stock)
/// - `topResumedActivity` (Samsung One UI, newer AOSP)
/// - `ResumedActivity:` (some Samsung builds)
fn parse_resumed_activity(output: &str) -> Option<AndroidActivity> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("mResumedActivity")
            || trimmed.contains("topResumedActivity")
            || trimmed.starts_with("ResumedActivity:")
        {
            if let Some(component) = extract_component_from_activity_line(trimmed) {
                return Some(component);
            }
        }
    }
    None
}

/// Extract package/activity component from an ActivityRecord line.
fn extract_component_from_activity_line(line: &str) -> Option<AndroidActivity> {
    for token in line.split_whitespace() {
        if token.contains('/') && token.contains('.') && !token.starts_with('{') {
            let (package, activity) = token.split_once('/')?;
            // Remove any trailing characters like "}" or "t123}"
            let activity_clean = activity.trim_end_matches('}');

            // If activity starts with '.', it's relative to the package
            let full_activity = if activity_clean.starts_with('.') {
                format!("{}{}", package, activity_clean)
            } else {
                activity_clean.to_string()
            };

            return Some(AndroidActivity {
                package: package.to_string(),
                activity: full_activity,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_list() {
        let output = "package:com.android.settings\npackage:com.android.browser\n";
        let apps = parse_package_list(output);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].package_name, "com.android.settings");
        assert_eq!(apps[1].package_name, "com.android.browser");
    }

    #[test]
    fn test_parse_package_list_empty() {
        let apps = parse_package_list("");
        assert!(apps.is_empty());
    }

    #[test]
    fn test_parse_wm_size() {
        let output = "Physical size: 1080x1920\n";
        let result = parse_wm_size(output);
        assert_eq!(result, Some((1080, 1920)));
    }

    #[test]
    fn test_parse_wm_size_with_override() {
        let output = "Physical size: 1080x1920\nOverride size: 720x1280\n";
        let result = parse_wm_size(output);
        // Override takes precedence (last match wins)
        assert_eq!(result, Some((720, 1280)));
    }

    #[test]
    fn test_parse_wm_density() {
        let output = "Physical density: 420\n";
        let result = parse_wm_density(output);
        assert_eq!(result, Some(420));
    }

    #[test]
    fn test_parse_resumed_activity() {
        let output =
            "  mResumedActivity: ActivityRecord{abc123 u0 com.android.settings/.Settings t42}\n";
        let result = parse_resumed_activity(output);
        assert!(result.is_some());
        let activity = result.unwrap();
        assert_eq!(activity.package, "com.android.settings");
        assert_eq!(activity.activity, "com.android.settings.Settings");
    }

    #[test]
    fn test_parse_resumed_activity_full_class() {
        let output = "  mResumedActivity: ActivityRecord{abc u0 com.app/com.app.MainActivity t1}\n";
        let result = parse_resumed_activity(output);
        assert!(result.is_some());
        let activity = result.unwrap();
        assert_eq!(activity.package, "com.app");
        assert_eq!(activity.activity, "com.app.MainActivity");
    }

    #[test]
    fn test_parse_resumed_activity_samsung_top_resumed() {
        let output = "      topResumedActivity=ActivityRecord{205279515 u0 com.microsoft.launcher/.Launcher t17857}\n";
        let result = parse_resumed_activity(output);
        assert!(result.is_some());
        let activity = result.unwrap();
        assert_eq!(activity.package, "com.microsoft.launcher");
        assert_eq!(activity.activity, "com.microsoft.launcher.Launcher");
    }

    #[test]
    fn test_parse_resumed_activity_samsung_resumed_label() {
        let output = "  ResumedActivity: ActivityRecord{108657286 u0 com.android.settings/.Settings t18519}\n";
        let result = parse_resumed_activity(output);
        assert!(result.is_some());
        let activity = result.unwrap();
        assert_eq!(activity.package, "com.android.settings");
        assert_eq!(activity.activity, "com.android.settings.Settings");
    }
}
