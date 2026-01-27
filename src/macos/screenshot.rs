use super::display;
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
