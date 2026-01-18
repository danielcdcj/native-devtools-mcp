use super::display;
use base64::{engine::general_purpose::STANDARD, Engine};
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
}

impl Screenshot {
    pub fn to_base64(&self) -> String {
        STANDARD.encode(&self.png_data)
    }
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

    let scale_factor = display::get_main_display()
        .map(|d| d.backing_scale_factor)
        .unwrap_or(2.0);

    let png_data = std::fs::read(&path)?;
    Ok(Screenshot {
        png_data,
        scale_factor,
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

    let png_data = std::fs::read(&path)?;

    if png_data.is_empty() {
        return Err(ScreenshotError::WindowNotFound(window_id));
    }

    // Determine scale factor based on window position
    let scale_factor = super::find_window_by_id(window_id)
        .ok()
        .flatten()
        .map(|w| display::backing_scale_for_point(w.bounds.x, w.bounds.y))
        .unwrap_or(2.0);

    Ok(Screenshot {
        png_data,
        scale_factor,
    })
}
