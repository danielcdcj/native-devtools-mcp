//! Input tools for system-level mouse and keyboard simulation.

use crate::macos::{display, input, ocr};
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;

/// Check accessibility permission and return appropriate error if not granted.
fn check_permission() -> Option<CallToolResult> {
    if !input::check_accessibility_permission() {
        return Some(CallToolResult::error(vec![Content::text(
            "Accessibility permission required.\n\n\
             Grant permission to your MCP client (e.g., Claude Desktop, VS Code, Terminal) in:\n\
             System Settings → Privacy & Security → Accessibility\n\n\
             The permission must be granted to the app that runs this MCP server, \
             not to the server binary itself.",
        )]));
    }
    None
}

// ============================================================================
// Click
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ClickParams {
    /// Screen X coordinate (required unless using window-relative)
    pub x: Option<f64>,
    /// Screen Y coordinate (required unless using window-relative)
    pub y: Option<f64>,

    /// Window-relative X coordinate
    pub window_x: Option<f64>,
    /// Window-relative Y coordinate
    pub window_y: Option<f64>,
    /// Window ID for window-relative coordinates
    pub window_id: Option<u32>,

    /// Screenshot pixel X coordinate
    pub screenshot_x: Option<f64>,
    /// Screenshot pixel Y coordinate
    pub screenshot_y: Option<f64>,
    /// Window ID that the screenshot was taken from (for scaling)
    pub screenshot_window_id: Option<u32>,

    /// Mouse button: "left" (default), "right", or "center"
    #[serde(default)]
    pub button: Option<String>,

    /// Number of clicks (1 for single, 2 for double)
    #[serde(default = "default_click_count")]
    pub click_count: u32,
}

fn default_click_count() -> u32 {
    1
}

pub fn click(params: ClickParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    // Parse button
    let button = match params.button.as_deref() {
        Some("right") => input::MouseButton::Right,
        Some("center") | Some("middle") => input::MouseButton::Center,
        _ => input::MouseButton::Left,
    };

    // Resolve coordinates
    let (x, y) = if let (Some(x), Some(y)) = (params.x, params.y) {
        // Direct screen coordinates
        (x, y)
    } else if let (Some(wx), Some(wy), Some(window_id)) =
        (params.window_x, params.window_y, params.window_id)
    {
        // Window-relative coordinates
        let window = match crate::macos::find_window_by_id(window_id) {
            Some(w) => w,
            None => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Window {} not found",
                    window_id
                ))])
            }
        };

        let bounds = display::WindowBounds {
            x: window.bounds.x,
            y: window.bounds.y,
        };
        display::window_to_screen(&bounds, wx, wy)
    } else if let (Some(px), Some(py), Some(window_id)) = (
        params.screenshot_x,
        params.screenshot_y,
        params.screenshot_window_id,
    ) {
        // Screenshot pixel coordinates
        let window = match crate::macos::find_window_by_id(window_id) {
            Some(w) => w,
            None => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Window {} not found",
                    window_id
                ))])
            }
        };

        let bounds = display::WindowBounds {
            x: window.bounds.x,
            y: window.bounds.y,
        };

        // Get backing scale factor for this window's location
        let scale = display::backing_scale_for_point(window.bounds.x, window.bounds.y);
        display::screenshot_to_screen(&bounds, scale, px, py)
    } else {
        return CallToolResult::error(vec![Content::text(
            "Provide coordinates in one of these formats:\n\
             - Screen coordinates: x, y\n\
             - Window-relative: window_x, window_y, window_id\n\
             - Screenshot pixels: screenshot_x, screenshot_y, screenshot_window_id",
        )]);
    };

    match input::click(x, y, button, params.click_count) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Clicked at ({:.0}, {:.0})",
            x, y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Click failed: {}", e))]),
    }
}

// ============================================================================
// Move Mouse
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MoveMouseParams {
    /// Screen X coordinate
    pub x: f64,
    /// Screen Y coordinate
    pub y: f64,
}

pub fn move_mouse(params: MoveMouseParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    match input::move_mouse(params.x, params.y) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Moved mouse to ({:.0}, {:.0})",
            params.x, params.y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Move failed: {}", e))]),
    }
}

// ============================================================================
// Drag
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DragParams {
    /// Start X coordinate
    pub start_x: f64,
    /// Start Y coordinate
    pub start_y: f64,
    /// End X coordinate
    pub end_x: f64,
    /// End Y coordinate
    pub end_y: f64,
    /// Mouse button: "left" (default), "right", or "center"
    #[serde(default)]
    pub button: Option<String>,
}

pub fn drag(params: DragParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let button = match params.button.as_deref() {
        Some("right") => input::MouseButton::Right,
        Some("center") | Some("middle") => input::MouseButton::Center,
        _ => input::MouseButton::Left,
    };

    match input::drag(
        params.start_x,
        params.start_y,
        params.end_x,
        params.end_y,
        button,
    ) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Dragged from ({:.0}, {:.0}) to ({:.0}, {:.0})",
            params.start_x, params.start_y, params.end_x, params.end_y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Drag failed: {}", e))]),
    }
}

// ============================================================================
// Scroll
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ScrollParams {
    /// X coordinate to scroll at
    pub x: f64,
    /// Y coordinate to scroll at
    pub y: f64,
    /// Horizontal scroll delta (positive = right)
    #[serde(default)]
    pub delta_x: i32,
    /// Vertical scroll delta (positive = down, negative = up)
    pub delta_y: i32,
}

pub fn scroll(params: ScrollParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    match input::scroll(params.x, params.y, params.delta_x, params.delta_y) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Scrolled at ({:.0}, {:.0}) by ({}, {})",
            params.x, params.y, params.delta_x, params.delta_y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Scroll failed: {}", e))]),
    }
}

// ============================================================================
// Type Text
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TypeTextParams {
    /// Text to type
    pub text: String,
}

pub fn type_text(params: TypeTextParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    match input::type_text(&params.text) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Typed {} characters",
            params.text.len()
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Type failed: {}", e))]),
    }
}

// ============================================================================
// Press Key
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PressKeyParams {
    /// Key to press (e.g., "return", "tab", "a", "f1")
    pub key: String,
    /// Modifier keys: "shift", "control", "option", "command"
    #[serde(default)]
    pub modifiers: Vec<String>,
}

pub fn press_key(params: PressKeyParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    match input::press_key(&params.key, &params.modifiers) {
        Ok(()) => {
            let key_desc = if params.modifiers.is_empty() {
                params.key.clone()
            } else {
                format!("{}+{}", params.modifiers.join("+"), params.key)
            };
            CallToolResult::success(vec![Content::text(format!("Pressed {}", key_desc))])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Key press failed: {}", e))]),
    }
}

// ============================================================================
// Get Displays
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GetDisplaysParams {}

pub fn get_displays(_params: GetDisplaysParams) -> CallToolResult {
    match display::get_displays() {
        Ok(displays) => match serde_json::to_string_pretty(&displays) {
            Ok(json) => CallToolResult::success(vec![Content::text(json)]),
            Err(e) => CallToolResult::error(vec![Content::text(format!(
                "Failed to serialize displays: {}",
                e
            ))]),
        },
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "Failed to get displays: {}",
            e
        ))]),
    }
}

// ============================================================================
// Find Text (OCR)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct FindTextParams {
    pub text: String,
}

pub fn find_text(params: FindTextParams) -> CallToolResult {
    match ocr::find_text(&params.text) {
        Ok(matches) if matches.is_empty() => {
            CallToolResult::success(vec![Content::text(format!(
                "No matches found for \"{}\"",
                params.text
            ))])
        }
        Ok(matches) => match serde_json::to_string_pretty(&matches) {
            Ok(json) => CallToolResult::success(vec![Content::text(json)]),
            Err(e) => CallToolResult::error(vec![Content::text(format!(
                "Failed to serialize: {}",
                e
            ))]),
        },
        Err(e) => CallToolResult::error(vec![Content::text(e)]),
    }
}
