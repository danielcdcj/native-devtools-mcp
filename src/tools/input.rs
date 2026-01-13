use crate::macos::{self, ClickType};
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;
use std::thread;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct ClickParams {
    /// X coordinate
    pub x: f64,

    /// Y coordinate
    pub y: f64,

    /// Click type: "left", "right", or "middle"
    #[serde(default = "default_click_type")]
    pub button: String,

    /// Whether to double-click
    #[serde(default)]
    pub double_click: bool,

    /// If true, use Accessibility API to click without moving the mouse cursor
    #[serde(default)]
    pub synthetic: bool,
}

fn default_click_type() -> String {
    "left".to_string()
}

pub fn click(params: ClickParams) -> CallToolResult {
    let click_type = match params.button.to_lowercase().as_str() {
        "left" => ClickType::Left,
        "right" => ClickType::Right,
        "middle" | "center" => ClickType::Middle,
        _ => {
            return CallToolResult::error(vec![Content::text(format!(
                "Unknown button '{}'. Use 'left', 'right', or 'middle'",
                params.button
            ))]);
        }
    };

    match macos::click(params.x, params.y, click_type, params.double_click, params.synthetic) {
        Ok(()) => {
            let action = if params.double_click {
                "Double-clicked"
            } else {
                "Clicked"
            };
            let mode = if params.synthetic {
                " (synthetic)"
            } else {
                ""
            };
            CallToolResult::success(vec![Content::text(format!(
                "{}{} at ({}, {})",
                action, mode, params.x, params.y
            ))])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Click failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct TypeTextParams {
    /// Text to type
    pub text: String,
}

pub fn type_text(params: TypeTextParams) -> CallToolResult {
    match macos::type_text(&params.text) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Typed {} characters",
            params.text.len()
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Type failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct PressKeyParams {
    /// Key or key combination (e.g., "Enter", "Cmd+C", "Ctrl+Shift+A")
    pub key: String,
}

pub fn press_key(params: PressKeyParams) -> CallToolResult {
    match macos::press_key(&params.key) {
        Ok(()) => {
            CallToolResult::success(vec![Content::text(format!("Pressed key: {}", params.key))])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Key press failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct ScrollParams {
    /// X coordinate for scroll position
    pub x: f64,

    /// Y coordinate for scroll position
    pub y: f64,

    /// Horizontal scroll delta (positive = right)
    #[serde(default)]
    pub delta_x: i32,

    /// Vertical scroll delta (positive = down)
    #[serde(default)]
    pub delta_y: i32,
}

pub fn scroll(params: ScrollParams) -> CallToolResult {
    match macos::scroll(params.x, params.y, params.delta_x, params.delta_y) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Scrolled at ({}, {}) by ({}, {})",
            params.x, params.y, params.delta_x, params.delta_y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Scroll failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct DragParams {
    /// Starting X coordinate
    pub from_x: f64,

    /// Starting Y coordinate
    pub from_y: f64,

    /// Ending X coordinate
    pub to_x: f64,

    /// Ending Y coordinate
    pub to_y: f64,
}

pub fn drag(params: DragParams) -> CallToolResult {
    match macos::drag(params.from_x, params.from_y, params.to_x, params.to_y) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Dragged from ({}, {}) to ({}, {})",
            params.from_x, params.from_y, params.to_x, params.to_y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Drag failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct MoveMouseParams {
    /// X coordinate
    pub x: f64,

    /// Y coordinate
    pub y: f64,
}

pub fn move_mouse(params: MoveMouseParams) -> CallToolResult {
    match macos::move_mouse(params.x, params.y) {
        Ok(()) => CallToolResult::success(vec![Content::text(format!(
            "Moved mouse to ({}, {})",
            params.x, params.y
        ))]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Move mouse failed: {}", e))]),
    }
}

#[derive(Debug, Deserialize)]
pub struct WaitParams {
    /// Milliseconds to wait
    pub ms: u64,
}

pub fn wait(params: WaitParams) -> CallToolResult {
    thread::sleep(Duration::from_millis(params.ms));
    CallToolResult::success(vec![Content::text(format!("Waited {} ms", params.ms))])
}
