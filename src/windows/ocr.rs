//! OCR functionality using Windows.Media.Ocr for text detection.

use super::display;
use super::screenshot::capture_screen;
use serde::{Deserialize, Serialize};
use windows::core::Interface;
use windows::Graphics::Imaging::{BitmapDecoder, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};

/// Bounding box in screen coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A text match found by OCR with screen coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMatch {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub confidence: f64,
    pub bounds: TextBounds,
}

/// Run OCR on PNG image data and return all detected text with coordinates.
///
/// Coordinates are in image pixels, scaled by the provided scale factor.
/// The caller is responsible for adding screen offsets.
pub fn ocr_image(png_data: &[u8], scale: Option<f64>) -> Result<Vec<TextMatch>, String> {
    let scale = scale.unwrap_or_else(|| {
        display::get_main_display()
            .map(|d| d.backing_scale_factor)
            .unwrap_or(1.0)
    });

    run_winrt_ocr(png_data, scale)
}

fn run_winrt_ocr(png_data: &[u8], scale: f64) -> Result<Vec<TextMatch>, String> {
    let debug = std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok();

    // Create OCR engine for system language
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("OCR not available (Windows 10 1903+ required): {}", e))?;

    // Load PNG into SoftwareBitmap
    let bitmap = load_png_to_software_bitmap(png_data)?;

    if debug {
        if let (Ok(w), Ok(h)) = (bitmap.PixelWidth(), bitmap.PixelHeight()) {
            eprintln!(
                "[DEBUG run_winrt_ocr] bitmap_size={}x{}, scale_param={}",
                w, h, scale
            );
        }
    }

    // Run OCR
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("OCR failed: {}", e))?
        .get()
        .map_err(|e| format!("OCR async failed: {}", e))?;

    let lines = result
        .Lines()
        .map_err(|e| format!("Failed to get OCR lines: {}", e))?;

    let mut matches = Vec::new();
    let mut logged_first = false;

    for line in lines {
        let words = line.Words().map_err(|e| e.to_string())?;

        for word in words {
            let text = word.Text().map_err(|e| e.to_string())?.to_string();
            let rect = word.BoundingRect().map_err(|e| e.to_string())?;

            // Debug: log raw WinRT rect for first word
            if debug && !logged_first {
                eprintln!(
                    "[DEBUG run_winrt_ocr] first_word='{}', raw_rect=({}, {}, {}x{}), after_scale=({}, {})",
                    text,
                    rect.X, rect.Y, rect.Width, rect.Height,
                    rect.X as f64 / scale, rect.Y as f64 / scale
                );
                logged_first = true;
            }

            // WinRT returns coordinates in image pixels
            let bounds = TextBounds {
                x: rect.X as f64 / scale,
                y: rect.Y as f64 / scale,
                width: rect.Width as f64 / scale,
                height: rect.Height as f64 / scale,
            };

            // Center point for clicking
            let center_x = bounds.x + bounds.width / 2.0;
            let center_y = bounds.y + bounds.height / 2.0;

            matches.push(TextMatch {
                text,
                x: center_x,
                y: center_y,
                confidence: 1.0, // WinRT OCR doesn't provide per-word confidence
                bounds,
            });
        }
    }

    Ok(matches)
}

fn load_png_to_software_bitmap(png_data: &[u8]) -> Result<SoftwareBitmap, String> {
    // Create an in-memory stream
    let stream =
        InMemoryRandomAccessStream::new().map_err(|e| format!("Failed to create stream: {}", e))?;

    // Write PNG data to stream
    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|e| format!("Failed to create writer: {}", e))?;

    writer
        .WriteBytes(png_data)
        .map_err(|e| format!("Failed to write bytes: {}", e))?;

    writer
        .StoreAsync()
        .map_err(|e| format!("Failed to store: {}", e))?
        .get()
        .map_err(|e| format!("Store async failed: {}", e))?;

    writer
        .FlushAsync()
        .map_err(|e| format!("Failed to flush: {}", e))?
        .get()
        .map_err(|e| format!("Flush async failed: {}", e))?;

    // Detach stream from writer
    writer
        .DetachStream()
        .map_err(|e| format!("Failed to detach stream: {}", e))?;

    // Reset stream position to beginning
    stream
        .Seek(0)
        .map_err(|e| format!("Failed to seek: {}", e))?;

    // Create decoder
    let stream_ref: IRandomAccessStream = stream
        .cast()
        .map_err(|e| format!("Failed to cast stream: {}", e))?;

    let decoder = BitmapDecoder::CreateAsync(&stream_ref)
        .map_err(|e| format!("Failed to create decoder: {}", e))?
        .get()
        .map_err(|e| format!("Decoder async failed: {}", e))?;

    // Get software bitmap
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| format!("Failed to get bitmap: {}", e))?
        .get()
        .map_err(|e| format!("Bitmap async failed: {}", e))?;

    Ok(bitmap)
}

/// Find text on screen using OCR. Returns screen coordinates for each match.
pub fn find_text(search: &str, display_id: Option<u32>) -> Result<Vec<TextMatch>, String> {
    let displays = display::get_displays()?;
    let _display = displays
        .iter()
        .find(|d| display_id.map_or(d.is_main, |id| d.id == id))
        .cloned()
        .ok_or("Display not found")?;

    // Capture the display
    // For now, capture the full virtual screen and filter by display bounds
    let screenshot = capture_screen().map_err(|e| format!("Screenshot failed: {}", e))?;

    // Use screenshot.scale_factor, not display.backing_scale_factor.
    // On Windows, BitBlt captures in logical coordinates, so scale_factor is 1.0.
    let mut matches = ocr_image(&screenshot.png_data, Some(screenshot.scale_factor))?;

    // Debug: log first match before offset
    if std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok() {
        if let Some(first) = matches
            .iter()
            .find(|m| m.text.to_lowercase().contains(&search.to_lowercase()))
        {
            eprintln!(
                "[DEBUG find_text] search='{}', screenshot_origin=({}, {}), scale_factor={}, first_match_before_offset=({}, {})",
                search, screenshot.origin_x, screenshot.origin_y, screenshot.scale_factor, first.x, first.y
            );
        }
    }

    // Offset coordinates by screenshot origin and filter by search term
    let search_lower = search.to_lowercase();
    for m in &mut matches {
        m.x += screenshot.origin_x;
        m.y += screenshot.origin_y;
        m.bounds.x += screenshot.origin_x;
        m.bounds.y += screenshot.origin_y;
    }

    matches.retain(|m| m.text.to_lowercase().contains(&search_lower));
    matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    // Debug: log first match after offset
    if std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok() {
        if let Some(first) = matches.first() {
            eprintln!(
                "[DEBUG find_text] first_match_after_offset: text='{}', screen_coords=({}, {})",
                first.text, first.x, first.y
            );
        }
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_engine_available() {
        // Just test that we can create an OCR engine
        let result = OcrEngine::TryCreateFromUserProfileLanguages();
        assert!(result.is_ok(), "Should be able to query OCR availability");
    }
}
