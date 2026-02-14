use std::io::Cursor;

use super::device::AndroidDevice;

pub struct AndroidScreenshot {
    pub png_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Capture a screenshot, trying the ADB framebuffer first and falling back to `screencap -p`.
pub fn capture(device: &mut AndroidDevice) -> Result<AndroidScreenshot, String> {
    let png_data = match device.framebuffer_png() {
        Ok(data) => data,
        Err(fb_err) => {
            tracing::warn!(
                "Framebuffer capture failed, falling back to screencap: {}",
                fb_err
            );
            let mut data = Vec::new();
            device
                .shell_bytes(&["screencap", "-p"], &mut data)
                .map_err(|e| format!("screencap fallback also failed: {}", e))?;
            if data.is_empty() {
                return Err("screencap returned empty output".to_string());
            }
            data
        }
    };

    let (width, height) = png_dimensions(&png_data).unwrap_or((0, 0));
    Ok(AndroidScreenshot {
        png_data,
        width,
        height,
    })
}

fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    use image::ImageReader;
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}
