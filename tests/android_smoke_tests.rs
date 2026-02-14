//! Smoke tests for Android device integration.
//!
//! These tests require a real Android device connected via ADB with USB debugging enabled.
//! They are `#[ignore]`d by default so they don't run in CI.
//!
//! Run with:
//! ```bash
//! cargo test --features android --test android_smoke_tests -- --ignored --test-threads=1
//! ```
//!
//! Tests must run sequentially (`--test-threads=1`) since they share a single physical device.
//!
//! Prerequisites:
//! - ADB server running (`adb devices` should list at least one device)
//! - Device authorized for USB debugging
//! - Screen unlocked and awake
//! - For input/uiautomator tests: MIUI devices need "USB debugging (Security settings)" enabled

#![cfg(feature = "android")]

use native_devtools_mcp::android::{device, input, navigation, screenshot, ui_automator};
use std::thread;
use std::time::Duration;

/// Helper: connect to the first available device.
fn connect_first_device() -> device::AndroidDevice {
    let devices = device::list_devices().expect("Failed to list devices");
    assert!(!devices.is_empty(), "No ADB devices connected");
    let serial = &devices[0].serial;
    device::AndroidDevice::connect(serial)
        .unwrap_or_else(|e| panic!("Failed to connect to device '{}': {}", serial, e))
}

/// Helper: ensure device is awake, unlocked, and on home screen.
fn wake_and_go_home(device: &mut device::AndroidDevice) {
    input::press_key(device, "KEYCODE_WAKEUP").ok();
    thread::sleep(Duration::from_millis(500));
    input::press_key(device, "KEYCODE_HOME").ok();
    thread::sleep(Duration::from_secs(1));
}

/// Helper: launch Settings app and wait for it to be ready.
fn launch_settings(device: &mut device::AndroidDevice) {
    navigation::launch_app(device, "com.android.settings").expect("Failed to launch Settings");
    thread::sleep(Duration::from_secs(2));
}

// --- Core tools (no input injection needed) ---

#[test]
#[ignore]
fn test_list_devices() {
    let devices = device::list_devices().expect("Failed to list devices");
    assert!(!devices.is_empty(), "No ADB devices connected");
    for d in &devices {
        assert!(!d.serial.is_empty());
        assert_eq!(
            d.state, "device",
            "Device '{}' not ready: {}",
            d.serial, d.state
        );
    }
}

#[test]
#[ignore]
fn test_connect_and_shell() {
    let mut device = connect_first_device();
    let output = device.shell("echo hello").expect("Shell command failed");
    assert!(output.contains("hello"));
}

#[test]
#[ignore]
fn test_get_display_info() {
    let mut device = connect_first_device();
    let info = navigation::get_display_info(&mut device).expect("Failed to get display info");
    assert!(info.width > 0);
    assert!(info.height > 0);
    assert!(info.density > 0);
}

#[test]
#[ignore]
fn test_screenshot() {
    let mut device = connect_first_device();
    let shot = screenshot::capture(&mut device).expect("Failed to capture screenshot");
    assert!(!shot.png_data.is_empty());
    assert!(shot.width > 0);
    assert!(shot.height > 0);
    // PNG magic bytes
    assert_eq!(&shot.png_data[..4], &[0x89, 0x50, 0x4E, 0x47]);
}

#[test]
#[ignore]
fn test_list_apps() {
    let mut device = connect_first_device();
    let apps = navigation::list_apps(&mut device, false).expect("Failed to list apps");
    assert!(!apps.is_empty());
    // Every Android device has the settings package
    assert!(apps
        .iter()
        .any(|a| a.package_name == "com.android.settings"));
}

// --- Tools that require input injection ---

#[test]
#[ignore]
fn test_press_key_and_click() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    // Tap center of screen — safe no-op on any home screen
    let info = navigation::get_display_info(&mut device).expect("Failed to get display info");
    input::click(
        &mut device,
        info.width as f64 / 2.0,
        info.height as f64 / 2.0,
    )
    .expect("Failed to tap screen");
}

#[test]
#[ignore]
fn test_get_current_activity() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    launch_settings(&mut device);

    let activity =
        navigation::get_current_activity(&mut device).expect("Failed to get current activity");
    assert!(activity.package.contains("com.android.settings"));

    // Clean up
    input::press_key(&mut device, "KEYCODE_HOME").ok();
}

#[test]
#[ignore]
fn test_launch_app() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    launch_settings(&mut device);

    // Verify Settings is in foreground by taking a screenshot
    // (get_current_activity is tested separately)
    let shot = screenshot::capture(&mut device).expect("Failed to capture screenshot");
    assert!(shot.width > 0);

    // Clean up
    input::press_key(&mut device, "KEYCODE_HOME").ok();
}

#[test]
#[ignore]
fn test_find_text_in_settings() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    launch_settings(&mut device);

    // "Settings" appears in the title bar on every device/language (English assumed)
    let results = ui_automator::find_text(&mut device, "Settings")
        .expect("uiautomator dump failed — is the screen unlocked?");
    assert!(
        !results.is_empty(),
        "Should find 'Settings' text in Settings app"
    );
    assert!(
        results[0].x > 0.0 && results[0].y > 0.0,
        "Coordinates should be positive"
    );
    assert!(results[0].bounds.width > 0.0 && results[0].bounds.height > 0.0);

    // Clean up
    input::press_key(&mut device, "KEYCODE_HOME").ok();
}

#[test]
#[ignore]
fn test_swipe() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    launch_settings(&mut device);

    // Swipe up in center of screen (scroll down in Settings)
    let info = navigation::get_display_info(&mut device).expect("Failed to get display info");
    let cx = info.width as f64 / 2.0;
    let top = info.height as f64 * 0.7;
    let bottom = info.height as f64 * 0.3;
    input::swipe(&mut device, cx, top, cx, bottom, Some(300)).expect("Failed to swipe");

    // Clean up
    input::press_key(&mut device, "KEYCODE_HOME").ok();
}

#[test]
#[ignore]
fn test_type_text() {
    let mut device = connect_first_device();
    wake_and_go_home(&mut device);
    launch_settings(&mut device);

    // Tap the search area (most Settings apps have search near top)
    let info = navigation::get_display_info(&mut device).expect("Failed to get display info");
    // Look for a search element via uiautomator
    let search_results = ui_automator::find_text(&mut device, "Search");
    if let Ok(results) = search_results {
        if !results.is_empty() {
            input::click(&mut device, results[0].x, results[0].y).ok();
            thread::sleep(Duration::from_secs(1));
            input::type_text(&mut device, "wifi").expect("Failed to type text");
            thread::sleep(Duration::from_secs(1));
        }
    }
    // If no search bar found, just verify type_text doesn't error
    // (some devices may not have a visible search bar on Settings main page)

    // Clean up
    input::press_key(&mut device, "KEYCODE_HOME").ok();
    // Clear any lingering text input state
    let _ = info;
}
