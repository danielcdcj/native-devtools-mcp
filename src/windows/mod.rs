//! Windows platform backend for native devtools.
//!
//! This module provides Windows implementations of system automation APIs
//! that match the interface of the macOS backend.

pub mod app;
pub mod display;
pub mod input;
pub mod ocr;
pub mod screenshot;
pub mod uia;
pub mod window;

pub use app::*;
pub use ocr::{ocr_image, TextMatch};
pub use screenshot::*;
pub use window::*;

/// Initialize Windows-specific settings. Call this early in main().
/// Sets process DPI awareness to Per-Monitor V2 for correct coordinate handling.
pub fn init() -> Result<(), String> {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .map_err(|e| format!("Failed to set DPI awareness: {}", e))?;
    }
    Ok(())
}
