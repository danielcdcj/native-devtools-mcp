use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use std::ffi::c_void;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccessibilityError {
    #[error("No element found at position ({0}, {1})")]
    NoElementAtPosition(f64, f64),
    #[error("Failed to perform action on element")]
    ActionFailed,
    #[error("Accessibility API error: {0}")]
    ApiError(i32),
}

// AXUIElement types (opaque pointers)
type AXUIElementRef = *mut c_void;

// AXError codes
const K_AX_ERROR_SUCCESS: i32 = 0;

// Link against ApplicationServices framework
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut AXUIElementRef,
    ) -> i32;
    fn AXUIElementPerformAction(
        element: AXUIElementRef,
        action: core_foundation::string::CFStringRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Click at a position using the Accessibility API without moving the mouse cursor.
/// This finds the UI element at the given coordinates and performs an AXPress action on it.
pub fn click_at_position(x: f64, y: f64) -> Result<(), AccessibilityError> {
    unsafe {
        // Get system-wide accessibility element
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            return Err(AccessibilityError::ApiError(-1));
        }

        // Get element at position
        let mut element: AXUIElementRef = std::ptr::null_mut();
        let result =
            AXUIElementCopyElementAtPosition(system_wide, x as f32, y as f32, &mut element);

        if result != K_AX_ERROR_SUCCESS {
            CFRelease(system_wide);
            return Err(AccessibilityError::ApiError(result));
        }

        if element.is_null() {
            CFRelease(system_wide);
            return Err(AccessibilityError::NoElementAtPosition(x, y));
        }

        // Perform press action
        let action = CFString::new("AXPress");
        let result = AXUIElementPerformAction(element, action.as_concrete_TypeRef());

        // Clean up
        CFRelease(element);
        CFRelease(system_wide);

        if result != K_AX_ERROR_SUCCESS {
            return Err(AccessibilityError::ActionFailed);
        }

        Ok(())
    }
}
