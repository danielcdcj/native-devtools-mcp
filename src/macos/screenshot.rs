use super::display;
use core_graphics::window::{
    kCGWindowImageBoundsIgnoreFraming, kCGWindowListOptionIncludingWindow,
};
use std::io::Cursor;
use std::process::Command;
use tempfile::tempdir;
use thiserror::Error;

/// Extract pixel dimensions from PNG data by reading the IHDR chunk.
fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    use image::ImageReader;
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let dims = reader.into_dimensions().ok()?;
    Some(dims)
}

#[derive(Error, Debug)]
pub enum ScreenshotError {
    #[error("Failed to capture screenshot: {0}")]
    CaptureError(String),
    #[error("Failed to read screenshot file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Window not found: {0}")]
    WindowNotFound(u32),
}

#[non_exhaustive]
pub struct Screenshot {
    pub png_data: Vec<u8>,
    /// The backing scale factor of the display this screenshot was taken from.
    /// Used for converting pixel coordinates to screen coordinates in OCR.
    pub scale_factor: f64,
    /// Screen-space origin of the screenshot (top-left), in points.
    pub origin_x: f64,
    pub origin_y: f64,
    /// Pixel dimensions of the screenshot image.
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// Capture the entire screen (main display) using screencapture
pub fn capture_screen() -> Result<Screenshot, ScreenshotError> {
    let temp_dir = tempdir()?;
    let path = temp_dir.path().join("screenshot.png");
    let path_str = path.to_string_lossy().to_string();

    let output = Command::new("screencapture")
        .args(["-x", "-C", "-t", "png", &path_str])
        .output()?;

    if !output.status.success() {
        return Err(ScreenshotError::CaptureError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let display = display::get_main_display().ok();
    let (scale_factor, origin_x, origin_y) = match display {
        Some(info) => (info.backing_scale_factor, info.bounds.x, info.bounds.y),
        None => (2.0, 0.0, 0.0),
    };

    let png_data = std::fs::read(&path)?;
    let (pixel_width, pixel_height) = png_dimensions(&png_data).unwrap_or((0, 0));
    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x,
        origin_y,
        pixel_width,
        pixel_height,
    })
}

/// Capture a specific region of the screen using screencapture
pub fn capture_region(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<Screenshot, ScreenshotError> {
    let temp_dir = tempdir()?;
    let path = temp_dir.path().join("screenshot.png");
    let path_str = path.to_string_lossy().to_string();

    // Round coordinates to integers to match what screencapture actually captures.
    // This ensures origin_x/y align with the captured region.
    let x_int = x as i32;
    let y_int = y as i32;
    let w_int = width as i32;
    let h_int = height as i32;

    // screencapture -R x,y,w,h for region
    let region = format!("{},{},{},{}", x_int, y_int, w_int, h_int);

    let output = Command::new("screencapture")
        .args(["-x", "-R", &region, "-t", "png", &path_str])
        .output()?;

    if !output.status.success() {
        return Err(ScreenshotError::CaptureError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // Use the integer-aligned origin to match the captured region exactly
    let origin_x = f64::from(x_int);
    let origin_y = f64::from(y_int);

    // Determine scale factor based on which display the region is on
    let scale_factor = display::backing_scale_for_point(origin_x, origin_y);

    let png_data = std::fs::read(&path)?;
    let (pixel_width, pixel_height) = png_dimensions(&png_data).unwrap_or((0, 0));
    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x,
        origin_y,
        pixel_width,
        pixel_height,
    })
}

/// Capture a specific window by its ID using screencapture
pub fn capture_window(window_id: u32) -> Result<Screenshot, ScreenshotError> {
    let temp_dir = tempdir()?;
    let path = temp_dir.path().join("screenshot.png");
    let path_str = path.to_string_lossy().to_string();

    // screencapture -l window_id for specific window
    // -o excludes window shadow so coordinates align with CGWindowBounds
    let output = Command::new("screencapture")
        .args([
            "-x",
            "-o",
            "-l",
            &window_id.to_string(),
            "-t",
            "png",
            &path_str,
        ])
        .output()?;

    if !output.status.success() {
        return Err(ScreenshotError::WindowNotFound(window_id));
    }

    let window = super::find_window_by_id(window_id)
        .ok()
        .flatten()
        .ok_or(ScreenshotError::WindowNotFound(window_id))?;
    let scale_factor = display::backing_scale_for_point(window.bounds.x, window.bounds.y);

    let png_data = std::fs::read(&path)?;

    if png_data.is_empty() {
        return Err(ScreenshotError::WindowNotFound(window_id));
    }

    let (pixel_width, pixel_height) = png_dimensions(&png_data).unwrap_or((0, 0));
    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: window.bounds.x,
        origin_y: window.bounds.y,
        pixel_width,
        pixel_height,
    })
}

/// Metadata returned alongside a JPEG-encoded window capture.
#[derive(Debug, Clone, Copy)]
pub struct WindowCaptureMeta {
    pub origin_x: f64,
    pub origin_y: f64,
    pub scale: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// Capture a window via `CGWindowListCreateImage` and return JPEG bytes directly.
///
/// Much faster than `capture_window` (no process spawn, no PNG roundtrip).
/// Used by the screen recorder for sustained ~3fps capture.
pub fn capture_window_cg_jpeg(
    window_id: u32,
    quality: u8,
) -> Result<(Vec<u8>, WindowCaptureMeta), ScreenshotError> {
    let window_info = super::window::find_window_by_id(window_id)
        .map_err(|e| ScreenshotError::CaptureError(e))?
        .ok_or(ScreenshotError::WindowNotFound(window_id))?;

    let null_rect = unsafe { core_graphics::display::CGRectNull };
    let cg_image = core_graphics::window::create_image(
        null_rect,
        kCGWindowListOptionIncludingWindow,
        window_id,
        kCGWindowImageBoundsIgnoreFraming,
    )
    .ok_or_else(|| {
        ScreenshotError::CaptureError("CGWindowListCreateImage returned null".into())
    })?;

    let pixel_width = cg_image.width() as u32;
    let pixel_height = cg_image.height() as u32;
    let scale = if window_info.bounds.width > 0.0 {
        pixel_width as f64 / window_info.bounds.width
    } else {
        display::backing_scale_for_point(window_info.bounds.x, window_info.bounds.y)
    };

    let jpeg_data = cg_image_to_jpeg(&cg_image, quality)?;

    Ok((
        jpeg_data,
        WindowCaptureMeta {
            origin_x: window_info.bounds.x,
            origin_y: window_info.bounds.y,
            scale,
            pixel_width,
            pixel_height,
        },
    ))
}

/// Convert a CGImage to JPEG bytes via the `image` crate.
///
/// Extracts raw pixel data from the CGImage, handles BGRA→RGBA conversion
/// (macOS uses BGRA byte order), and encodes directly to JPEG.
fn cg_image_to_jpeg(
    cg_image: &core_graphics::image::CGImage,
    quality: u8,
) -> Result<Vec<u8>, ScreenshotError> {
    let width = cg_image.width() as u32;
    let height = cg_image.height() as u32;
    let bytes_per_row = cg_image.bytes_per_row();
    let data = cg_image.data();
    let raw_bytes = data.bytes();

    // CGImage typically returns 32-bit BGRA with premultiplied alpha.
    // Convert to RGB for JPEG (which doesn't support alpha).
    let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height as usize {
        let row_start = y * bytes_per_row;
        for x in 0..width as usize {
            let offset = row_start + x * 4;
            if offset + 2 < raw_bytes.len() {
                // BGRA → RGB
                rgb_data.push(raw_bytes[offset + 2]); // R
                rgb_data.push(raw_bytes[offset + 1]); // G
                rgb_data.push(raw_bytes[offset]);     // B
            }
        }
    }

    let img = image::RgbImage::from_raw(width, height, rgb_data).ok_or_else(|| {
        ScreenshotError::CaptureError("Failed to create image from CGImage pixel data".into())
    })?;

    let mut jpeg_buf = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, quality);
    img.write_with_encoder(encoder)
        .map_err(|e| ScreenshotError::CaptureError(format!("JPEG encode failed: {e}")))?;

    Ok(jpeg_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // requires Finder running
    fn test_capture_window_cg_jpeg_returns_image_data() {
        let windows = super::super::window::find_windows_by_app("Finder")
            .expect("find_windows_by_app should succeed");
        let window = windows.first().expect("Finder must be running");
        let (jpeg, meta) =
            capture_window_cg_jpeg(window.id, 80).expect("capture_window_cg_jpeg should succeed");
        assert!(!jpeg.is_empty(), "JPEG data should not be empty");
        assert!(meta.pixel_width > 0);
        assert!(meta.pixel_height > 0);
        assert!(meta.scale > 0.0);
    }

    #[test]
    fn test_capture_window_cg_jpeg_invalid_window() {
        let result = capture_window_cg_jpeg(999_999_999, 80);
        assert!(result.is_err());
    }
}
