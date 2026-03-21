pub mod app_protocol;
pub mod ax_snapshot;
pub mod find_image;
pub mod hover_tracker;
pub mod image_cache;
pub mod input;
pub mod load_image;
pub mod navigation;
pub mod screen_recorder;
pub mod screenshot;
pub mod screenshot_cache;

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as Unix milliseconds.
pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// JPEG compression quality used for screenshots and recording frames (0–100).
pub(crate) const JPEG_QUALITY: u8 = 80;
