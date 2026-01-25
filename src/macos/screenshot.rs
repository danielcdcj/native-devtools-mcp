use super::display;
use std::process::Command;
use tempfile::tempdir;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScreenshotError {
    #[error("Failed to capture screenshot: {0}")]
    CaptureError(String),
    #[error("Failed to read screenshot file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Window not found: {0}")]
    WindowNotFound(u32),
}

pub struct Screenshot {
    pub png_data: Vec<u8>,
    /// The backing scale factor of the display this screenshot was taken from.
    /// Used for converting pixel coordinates to screen coordinates in OCR.
    pub scale_factor: f64,
    /// Screen-space origin of the screenshot (top-left), in points.
    pub origin_x: f64,
    pub origin_y: f64,
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
    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x,
        origin_y,
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

    // screencapture -R x,y,w,h for region
    let region = format!(
        "{},{},{},{}",
        x as i32, y as i32, width as i32, height as i32
    );

    let output = Command::new("screencapture")
        .args(["-x", "-R", &region, "-t", "png", &path_str])
        .output()?;

    if !output.status.success() {
        return Err(ScreenshotError::CaptureError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // Determine scale factor based on which display the region is on
    let scale_factor = display::backing_scale_for_point(x, y);

    let png_data = std::fs::read(&path)?;
    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: x,
        origin_y: y,
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

    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: window.bounds.x,
        origin_y: window.bounds.y,
    })
}
