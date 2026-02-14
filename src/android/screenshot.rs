use std::io::Cursor;

use super::device::AndroidDevice;

/// A screenshot captured from an Android device.
pub struct AndroidScreenshot {
    /// Raw PNG image data.
    pub png_data: Vec<u8>,
    /// Width of the image in pixels.
    pub width: u32,
    /// Height of the image in pixels.
    pub height: u32,
}

/// Capture a screenshot from the given Android device.
///
/// Tries the ADB framebuffer protocol first, then falls back to
/// `screencap -p` via shell if the framebuffer call fails.
pub fn capture(device: &mut AndroidDevice) -> Result<AndroidScreenshot, String> {
    // Try framebuffer first
    match device.framebuffer_png() {
        Ok(png_data) => {
            let (width, height) = png_dimensions(&png_data).unwrap_or((0, 0));
            Ok(AndroidScreenshot {
                png_data,
                width,
                height,
            })
        }
        Err(fb_err) => {
            tracing::warn!(
                "Framebuffer capture failed, falling back to screencap: {}",
                fb_err
            );

            // Fall back to screencap -p via shell
            let mut png_data = Vec::new();
            device
                .shell_bytes(&["screencap", "-p"], &mut png_data)
                .map_err(|e| format!("screencap fallback also failed: {}", e))?;

            if png_data.is_empty() {
                return Err("screencap returned empty output".to_string());
            }

            let (width, height) = png_dimensions(&png_data).unwrap_or((0, 0));
            Ok(AndroidScreenshot {
                png_data,
                width,
                height,
            })
        }
    }
}

/// Extract pixel dimensions from PNG data using the `image` crate.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    use image::ImageReader;
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}
