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
            let mut contents = vec![Content::image(base64_data, "image/png")];

            // Run OCR if requested
            if params.include_ocr {
                match macos::ocr_image(&screenshot.png_data) {
                    Ok(matches) => {
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

/// Format OCR results as a text summary with clickable coordinates.
fn format_ocr_results(matches: &[macos::TextMatch]) -> String {
    let mut result = String::from("## OCR Text Detected (click coordinates)\n\n");

    for m in matches.iter().filter(|m| m.confidence > 50.0) {
        result.push_str(&format!(
            "- \"{}\" at ({:.0}, {:.0})\n",
            m.text, m.x, m.y
        ));
    }

    result
}
