//! Screenshot capture for Windows using GDI and WIC.

use super::display;
use super::window::{find_window_by_id, hwnd_from_id, WindowBounds};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::mem;
use thiserror::Error;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

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
    pub scale_factor: f64,
    /// Screen-space origin of the screenshot (top-left).
    pub origin_x: f64,
    pub origin_y: f64,
}

impl Screenshot {
    pub fn to_base64(&self) -> String {
        STANDARD.encode(&self.png_data)
    }
}

/// Capture the entire virtual screen (all monitors).
pub fn capture_screen() -> Result<Screenshot, ScreenshotError> {
    let (vx, vy, vw, vh) = display::get_virtual_screen_bounds();

    let display = display::get_main_display().ok();
    let scale_factor = display.map(|d| d.backing_scale_factor).unwrap_or(1.0);

    let png_data = capture_region_to_png(vx, vy, vw, vh)?;

    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: vx as f64,
        origin_y: vy as f64,
    })
}

/// Capture a specific region of the screen.
pub fn capture_region(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<Screenshot, ScreenshotError> {
    let scale_factor = display::backing_scale_for_point(x, y);

    let png_data = capture_region_to_png(x as i32, y as i32, width as i32, height as i32)?;

    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: x,
        origin_y: y,
    })
}

/// Capture a specific window by its ID.
pub fn capture_window(window_id: u32) -> Result<Screenshot, ScreenshotError> {
    let window = find_window_by_id(window_id)
        .map_err(ScreenshotError::CaptureError)?
        .ok_or(ScreenshotError::WindowNotFound(window_id))?;

    let hwnd = hwnd_from_id(window_id);
    let bounds = get_window_bounds_for_capture(hwnd);

    let width = bounds.width as i32;
    let height = bounds.height as i32;

    if width <= 0 || height <= 0 {
        return Err(ScreenshotError::WindowNotFound(window_id));
    }

    // Use BitBlt to capture the window's screen region
    let png_data = capture_region_to_png(bounds.x as i32, bounds.y as i32, width, height)?;

    let scale_factor = display::backing_scale_for_point(window.bounds.x, window.bounds.y);

    Ok(Screenshot {
        png_data,
        scale_factor,
        origin_x: window.bounds.x,
        origin_y: window.bounds.y,
    })
}

fn get_window_bounds_for_capture(hwnd: HWND) -> WindowBounds {
    let mut rect = RECT::default();

    // Try DWM extended frame bounds first
    let dwm_result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            mem::size_of::<RECT>() as u32,
        )
    };

    if dwm_result.is_err() {
        unsafe {
            let _ = GetWindowRect(hwnd, &mut rect);
        }
    }

    WindowBounds {
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left) as f64,
        height: (rect.bottom - rect.top) as f64,
    }
}

/// Capture a region of the screen using BitBlt.
fn capture_region_to_png(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, ScreenshotError> {
    unsafe {
        let screen_dc = GetDC(HWND::default());
        if screen_dc.is_invalid() {
            return Err(ScreenshotError::CaptureError("GetDC failed".to_string()));
        }

        let result = capture_dc_region_to_png(screen_dc, x, y, width, height);

        ReleaseDC(HWND::default(), screen_dc);

        result
    }
}

/// Capture from a DC to PNG.
fn capture_dc_region_to_png(
    source_dc: HDC,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, ScreenshotError> {
    unsafe {
        let mem_dc = CreateCompatibleDC(source_dc);
        if mem_dc.is_invalid() {
            return Err(ScreenshotError::CaptureError(
                "CreateCompatibleDC failed".to_string(),
            ));
        }

        let bitmap = CreateCompatibleBitmap(source_dc, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            return Err(ScreenshotError::CaptureError(
                "CreateCompatibleBitmap failed".to_string(),
            ));
        }

        let old_bitmap = SelectObject(mem_dc, bitmap);

        let blt_result = BitBlt(mem_dc, 0, 0, width, height, source_dc, x, y, SRCCOPY);

        let result = if blt_result.is_ok() {
            extract_bitmap_to_png(mem_dc, bitmap, width, height)
        } else {
            Err(ScreenshotError::CaptureError("BitBlt failed".to_string()))
        };

        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);

        result
    }
}

/// Extract bitmap data and encode as PNG.
fn extract_bitmap_to_png(
    dc: HDC,
    bitmap: HBITMAP,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, ScreenshotError> {
    unsafe {
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Negative for top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default()],
        };

        let row_bytes = width as usize * 4;
        let mut pixels: Vec<u8> = vec![0; row_bytes * height as usize];

        let result = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        if result == 0 {
            return Err(ScreenshotError::CaptureError(
                "GetDIBits failed".to_string(),
            ));
        }

        // Convert BGRA to RGBA
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2); // Swap B and R
        }

        // Encode as PNG using a simple PNG encoder
        encode_rgba_to_png(&pixels, width as u32, height as u32)
    }
}

/// Encode RGBA pixel data to PNG format.
/// This is a minimal PNG encoder for our specific use case.
fn encode_rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ScreenshotError> {
    use std::io::Write;

    let mut output = Vec::new();

    // PNG signature
    output
        .write_all(&[137, 80, 78, 71, 13, 10, 26, 10])
        .map_err(|e| ScreenshotError::CaptureError(e.to_string()))?;

    // IHDR chunk
    let mut ihdr_data = Vec::new();
    ihdr_data.extend_from_slice(&width.to_be_bytes());
    ihdr_data.extend_from_slice(&height.to_be_bytes());
    ihdr_data.push(8); // bit depth
    ihdr_data.push(6); // color type (RGBA)
    ihdr_data.push(0); // compression method
    ihdr_data.push(0); // filter method
    ihdr_data.push(0); // interlace method
    write_chunk(&mut output, b"IHDR", &ihdr_data)?;

    // Prepare image data with filter bytes
    let row_bytes = width as usize * 4;
    let mut raw_data = Vec::with_capacity((row_bytes + 1) * height as usize);
    for row in 0..height as usize {
        raw_data.push(0); // No filter
        raw_data.extend_from_slice(&rgba[row * row_bytes..(row + 1) * row_bytes]);
    }

    // Compress with zlib (deflate)
    let compressed = compress_zlib(&raw_data);

    // IDAT chunk
    write_chunk(&mut output, b"IDAT", &compressed)?;

    // IEND chunk
    write_chunk(&mut output, b"IEND", &[])?;

    Ok(output)
}

fn write_chunk(
    output: &mut Vec<u8>,
    chunk_type: &[u8; 4],
    data: &[u8],
) -> Result<(), ScreenshotError> {
    use std::io::Write;

    let length = data.len() as u32;
    output
        .write_all(&length.to_be_bytes())
        .map_err(|e| ScreenshotError::CaptureError(e.to_string()))?;
    output
        .write_all(chunk_type)
        .map_err(|e| ScreenshotError::CaptureError(e.to_string()))?;
    output
        .write_all(data)
        .map_err(|e| ScreenshotError::CaptureError(e.to_string()))?;

    // CRC32 of chunk type + data
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(chunk_type);
    crc_data.extend_from_slice(data);
    let crc = crc32(&crc_data);
    output
        .write_all(&crc.to_be_bytes())
        .map_err(|e| ScreenshotError::CaptureError(e.to_string()))?;

    Ok(())
}

/// Simple zlib compression (deflate with zlib header).
fn compress_zlib(data: &[u8]) -> Vec<u8> {
    // Use miniz_oxide-style compression via flate2-compatible approach
    // For simplicity, we'll use the Windows built-in compression or store uncompressed

    // Minimal zlib: CMF=0x78 (deflate, 32K window), FLG=0x01 (no dict, fastest)
    // Then stored blocks, then Adler-32 checksum

    let mut output = Vec::new();
    output.push(0x78); // CMF
    output.push(0x01); // FLG

    // Store data in uncompressed deflate blocks
    let mut remaining = data;
    while !remaining.is_empty() {
        let chunk_size = remaining.len().min(65535);
        let is_final = chunk_size == remaining.len();

        output.push(if is_final { 0x01 } else { 0x00 }); // BFINAL + BTYPE=00 (stored)
        output.extend_from_slice(&(chunk_size as u16).to_le_bytes());
        output.extend_from_slice(&(!(chunk_size as u16)).to_le_bytes());
        output.extend_from_slice(&remaining[..chunk_size]);

        remaining = &remaining[chunk_size..];
    }

    // Adler-32 checksum
    let adler = adler32(data);
    output.extend_from_slice(&adler.to_be_bytes());

    output
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;

    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }

    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[index] ^ (crc >> 8);
    }

    crc ^ 0xFFFFFFFF
}

// CRC32 lookup table for PNG
static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};
