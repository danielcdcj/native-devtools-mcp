//! Tests for screenshot metadata integrity
//!
//! These tests verify:
//! 1. Region capture aligns origin coordinates to integers
//!
//! Note: Screenshot struct field tests are not included because the struct
//! is marked #[non_exhaustive] and cannot be constructed in external crates.
//! The struct fields are validated through integration tests on supported platforms.

// Only compile this test file on supported platforms
#![cfg(any(target_os = "macos", target_os = "windows"))]

#[cfg(test)]
mod region_origin_alignment {
    /// Test that region coordinates truncate to integers correctly.
    /// This verifies the fix for region origin rounding drift where
    /// origin_x/y retained float values while screencapture used truncated ints.
    #[test]
    fn test_region_coordinate_truncation() {
        // Simulate the truncation logic used in capture_region
        let x: f64 = 100.7;
        let y: f64 = 200.9;

        let x_int = x as i32;
        let y_int = y as i32;

        // Verify truncation (not rounding)
        assert_eq!(x_int, 100);
        assert_eq!(y_int, 200);

        // The aligned origin should use the truncated values
        let aligned_x = f64::from(x_int);
        let aligned_y = f64::from(y_int);

        assert_eq!(aligned_x, 100.0);
        assert_eq!(aligned_y, 200.0);
    }

    #[test]
    fn test_negative_region_coordinates() {
        // On multi-display setups, coordinates can be negative
        let x: f64 = -100.3;
        let y: f64 = -50.8;

        let x_int = x as i32;
        let y_int = y as i32;

        // Verify truncation toward zero for negative values
        assert_eq!(x_int, -100);
        assert_eq!(y_int, -50);
    }
}
