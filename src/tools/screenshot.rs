use crate::platform;
use rmcp::model::{CallToolResult, Content};
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;

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

    /// Include OCR text annotations with clickable coordinates (default: true)
    #[serde(default = "default_include_ocr")]
    pub include_ocr: bool,
}

fn default_mode() -> String {
    "screen".to_string()
}

fn default_include_ocr() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct ScreenshotMetadata {
    screenshot_origin_x: f64,
    screenshot_origin_y: f64,
    screenshot_scale: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    screenshot_window_id: Option<u32>,
}

pub fn take_screenshot(params: TakeScreenshotParams) -> CallToolResult {
    let result = match params.mode.as_str() {
        "screen" => platform::capture_screen(),
        "window" => {
            let window_id = match params.window_id {
                Some(id) => id,
                None => {
                    return CallToolResult::error(vec![Content::text(
                        "window_id is required for mode='window'",
                    )]);
                }
            };
            platform::capture_window(window_id)
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
            platform::capture_region(x, y, w, h)
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
            let mut contents = vec![Content::image(base64_data, "image/png")];
            let screenshot_window_id = if params.mode == "window" {
                params.window_id
            } else {
                None
            };
            let metadata = ScreenshotMetadata {
                screenshot_origin_x: screenshot.origin_x,
                screenshot_origin_y: screenshot.origin_y,
                screenshot_scale: screenshot.scale_factor,
                screenshot_window_id,
            };
            if let Ok(json) = to_string_pretty(&metadata) {
                contents.push(Content::text(json));
            }

            // Run OCR if requested
            if params.include_ocr {
                match platform::ocr_image(&screenshot.png_data, Some(screenshot.scale_factor)) {
                    Ok(mut matches) => {
                        apply_ocr_offset(&mut matches, screenshot.origin_x, screenshot.origin_y);
                        if !matches.is_empty() {
                            let ocr_text = format_ocr_results(&matches);
                            contents.push(Content::text(ocr_text));
                        }
                    }
                    Err(e) => {
                        contents.push(Content::text(format!("OCR failed: {}", e)));
                    }
                }
            }

            CallToolResult::success(contents)
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Screenshot failed: {}", e))]),
    }
}

/// Convert OCR coordinates from image-relative to screen-absolute.
///
/// OCR detects text at pixel positions within the screenshot image. For window
/// or region screenshots, these positions are relative to the image origin (0,0).
/// To make the coordinates directly clickable, we add the screenshot's screen
/// origin so the LLM can use them with the click tool without further translation.
fn apply_ocr_offset(matches: &mut [platform::TextMatch], offset_x: f64, offset_y: f64) {
    if offset_x == 0.0 && offset_y == 0.0 {
        return;
    }

    for m in matches {
        m.x += offset_x;
        m.y += offset_y;
        m.bounds.x += offset_x;
        m.bounds.y += offset_y;
    }
}

/// Format OCR results as a text summary with clickable coordinates and bounds.
fn format_ocr_results(matches: &[platform::TextMatch]) -> String {
    let mut result = String::from("## OCR Text Detected (click coordinates)\n\n");

    for m in matches.iter().filter(|m| m.confidence > 0.5) {
        result.push_str(&format!(
            "- \"{}\" at ({:.0}, {:.0}) bounds: {{x: {:.0}, y: {:.0}, w: {:.0}, h: {:.0}}}\n",
            m.text, m.x, m.y, m.bounds.x, m.bounds.y, m.bounds.width, m.bounds.height
        ));
    }

    result
}
