use core_graphics::display::{CGDisplay, CGMainDisplayID};

/// Get the scale factor of the main display.
/// Returns 2.0 for Retina displays, 1.0 for standard displays.
pub fn get_main_display_scale_factor() -> f64 {
    let main_display_id = unsafe { CGMainDisplayID() };
    let display = CGDisplay::new(main_display_id);

    // Get the display mode to determine scale factor
    if let Some(mode) = display.display_mode() {
        let pixel_width = mode.pixel_width() as f64;
        let logical_width = mode.width() as f64;

        if logical_width > 0.0 {
            return pixel_width / logical_width;
        }
    }

    // Fallback to 1.0 if we can't determine scale factor
    1.0
}

/// Convert pixel coordinates (from screenshot) to logical coordinates (for input events).
/// Divides by the display scale factor.
#[inline]
pub fn pixels_to_points(pixel_coord: f64, scale_factor: f64) -> f64 {
    pixel_coord / scale_factor
}
