//! Input tools for system-level mouse and keyboard simulation.
//!
//! These tools wrap input operations in `spawn_blocking` to avoid
//! blocking the tokio runtime, since input operations use `thread::sleep`.

use crate::platform::{display, input, ocr};
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;

/// Check accessibility permission and return appropriate error if not granted.
fn check_permission() -> Option<CallToolResult> {
    if !input::check_accessibility_permission() {
        #[cfg(target_os = "macos")]
        let msg = "Accessibility permission required.\n\n\
             Grant permission to your MCP client (e.g., Claude Desktop, VS Code, Terminal) in:\n\
             System Settings → Privacy & Security → Accessibility\n\n\
             The permission must be granted to the app that runs this MCP server, \
             not to the server binary itself.";

        #[cfg(target_os = "windows")]
        let msg = "Input injection permission denied.\n\n\
             This typically occurs when targeting elevated (admin) windows \
             from a non-elevated process, or when targeting secure desktops.";

        return Some(CallToolResult::error(vec![Content::text(msg)]));
    }
    None
}

/// Run a blocking input operation and convert the result to CallToolResult.
async fn run_input<F>(op: F, success_msg: String, error_prefix: &str) -> CallToolResult
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    match tokio::task::spawn_blocking(op).await {
        Ok(Ok(())) => CallToolResult::success(vec![Content::text(success_msg)]),
        Ok(Err(e)) => {
            CallToolResult::error(vec![Content::text(format!("{}: {}", error_prefix, e))])
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!("Task failed: {}", e))]),
    }
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
    /// Screenshot origin X coordinate in screen space
    pub screenshot_origin_x: Option<f64>,
    /// Screenshot origin Y coordinate in screen space
    pub screenshot_origin_y: Option<f64>,
    /// Backing scale factor used for the screenshot
    pub screenshot_scale: Option<f64>,
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

pub async fn click(params: ClickParams) -> CallToolResult {
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
        let window = match crate::platform::find_window_by_id(window_id) {
            Ok(Some(w)) => w,
            Ok(None) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Window {} not found",
                    window_id
                ))])
            }
            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
        };

        let bounds = display::WindowBounds {
            x: window.bounds.x,
            y: window.bounds.y,
        };
        display::window_to_screen(&bounds, wx, wy)
    } else if let (Some(px), Some(py), Some(origin_x), Some(origin_y), Some(scale)) = (
        params.screenshot_x,
        params.screenshot_y,
        params.screenshot_origin_x,
        params.screenshot_origin_y,
        params.screenshot_scale,
    ) {
        // Screenshot pixel coordinates with captured origin + scale
        let bounds = display::WindowBounds {
            x: origin_x,
            y: origin_y,
        };
        display::screenshot_to_screen(&bounds, scale, px, py)
    } else if let (Some(px), Some(py), Some(window_id)) = (
        params.screenshot_x,
        params.screenshot_y,
        params.screenshot_window_id,
    ) {
        // Screenshot pixel coordinates (legacy: lookup window at click time)
        let window = match crate::platform::find_window_by_id(window_id) {
            Ok(Some(w)) => w,
            Ok(None) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Window {} not found",
                    window_id
                ))])
            }
            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
        };

        let bounds = display::WindowBounds {
            x: window.bounds.x,
            y: window.bounds.y,
        };

        // macOS: screencapture captures in physical (Retina) pixels, need scale factor
        // Windows: BitBlt captures in logical coordinates, scale is always 1.0
        #[cfg(target_os = "macos")]
        let scale = display::backing_scale_for_point(window.bounds.x, window.bounds.y);
        #[cfg(target_os = "windows")]
        let scale = 1.0;

        display::screenshot_to_screen(&bounds, scale, px, py)
    } else {
        return CallToolResult::error(vec![Content::text(
            "Provide coordinates in one of these formats:\n\
             - Screen coordinates: x, y\n\
             - Window-relative: window_x, window_y, window_id\n\
             - Screenshot pixels: screenshot_x, screenshot_y, screenshot_origin_x, screenshot_origin_y, screenshot_scale\n\
             - Screenshot pixels (legacy): screenshot_x, screenshot_y, screenshot_window_id",
        )]);
    };

    let click_count = params.click_count;
    run_input(
        move || input::click(x, y, button, click_count),
        format!("Clicked at ({:.0}, {:.0})", x, y),
        "Click failed",
    )
    .await
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

pub async fn move_mouse(params: MoveMouseParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let (x, y) = (params.x, params.y);
    run_input(
        move || input::move_mouse(x, y),
        format!("Moved mouse to ({:.0}, {:.0})", x, y),
        "Move failed",
    )
    .await
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

pub async fn drag(params: DragParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let button = match params.button.as_deref() {
        Some("right") => input::MouseButton::Right,
        Some("center") | Some("middle") => input::MouseButton::Center,
        _ => input::MouseButton::Left,
    };

    let (start_x, start_y, end_x, end_y) =
        (params.start_x, params.start_y, params.end_x, params.end_y);
    run_input(
        move || input::drag(start_x, start_y, end_x, end_y, button),
        format!(
            "Dragged from ({:.0}, {:.0}) to ({:.0}, {:.0})",
            start_x, start_y, end_x, end_y
        ),
        "Drag failed",
    )
    .await
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

pub async fn scroll(params: ScrollParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let (x, y, delta_x, delta_y) = (params.x, params.y, params.delta_x, params.delta_y);
    run_input(
        move || input::scroll(x, y, delta_x, delta_y),
        format!(
            "Scrolled at ({:.0}, {:.0}) by ({}, {})",
            x, y, delta_x, delta_y
        ),
        "Scroll failed",
    )
    .await
}

// ============================================================================
// Type Text
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TypeTextParams {
    /// Text to type
    pub text: String,
}

pub async fn type_text(params: TypeTextParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let len = params.text.len();
    let text = params.text;
    run_input(
        move || input::type_text(&text),
        format!("Typed {} characters", len),
        "Type failed",
    )
    .await
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

pub async fn press_key(params: PressKeyParams) -> CallToolResult {
    if let Some(err) = check_permission() {
        return err;
    }

    let key_desc = if params.modifiers.is_empty() {
        params.key.clone()
    } else {
        format!("{}+{}", params.modifiers.join("+"), params.key)
    };

    let key = params.key;
    let modifiers = params.modifiers;
    run_input(
        move || input::press_key(&key, &modifiers),
        format!("Pressed {}", key_desc),
        "Key press failed",
    )
    .await
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
// Find Text (Accessibility + OCR)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct FindTextParams {
    pub text: String,
    /// Optional display ID to search on. If omitted, searches the main display.
    /// Ignored when window_id or app_name is provided.
    pub display_id: Option<u32>,
    /// Window ID to scope the search to a specific window.
    pub window_id: Option<u32>,
    /// Application name to scope the search to a specific app's window.
    pub app_name: Option<String>,
    /// Enable language correction (helps with word accuracy but hurts single-character
    /// detection). Defaults to false, which is better for UI automation.
    #[serde(default)]
    pub uses_language_correction: bool,
}

pub fn find_text(params: FindTextParams) -> CallToolResult {
    let debug = std::env::var("NATIVE_DEVTOOLS_DEBUG").is_ok();

    // Resolve window_id from app_name if provided
    let window_id = match (params.window_id, &params.app_name) {
        (Some(id), _) => Some(id),
        (None, Some(app_name)) => match crate::platform::find_windows_by_app(app_name) {
            Ok(windows) if !windows.is_empty() => Some(windows[0].id),
            Ok(_) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "No window found for app '{}'",
                    app_name
                ))]);
            }
            Err(e) => {
                return CallToolResult::error(vec![Content::text(format!(
                    "Failed to find window: {}",
                    e
                ))]);
            }
        },
        (None, None) => None,
    };

    // Primary: try accessibility tree search
    match find_text_accessibility(&params.text, window_id) {
        Ok(mut matches) if !matches.is_empty() => {
            rank_matches(&mut matches, &params.text);
            return serialize_matches(&matches);
        }
        Ok(_) if debug => {
            eprintln!(
                "[DEBUG find_text] no accessibility matches for '{}', trying OCR",
                params.text
            );
        }
        Err(e) if debug => {
            eprintln!(
                "[DEBUG find_text] accessibility failed for '{}': {}, trying OCR",
                params.text, e
            );
        }
        _ => {}
    }

    // Fallback: OCR
    let ocr_result = if let Some(wid) = window_id {
        find_text_in_window(&params.text, wid, params.uses_language_correction)
    } else {
        #[cfg(target_os = "macos")]
        {
            ocr::find_text(
                &params.text,
                params.display_id,
                params.uses_language_correction,
            )
        }
        #[cfg(target_os = "windows")]
        {
            ocr::find_text(&params.text, params.display_id)
        }
    };

    match ocr_result {
        Ok(ref matches) if !matches.is_empty() => serialize_matches(matches),
        Ok(_) => empty_result_with_available_elements(&params.text, window_id, debug),
        Err(e) => CallToolResult::error(vec![Content::text(e)]),
    }
}

/// Try to find text using the platform accessibility API.
fn find_text_accessibility(
    search: &str,
    window_id: Option<u32>,
) -> Result<Vec<ocr::TextMatch>, String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::ax::find_text(search, window_id)
    }
    #[cfg(target_os = "windows")]
    {
        // TODO: support targeting specific window_id via ElementFromHandle(hwnd)
        let _ = window_id;
        crate::windows::uia::find_text(search)
    }
}

/// Collect all visible element names using the platform accessibility API.
fn list_element_names_accessibility(window_id: Option<u32>) -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::ax::list_element_names(window_id)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = window_id;
        crate::windows::uia::list_element_names()
    }
}

const MAX_HINT_ELEMENTS: usize = 200;

/// Build a "no matches found" hint JSON string with available element names.
/// Shared between desktop `find_text` and `android_find_text`.
pub fn build_no_matches_hint(search: &str, available_elements: &[String]) -> String {
    let capped: Vec<&str> = available_elements
        .iter()
        .take(MAX_HINT_ELEMENTS)
        .map(|s| s.as_str())
        .collect();
    let hint = serde_json::json!({
        "message": format!("No matches found for \"{}\"", search),
        "available_elements": capped,
    });
    hint.to_string()
}

/// Build an empty result with a list of available UI elements as a hint.
fn empty_result_with_available_elements(
    search: &str,
    window_id: Option<u32>,
    debug: bool,
) -> CallToolResult {
    let mut content = vec![Content::text("[]")];

    match list_element_names_accessibility(window_id) {
        Ok(names) => {
            if debug && !names.is_empty() {
                eprintln!(
                    "[DEBUG find_text] listing {} available element names as hint",
                    names.len()
                );
            }
            content.push(Content::text(build_no_matches_hint(search, &names)));
        }
        Err(e) if debug => {
            eprintln!("[DEBUG find_text] failed to list element names: {}", e);
        }
        _ => {}
    }

    CallToolResult::success(content)
}

/// Serialize text matches to a JSON CallToolResult.
/// Rank find_text results so that exact matches and interactive elements appear first.
///
/// Ranking priority (lower score = higher rank):
///   0 — exact match + interactive element
///   1 — exact match + non-interactive element
///   2 — substring match + interactive element
///   3 — substring match + non-interactive element
///
/// Within the same rank, original tree-traversal order is preserved (stable sort).
fn rank_matches(matches: &mut [ocr::TextMatch], search: &str) {
    let search_lower = search.to_lowercase();
    matches.sort_by_key(|m| {
        let is_exact = m.text.to_lowercase() == search_lower;
        let is_interactive = m.role.as_deref().is_some_and(is_interactive_role);
        match (is_exact, is_interactive) {
            (true, true) => 0u8,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        }
    });
}

/// Check whether an accessibility role represents an interactive element.
///
/// Covers both macOS AXRole names (e.g. "AXButton") and Windows UIA control
/// type names (e.g. "Button").
fn is_interactive_role(role: &str) -> bool {
    matches!(
        role,
        // macOS AXRoles
        "AXButton"
        | "AXTextField"
        | "AXTextArea"
        | "AXLink"
        | "AXCheckBox"
        | "AXRadioButton"
        | "AXPopUpButton"
        | "AXMenuButton"
        | "AXSlider"
        | "AXIncrementor"
        | "AXComboBox"
        | "AXMenuItem"
        | "AXTabGroup"
        | "AXTab"
        // Windows UIA control types
        | "Button"
        | "Edit"
        | "Hyperlink"
        | "CheckBox"
        | "RadioButton"
        | "ComboBox"
        | "Slider"
        | "Spinner"
        | "MenuItem"
        | "TabItem"
        | "ListItem"
        | "TreeItem"
        | "DataItem"
        | "SplitButton"
    )
}

fn serialize_matches(matches: &[ocr::TextMatch]) -> CallToolResult {
    match serde_json::to_string_pretty(matches) {
        Ok(json) => CallToolResult::success(vec![Content::text(json)]),
        Err(e) => CallToolResult::error(vec![Content::text(format!("Failed to serialize: {}", e))]),
    }
}

/// Run OCR scoped to a single window and return matching text with screen coordinates.
fn find_text_in_window(
    search: &str,
    window_id: u32,
    uses_language_correction: bool,
) -> Result<Vec<ocr::TextMatch>, String> {
    let screenshot = crate::platform::capture_window(window_id)
        .map_err(|e| format!("Failed to capture window: {}", e))?;

    #[cfg(target_os = "macos")]
    let mut matches = ocr::ocr_image(
        &screenshot.png_data,
        Some(screenshot.scale_factor),
        uses_language_correction,
    )?;
    #[cfg(target_os = "windows")]
    let mut matches = {
        let _ = uses_language_correction; // Windows OCR doesn't support this param
        ocr::ocr_image(&screenshot.png_data, Some(screenshot.scale_factor))?
    };

    // Offset OCR coordinates from image-relative to screen-absolute
    for m in &mut matches {
        m.x += screenshot.origin_x;
        m.y += screenshot.origin_y;
        m.bounds.x += screenshot.origin_x;
        m.bounds.y += screenshot.origin_y;
    }

    // Filter by search term
    let search_lower = search.to_lowercase();
    matches.retain(|m| m.text.to_lowercase().contains(&search_lower));
    matches.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ocr::{TextBounds, TextMatch};

    fn make_match(text: &str, role: Option<&str>) -> TextMatch {
        TextMatch {
            text: text.to_string(),
            x: 0.0,
            y: 0.0,
            confidence: 1.0,
            bounds: TextBounds {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 20.0,
            },
            role: role.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_rank_exact_match_before_substring() {
        let mut matches = vec![
            make_match("2×3", None), // substring match, no role
            make_match("2", None),   // exact match, no role
        ];
        rank_matches(&mut matches, "2");
        assert_eq!(matches[0].text, "2");
        assert_eq!(matches[1].text, "2×3");
    }

    #[test]
    fn test_rank_interactive_before_static() {
        let mut matches = vec![
            make_match("Submit", Some("AXStaticText")), // static
            make_match("Submit", Some("AXButton")),     // interactive
        ];
        rank_matches(&mut matches, "Submit");
        assert_eq!(matches[0].role.as_deref(), Some("AXButton"));
        assert_eq!(matches[1].role.as_deref(), Some("AXStaticText"));
    }

    #[test]
    fn test_rank_exact_interactive_is_top() {
        let mut matches = vec![
            make_match("2×3", Some("AXButton")),   // substring + interactive
            make_match("2", Some("AXStaticText")), // exact + static
            make_match("2×3", Some("AXStaticText")), // substring + static
            make_match("2", Some("AXButton")),     // exact + interactive
        ];
        rank_matches(&mut matches, "2");
        assert_eq!(matches[0].text, "2");
        assert_eq!(matches[0].role.as_deref(), Some("AXButton"));
        assert_eq!(matches[1].text, "2");
        assert_eq!(matches[1].role.as_deref(), Some("AXStaticText"));
        assert_eq!(matches[2].text, "2×3");
        assert_eq!(matches[2].role.as_deref(), Some("AXButton"));
        assert_eq!(matches[3].text, "2×3");
        assert_eq!(matches[3].role.as_deref(), Some("AXStaticText"));
    }

    #[test]
    fn test_rank_preserves_order_within_same_rank() {
        let mut matches = vec![
            make_match("Open", Some("AXButton")),
            make_match("Open", Some("AXMenuItem")),
        ];
        rank_matches(&mut matches, "Open");
        // Both are exact + interactive, original order preserved (stable sort)
        assert_eq!(matches[0].role.as_deref(), Some("AXButton"));
        assert_eq!(matches[1].role.as_deref(), Some("AXMenuItem"));
    }

    #[test]
    fn test_rank_case_insensitive_exact_match() {
        let mut matches = vec![
            make_match("SUBMIT button", Some("AXStaticText")),
            make_match("submit", Some("AXButton")),
        ];
        rank_matches(&mut matches, "Submit");
        assert_eq!(matches[0].text, "submit");
        assert_eq!(matches[1].text, "SUBMIT button");
    }

    #[test]
    fn test_rank_no_role_treated_as_non_interactive() {
        let mut matches = vec![
            make_match("OK", None),             // exact, no role (OCR)
            make_match("OK", Some("AXButton")), // exact, interactive
        ];
        rank_matches(&mut matches, "OK");
        assert_eq!(matches[0].role.as_deref(), Some("AXButton"));
        assert_eq!(matches[1].role, None);
    }

    #[test]
    fn test_is_interactive_role_macos() {
        assert!(is_interactive_role("AXButton"));
        assert!(is_interactive_role("AXTextField"));
        assert!(is_interactive_role("AXLink"));
        assert!(is_interactive_role("AXCheckBox"));
        assert!(is_interactive_role("AXMenuItem"));
        assert!(!is_interactive_role("AXStaticText"));
        assert!(!is_interactive_role("AXGroup"));
        assert!(!is_interactive_role("AXImage"));
        assert!(!is_interactive_role("AXScrollArea"));
    }

    #[test]
    fn test_is_interactive_role_windows() {
        assert!(is_interactive_role("Button"));
        assert!(is_interactive_role("Edit"));
        assert!(is_interactive_role("Hyperlink"));
        assert!(is_interactive_role("CheckBox"));
        assert!(is_interactive_role("MenuItem"));
        assert!(!is_interactive_role("Text"));
        assert!(!is_interactive_role("Group"));
        assert!(!is_interactive_role("Image"));
        assert!(!is_interactive_role("Pane"));
    }
}
