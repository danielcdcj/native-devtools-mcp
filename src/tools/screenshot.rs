use crate::macos;
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TakeScreenshotParams {
    /// Capture mode: "screen", "window", or "region"
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Window ID (required for mode="window")
    pub window_id: Option<u32>,

    /// Region coordinates (required for mode="region")
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

fn default_mode() -> String {
    "screen".to_string()
}

pub fn take_screenshot(params: TakeScreenshotParams) -> CallToolResult {
    let result = match params.mode.as_str() {
        "screen" => macos::capture_screen(),
        "window" => {
            let window_id = match params.window_id {
                Some(id) => id,
                None => {
                    return CallToolResult::error(vec![Content::text(
                        "window_id is required for mode='window'",
                    )]);
                }
            };
            macos::capture_window(window_id)
        }
        "region" => {
            let (x, y, w, h) = match (params.x, params.y, params.width, params.height) {
                (Some(x), Some(y), Some(w), Some(h)) => (x, y, w, h),
                _ => {
                    return CallToolResult::error(vec![Content::text(
                        "x, y, width, and height are required for mode='region'",
                    )]);
                }
            };
            macos::capture_region(x, y, w, h)
        }
        _ => {
            return CallToolResult::error(vec![Content::text(format!(
                "Unknown mode '{}'. Use 'screen', 'window', or 'region'",
                params.mode
            ))]);
        }
    };

    match result {
        Ok(screenshot) => {
            let base64_data = screenshot.to_base64();
            CallToolResult::success(vec![Content::image(base64_data, "image/png")])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Screenshot failed: {}", e))]),
    }
}
