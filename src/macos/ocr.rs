//! OCR functionality using Tesseract for text detection on screen.

use super::display;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

/// A text match found by OCR with screen coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMatch {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub confidence: f64,
}

/// Run OCR on an image file and return all text with coordinates.
/// The `scale` parameter is used to convert pixel coordinates to screen coordinates.
fn run_ocr_on_file(image_path: &Path, scale: f64) -> Result<Vec<TextMatch>, String> {
    let tsv_base =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    let tsv_base_path = tsv_base.path().to_str().unwrap().to_string();

    let output = Command::new("tesseract")
        .args([
            image_path.to_str().unwrap(),
            &tsv_base_path,
            "-c",
            "tessedit_create_tsv=1",
        ])
        .output()
        .map_err(|e| format!("tesseract failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Parse TSV
    let tsv_path = format!("{}.tsv", tsv_base_path);
    let tsv =
        std::fs::read_to_string(&tsv_path).map_err(|e| format!("Failed to read TSV: {}", e))?;
    let _ = std::fs::remove_file(&tsv_path);

    let matches: Vec<TextMatch> = tsv
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 12 {
                return None;
            }

            let text = f[11].trim();
            if text.is_empty() {
                return None;
            }

            let left: f64 = f[6].parse().ok()?;
            let top: f64 = f[7].parse().ok()?;
            let width: f64 = f[8].parse().ok()?;
            let height: f64 = f[9].parse().ok()?;
            let confidence: f64 = f[10].parse().ok()?;

            Some(TextMatch {
                text: text.to_string(),
                x: (left + width / 2.0) / scale,
                y: (top + height / 2.0) / scale,
                confidence,
            })
        })
        .collect();

    Ok(matches)
}

/// Run OCR on PNG image data and return all detected text with screen coordinates.
/// Used by take_screenshot to include OCR annotations.
///
/// If `scale` is provided, it will be used to convert pixel coordinates to screen coordinates.
/// Otherwise, the main display's backing scale factor is used as a fallback.
pub fn ocr_image(png_data: &[u8], scale: Option<f64>) -> Result<Vec<TextMatch>, String> {
    let scale = scale.unwrap_or_else(|| {
        display::get_main_display()
            .map(|d| d.backing_scale_factor)
            .unwrap_or(2.0)
    });

    // Write PNG data to unique temp file (auto-cleaned on drop)
    let temp_file = tempfile::Builder::new()
        .prefix("native-devtools-ocr-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    std::fs::write(temp_file.path(), png_data)
        .map_err(|e| format!("Failed to write temp image: {}", e))?;

    run_ocr_on_file(temp_file.path(), scale)
}

/// Find text on screen using OCR. Returns screen coordinates for each match.
///
/// If `display_id` is provided, captures that specific display and uses its scale factor.
/// Otherwise, captures the main display.
pub fn find_text(search: &str, display_id: Option<u32>) -> Result<Vec<TextMatch>, String> {
    // Get the target display info
    let display = match display_id {
        Some(id) => display::get_displays()?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("Display {} not found", id))?,
        None => display::get_main_display()?,
    };

    let scale = display.backing_scale_factor;

    // Take screenshot to unique temp file (auto-cleaned on drop)
    let temp_file = tempfile::Builder::new()
        .prefix("native-devtools-ocr-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    // Use -D flag to capture specific display
    let status = Command::new("screencapture")
        .args([
            "-x",
            "-D",
            &display.id.to_string(),
            temp_file.path().to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("screencapture failed: {}", e))?;

    if !status.success() {
        return Err("screencapture failed".to_string());
    }

    // Run OCR and adjust coordinates to account for display offset
    let mut matches = run_ocr_on_file(temp_file.path(), scale)?;

    // Offset coordinates by display origin for multi-display setups
    for m in &mut matches {
        m.x += display.bounds.x;
        m.y += display.bounds.y;
    }

    // Filter by search term
    let search_lower = search.to_lowercase();
    let mut matches: Vec<TextMatch> = matches
        .into_iter()
        .filter(|m| m.text.to_lowercase().contains(&search_lower))
        .collect();

    matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    Ok(matches)
}
