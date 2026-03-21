use crate::app_protocol::AppProtocolClient;
use crate::tools::{
    app_protocol as app_tools, find_image, image_cache::ImageCache, input as input_tools,
    load_image, navigation, screenshot, screenshot_cache::ScreenshotCache,
};
use base64::Engine;
use rmcp::model::Content;
use rmcp::{
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
    Error as McpError,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::android::AndroidDevice;

/// Serialize a value to pretty-printed JSON, returning a formatted error on failure.
fn to_json_pretty(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("Failed to serialize: {}", e))
}

/// Extract a required string field from a JSON value.
fn parse_string_field(args: &Value, field: &str) -> Result<String, McpError> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| McpError::invalid_params(format!("missing required param: {}", field), None))
}

/// Extract required `x` and `y` number fields from a JSON value.
fn parse_xy(args: &Value) -> Result<(f64, f64), McpError> {
    let x = args
        .get("x")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| McpError::invalid_params("missing required param: x", None))?;
    let y = args
        .get("y")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| McpError::invalid_params("missing required param: y", None))?;
    Ok((x, y))
}

fn json_to_object(value: Value) -> rmcp::model::JsonObject {
    match value {
        Value::Object(map) => map,
        _ => Default::default(),
    }
}

#[derive(Clone)]
pub struct MacOSDevToolsServer {
    app_client: Arc<RwLock<Option<AppProtocolClient>>>,
    screenshot_cache: Arc<RwLock<ScreenshotCache>>,
    image_cache: Arc<RwLock<ImageCache>>,
    android_device: Arc<RwLock<Option<AndroidDevice>>>,
    hover_tracker: Arc<RwLock<Option<crate::tools::hover_tracker::HoverTracker>>>,
    screen_recorder: Arc<RwLock<Option<crate::tools::screen_recorder::ScreenRecorder>>>,
    #[cfg(feature = "cdp")]
    cdp_client: Arc<RwLock<Option<crate::cdp::CdpClient>>>,
}

impl Default for MacOSDevToolsServer {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSDevToolsServer {
    pub fn new() -> Self {
        Self {
            app_client: Arc::new(RwLock::new(None)),
            screenshot_cache: Arc::new(RwLock::new(ScreenshotCache::default())),
            image_cache: Arc::new(RwLock::new(ImageCache::default())),
            android_device: Arc::new(RwLock::new(None)),
            hover_tracker: Arc::new(RwLock::new(None)),
            screen_recorder: Arc::new(RwLock::new(None)),
            #[cfg(feature = "cdp")]
            cdp_client: Arc::new(RwLock::new(None)),
        }
    }

    async fn is_connected(&self) -> bool {
        self.app_client.read().await.is_some()
    }

    async fn is_android_connected(&self) -> bool {
        self.android_device.read().await.is_some()
    }

    async fn is_hover_tracking(&self) -> bool {
        self.hover_tracker.read().await.is_some()
    }

    async fn is_recording(&self) -> bool {
        self.screen_recorder.read().await.is_some()
    }

    #[cfg(feature = "cdp")]
    async fn is_cdp_connected(&self) -> bool {
        self.cdp_client.read().await.is_some()
    }

    /// Acquire the android device lock and call `f` with a mutable reference.
    /// Returns a "not connected" error result if no device is connected.
    async fn with_android_device<F>(&self, f: F) -> CallToolResult
    where
        F: FnOnce(&mut AndroidDevice) -> CallToolResult,
    {
        let mut guard = self.android_device.write().await;
        match guard.as_mut() {
            Some(device) => f(device),
            None => CallToolResult::error(vec![Content::text(
                "No Android device connected. Use android_connect first.",
            )]),
        }
    }

    /// Get tools available based on connection state.
    /// Base tools and app_connect are always available.
    /// Other app_* tools are only available when connected.
    pub fn get_tools(
        app_connected: bool,
        android_connected: bool,
        cdp_connected: bool,
        hover_tracking: bool,
        recording: bool,
    ) -> Vec<Tool> {
        let mut tools = Self::get_base_tools();
        tools.push(Self::get_app_connect_tool());
        if app_connected {
            tools.extend(Self::get_app_tools());
        }
        tools.extend(Self::get_android_base_tools());
        if android_connected {
            tools.extend(Self::get_android_tools());
        }
        #[cfg(feature = "cdp")]
        {
            tools.push(Self::get_cdp_connect_tool());
            if cdp_connected {
                tools.extend(Self::get_cdp_tools());
            }
        }
        let _ = cdp_connected; // suppress unused warning when cdp feature is off
        tools.extend(Self::get_hover_tracking_tools(hover_tracking));
        tools.extend(Self::get_recording_tools(recording));
        tools
    }

    /// Tools that are always available (system tools, CGEvent tools, etc.)
    fn get_base_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "take_screenshot",
                "Capture a screenshot of the screen, a specific window, or a region. Returns a base64-encoded image, JSON metadata for coordinate conversion, and OCR text annotations including clickable coordinates.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["screen", "window", "region"],
                            "description": "Capture mode (default: 'window'). Prefer 'window' with app_name for focused screenshots. Only use 'screen' when you need to see multiple windows or the desktop.",
                            "default": "window"
                        },
                        "window_id": {
                            "type": "integer",
                            "description": "Window ID to capture (required for mode='window')"
                        },
                        "app_name": {
                            "type": "string",
                            "description": "Application name to capture (for mode='window', alternative to window_id)"
                        },
                        "x": {
                            "type": "number",
                            "description": "X coordinate of region (required for mode='region')"
                        },
                        "y": {
                            "type": "number",
                            "description": "Y coordinate of region (required for mode='region')"
                        },
                        "width": {
                            "type": "number",
                            "description": "Width of region (required for mode='region')"
                        },
                        "height": {
                            "type": "number",
                            "description": "Height of region (required for mode='region')"
                        },
                        "include_ocr": {
                            "type": "boolean",
                            "description": "Include OCR text detection with clickable coordinates (default: true)",
                            "default": true
                        }
                    }
                }))),
            ),
            Tool::new(
                "list_windows",
                "List all visible windows on screen with their IDs, titles, app names, and bounds.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "app_name": {
                            "type": "string",
                            "description": "Filter windows by application name (optional)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "list_apps",
                "List all running applications with their names, bundle IDs, and PIDs.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "app_name": {
                            "type": "string",
                            "description": "Filter by application name (case-insensitive substring match)"
                        },
                        "user_apps_only": {
                            "type": "boolean",
                            "description": "Only return user-facing apps (excludes system agents, helpers, and daemons)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "focus_window",
                "Bring a window or application to the front. Specify window_id, app_name, or pid.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "window_id": {
                            "type": "integer",
                            "description": "Window ID to focus"
                        },
                        "app_name": {
                            "type": "string",
                            "description": "Application name to focus"
                        },
                        "pid": {
                            "type": "integer",
                            "description": "Process ID to focus"
                        }
                    }
                }))),
            ),
            Tool::new(
                "launch_app",
                "Launch an application by name. On macOS, finds apps in /Applications and other standard locations. If the app is already running and no args are provided, brings it to the front. If args are provided and the app is already running, returns an error — use quit_app first.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["app_name"],
                    "properties": {
                        "app_name": {
                            "type": "string",
                            "description": "Application name to launch (e.g., 'Calculator', 'Safari')"
                        },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "CLI arguments to pass to the app (e.g., ['--remote-debugging-port=9222']). Only applied on fresh launch — if the app is already running, returns an error."
                        }
                    }
                }))),
            ),
            Tool::new(
                "quit_app",
                "Quit a running application by name. Graceful by default (app can save state). Use force=true to kill immediately.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["app_name"],
                    "properties": {
                        "app_name": {
                            "type": "string",
                            "description": "Application name to quit (e.g., 'Calculator', 'Safari')"
                        },
                        "force": {
                            "type": "boolean",
                            "description": "Force kill instead of graceful quit (default: false)"
                        }
                    }
                }))),
            ),
            // System-level input tools (CGEvent on macOS, SendInput on Windows)
            Tool::new(
                "click",
                "Click at screen coordinates. Works with any app (egui, Electron, etc.). Supports screenshot metadata for deterministic conversion. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Screen X coordinate"
                        },
                        "y": {
                            "type": "number",
                            "description": "Screen Y coordinate"
                        },
                        "window_x": {
                            "type": "number",
                            "description": "X coordinate relative to window (use with window_id)"
                        },
                        "window_y": {
                            "type": "number",
                            "description": "Y coordinate relative to window (use with window_id)"
                        },
                        "window_id": {
                            "type": "integer",
                            "description": "Window ID for window-relative coordinates"
                        },
                        "screenshot_x": {
                            "type": "number",
                            "description": "X pixel coordinate from screenshot (use with screenshot_origin_* + screenshot_scale or screenshot_window_id)"
                        },
                        "screenshot_y": {
                            "type": "number",
                            "description": "Y pixel coordinate from screenshot (use with screenshot_origin_* + screenshot_scale or screenshot_window_id)"
                        },
                        "screenshot_origin_x": {
                            "type": "number",
                            "description": "Screenshot origin X (from take_screenshot metadata)"
                        },
                        "screenshot_origin_y": {
                            "type": "number",
                            "description": "Screenshot origin Y (from take_screenshot metadata)"
                        },
                        "screenshot_scale": {
                            "type": "number",
                            "description": "Screenshot scale factor (from take_screenshot metadata)"
                        },
                        "screenshot_window_id": {
                            "type": "integer",
                            "description": "Window ID the screenshot was taken from (legacy: lookup window at click time)"
                        },
                        "button": {
                            "type": "string",
                            "enum": ["left", "right", "center"],
                            "description": "Mouse button (default: left)"
                        },
                        "click_count": {
                            "type": "integer",
                            "description": "Number of clicks (1=single, 2=double)",
                            "default": 1
                        }
                    }
                }))),
            ),
            Tool::new(
                "move_mouse",
                "Move mouse cursor to screen coordinates. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Screen X coordinate"
                        },
                        "y": {
                            "type": "number",
                            "description": "Screen Y coordinate"
                        }
                    }
                }))),
            ),
            Tool::new(
                "drag",
                "Drag from one point to another. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["start_x", "start_y", "end_x", "end_y"],
                    "properties": {
                        "start_x": {
                            "type": "number",
                            "description": "Start X coordinate"
                        },
                        "start_y": {
                            "type": "number",
                            "description": "Start Y coordinate"
                        },
                        "end_x": {
                            "type": "number",
                            "description": "End X coordinate"
                        },
                        "end_y": {
                            "type": "number",
                            "description": "End Y coordinate"
                        },
                        "button": {
                            "type": "string",
                            "enum": ["left", "right", "center"],
                            "description": "Mouse button (default: left)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "scroll",
                "Scroll at a position. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y", "delta_y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Screen X coordinate to scroll at"
                        },
                        "y": {
                            "type": "number",
                            "description": "Screen Y coordinate to scroll at"
                        },
                        "delta_x": {
                            "type": "integer",
                            "description": "Horizontal scroll amount (positive=right)",
                            "default": 0
                        },
                        "delta_y": {
                            "type": "integer",
                            "description": "Vertical scroll amount (negative=up, positive=down)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "type_text",
                "Type text at the current cursor position. Works with any app. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        }
                    }
                }))),
            ),
            Tool::new(
                "press_key",
                "Press a key combination. Works with any app. Requires Accessibility permission on macOS.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key to press (e.g., 'return', 'tab', 'escape', 'a', 'f1', 'left', 'up')"
                        },
                        "modifiers": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Modifier keys: 'shift', 'control', 'option', 'command'",
                            "default": []
                        }
                    }
                }))),
            ),
            Tool::new(
                "get_displays",
                "Get information about all connected displays including bounds, scale factors, and resolution.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "find_text",
                "PREFERRED for clicking buttons/labels by name. Finds text on screen using the platform accessibility API (macOS Accessibility, Windows UI Automation) with OCR fallback, and returns screen coordinates ready for the click tool. Use this instead of visually estimating coordinates from screenshots. Can be scoped to a specific app window for faster, more precise results. Note: accessibility results use semantic element names (e.g., 'All Clear' instead of 'AC', 'Subtract' instead of '\u{2212}'), so search by meaning rather than displayed symbol. When no matches are found, the response includes an available_elements array listing all UI element names in the target window — use this to find the correct name and retry. Requires macOS 10.15+ or Windows 10 1903+.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to search for (case-insensitive substring match). Matches against accessibility element names first (e.g., 'All Clear', 'Subtract'), then falls back to OCR on visible text."
                        },
                        "app_name": {
                            "type": "string",
                            "description": "Application name to scope the search to a specific app's window (e.g., 'Calculator'). Faster and avoids false matches from other windows."
                        },
                        "window_id": {
                            "type": "integer",
                            "description": "Window ID to scope the search to a specific window"
                        },
                        "display_id": {
                            "type": "integer",
                            "description": "Display ID to search on. Use get_displays to list available displays. If omitted, searches the main display. Ignored when window_id or app_name is provided."
                        },
                        "uses_language_correction": {
                            "type": "boolean",
                            "description": "Enable language correction for better word accuracy in OCR fallback. Default is false, which is better for UI automation (buttons, labels, single characters). Has no effect when results come from the accessibility API."
                        }
                    }
                }))),
            ),
            Tool::new(
                "element_at_point",
                "Given screen coordinates (x, y), return the accessibility element at that point (name, role, label, value, bounds, pid, app_name). Optional app_name param to scope the lookup to a specific application for faster, more precise results.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Screen X coordinate"
                        },
                        "y": {
                            "type": "number",
                            "description": "Screen Y coordinate"
                        },
                        "app_name": {
                            "type": "string",
                            "description": "Application name to scope the lookup to a specific app (e.g., 'Calculator'). Faster and avoids ambiguity at window edges."
                        }
                    }
                }))),
            ),
            Tool::new(
                "find_image",
                "Find a template image within a screenshot using template matching. Returns precise click coordinates for non-text UI elements like icons and shapes. Use screenshot_id from take_screenshot or provide screenshot_image_base64. Use template_id from load_image or provide template_image_base64.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "screenshot_id": {
                            "type": "string",
                            "description": "Screenshot ID from a previous take_screenshot call (preferred)"
                        },
                        "screenshot_image_base64": {
                            "type": "string",
                            "description": "Base64-encoded screenshot image (used if no screenshot_id)"
                        },
                        "template_id": {
                            "type": "string",
                            "description": "Image ID from a previous load_image call (preferred over template_image_base64)"
                        },
                        "template_image_base64": {
                            "type": "string",
                            "description": "Base64-encoded template image to find (used if no template_id)"
                        },
                        "mask_id": {
                            "type": "string",
                            "description": "Image ID from a previous load_image call for the mask"
                        },
                        "mask_image_base64": {
                            "type": "string",
                            "description": "Base64-encoded mask image (optional; white=match, black=ignore)"
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["fast", "accurate"],
                            "description": "Matching mode: 'fast' (default) for quick searches, 'accurate' for thorough matching",
                            "default": "fast"
                        },
                        "threshold": {
                            "type": "number",
                            "description": "Minimum match score 0.0-1.0 (default: 0.75)"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum matches to return (default: 3 fast, 5 accurate)"
                        },
                        "scales": {
                            "type": "object",
                            "description": "Scale search range {min, max, step}",
                            "properties": {
                                "min": { "type": "number", "default": 0.8 },
                                "max": { "type": "number", "default": 1.2 },
                                "step": { "type": "number", "default": 0.1 }
                            }
                        },
                        "search_region": {
                            "type": "object",
                            "description": "Limit search to region {x, y, w, h} in screenshot pixels",
                            "properties": {
                                "x": { "type": "integer" },
                                "y": { "type": "integer" },
                                "w": { "type": "integer" },
                                "h": { "type": "integer" }
                            }
                        },
                        "stride": {
                            "type": "integer",
                            "description": "Search step size (default: 2 fast, 1 accurate)"
                        },
                        "rotations": {
                            "type": "array",
                            "items": { "type": "number" },
                            "description": "Rotations to try in degrees (only 0, 90, 180, 270 supported)"
                        },
                        "return_screen_coords": {
                            "type": "boolean",
                            "description": "Include screen coordinates for clicking (default: true)",
                            "default": true
                        }
                    }
                }))),
            ),
            Tool::new(
                "load_image",
                "Load an image from a local file path and cache it for use with find_image. Returns an image_id that can be passed to find_image as template_id or mask_id. This avoids manually base64-encoding images.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Local filesystem path to the image file"
                        },
                        "id_prefix": {
                            "type": "string",
                            "description": "Optional prefix for the generated ID (e.g., 'template', 'mask')"
                        },
                        "max_width": {
                            "type": "integer",
                            "description": "Maximum width to downscale to (maintains aspect ratio)"
                        },
                        "max_height": {
                            "type": "integer",
                            "description": "Maximum height to downscale to (maintains aspect ratio)"
                        },
                        "as_mask": {
                            "type": "boolean",
                            "description": "If true, convert to single-channel grayscale mask",
                            "default": false
                        },
                        "return_base64": {
                            "type": "boolean",
                            "description": "If true, include base64-encoded image data in response",
                            "default": false
                        }
                    }
                }))),
            ),
            Tool::new(
                "take_ax_snapshot",
                "Take an accessibility tree snapshot of an application. Returns a structured text representation with unique element IDs, roles, names, and state attributes. Works for any app without requiring a debug port.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "app_name": {
                            "type": "string",
                            "description": "Application name (defaults to frontmost app if omitted)"
                        }
                    }
                }))),
            ),
        ]
    }

    /// The app_connect tool - always available to initiate connections
    fn get_app_connect_tool() -> Tool {
        Tool::new(
            "app_connect",
            "Connect to an app's debug server via WebSocket. The app must have AppDebugKit embedded.",
            Arc::new(json_to_object(serde_json::json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "WebSocket URL (e.g., ws://127.0.0.1:9222)"
                    },
                    "expected_bundle_id": {
                        "type": "string",
                        "description": "Expected bundle ID (e.g., com.example.MyApp). Connection fails if mismatch."
                    },
                    "expected_app_name": {
                        "type": "string",
                        "description": "Expected app name (case-insensitive). Connection fails if mismatch."
                    }
                }
            }))),
        )
    }

    /// App debug tools - only available when connected to an app
    fn get_app_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "app_disconnect",
                "Disconnect from the app's debug server.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "app_get_info",
                "Get runtime info from the connected app (name, bundle ID, version, etc.).",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "app_get_tree",
                "Get the view hierarchy from the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "depth": {
                            "type": "integer",
                            "description": "Max depth to traverse (-1 for unlimited)",
                            "default": 5
                        },
                        "root_id": {
                            "type": "string",
                            "description": "Element ID to start from (optional, defaults to key window)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_query",
                "Find elements matching a CSS-like selector in the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": {
                            "type": "string",
                            "description": "CSS-like selector (#id, .ClassName, [prop=value])"
                        },
                        "all": {
                            "type": "boolean",
                            "description": "Return all matches (default: first only)",
                            "default": false
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_get_element",
                "Get detailed information about an element by ID.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["element_id"],
                    "properties": {
                        "element_id": {
                            "type": "string",
                            "description": "Element ID to get details for"
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_click",
                "Click an element in the connected app by ID.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["element_id"],
                    "properties": {
                        "element_id": {
                            "type": "string",
                            "description": "Element ID to click"
                        },
                        "click_count": {
                            "type": "integer",
                            "description": "Number of clicks (1 for single, 2 for double)",
                            "default": 1
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_type",
                "Type text into an element in the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        },
                        "element_id": {
                            "type": "string",
                            "description": "Element ID to type into (uses focused element if omitted)"
                        },
                        "clear_first": {
                            "type": "boolean",
                            "description": "Clear existing text first",
                            "default": false
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_press_key",
                "Press a key or key combination in the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key to press (e.g., 'Return', 'Tab', 'Escape')"
                        },
                        "modifiers": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Modifier keys: 'shift', 'control', 'option', 'command'",
                            "default": []
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_focus",
                "Focus an element in the connected app (make it first responder).",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["element_id"],
                    "properties": {
                        "element_id": {
                            "type": "string",
                            "description": "Element ID to focus"
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_screenshot",
                "Take a screenshot of an element or window in the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "element_id": {
                            "type": "string",
                            "description": "Element ID to capture (whole window if omitted)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "app_list_windows",
                "List all windows in the connected app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "app_focus_window",
                "Focus a window in the connected app (make it key and main window).",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["window_id"],
                    "properties": {
                        "window_id": {
                            "type": "string",
                            "description": "Window ID to focus (e.g., 'window-1')"
                        }
                    }
                }))),
            ),
        ]
    }

    /// Android tools that are always available (device discovery and connection)
    fn get_android_base_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "android_list_devices",
                "List all Android devices connected via ADB.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "android_connect",
                "Connect to an Android device by its serial number. Use android_list_devices to find available devices.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["serial"],
                    "properties": {
                        "serial": {
                            "type": "string",
                            "description": "Device serial number (e.g., 'emulator-5554' or a USB device serial)"
                        }
                    }
                }))),
            ),
        ]
    }

    /// Android tools available only when a device is connected
    fn get_android_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "android_disconnect",
                "Disconnect from the current Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "android_screenshot",
                "Take a screenshot of the Android device screen. Returns a base64-encoded JPEG image.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "Optional file path to save the screenshot PNG to instead of returning it inline."
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_click",
                "Tap at screen coordinates on the Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Screen X coordinate"
                        },
                        "y": {
                            "type": "number",
                            "description": "Screen Y coordinate"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_swipe",
                "Swipe from one point to another on the Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["start_x", "start_y", "end_x", "end_y"],
                    "properties": {
                        "start_x": {
                            "type": "number",
                            "description": "Start X coordinate"
                        },
                        "start_y": {
                            "type": "number",
                            "description": "Start Y coordinate"
                        },
                        "end_x": {
                            "type": "number",
                            "description": "End X coordinate"
                        },
                        "end_y": {
                            "type": "number",
                            "description": "End Y coordinate"
                        },
                        "duration_ms": {
                            "type": "integer",
                            "description": "Duration of the swipe in milliseconds (optional, default is instant)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_type_text",
                "Type text on the Android device. Special characters are automatically escaped.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_press_key",
                "Press a key on the Android device by keycode name (e.g., 'KEYCODE_HOME') or numeric code.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key to press (e.g., 'KEYCODE_HOME', 'KEYCODE_BACK', 'KEYCODE_ENTER', or a numeric keycode)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_find_text",
                "Find UI elements on the Android device screen that match the given text. Uses uiautomator to dump the view hierarchy and search for matching elements. Returns coordinates for clicking. When no matches are found, the response includes an available_elements array listing all UI element names on screen — use this to find the correct name and retry.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to search for (case-insensitive substring match against text and content-desc attributes)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_list_apps",
                "List installed apps on the Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "user_apps_only": {
                            "type": "boolean",
                            "description": "Only return user-installed (third-party) apps. Default is false (all packages)."
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_launch_app",
                "Launch an app on the Android device by its package name.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["package_name"],
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "Package name to launch (e.g., 'com.android.settings')"
                        }
                    }
                }))),
            ),
            Tool::new(
                "android_get_display_info",
                "Get display information (size and density) from the Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "android_get_current_activity",
                "Get the currently resumed activity on the Android device.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
        ]
    }

    /// Hover tracking tools. `start_hover_tracking` is always visible.
    /// `get_hover_events` and `stop_hover_tracking` only appear while tracking is active.
    fn get_hover_tracking_tools(tracking_active: bool) -> Vec<Tool> {
        let mut tools = vec![Tool::new(
            "start_hover_tracking",
            "Start tracking hover state changes. Polls cursor position and accessibility element at configurable intervals, recording transitions. Use get_hover_events to retrieve recorded events, and stop_hover_tracking to end the session. Only one tracking session can be active at a time.",
            Arc::new(json_to_object(serde_json::json!({
                "type": "object",
                "properties": {
                    "app_name": {
                        "type": "string",
                        "description": "Scope element lookup to a specific application (e.g., 'Safari'). Faster and avoids ambiguity."
                    },
                    "poll_interval_ms": {
                        "type": "integer",
                        "description": "Polling interval in milliseconds (default: 100)",
                        "default": 100
                    },
                    "max_duration_ms": {
                        "type": "integer",
                        "description": "Auto-stop after this many milliseconds (default: 60000 = 60s)",
                        "default": 60000
                    },
                    "min_dwell_ms": {
                        "type": "integer",
                        "description": "Minimum time (ms) cursor must stay on a new element before recording a transition. Filters out pass-through elements during fast mouse movement. 0 = record every change immediately. (default: 300)",
                        "default": 300
                    }
                }
            }))),
        )];
        if tracking_active {
            tools.push(Tool::new(
                "get_hover_events",
                "Retrieve and drain buffered hover events since the last call. Returns a JSON array of transition events, each with cursor position, element info, timestamp, and dwell time. Events are consumed — subsequent calls return only new events.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ));
            tools.push(Tool::new(
                "stop_hover_tracking",
                "Stop hover tracking and return any remaining buffered events. Ends the background polling task.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ));
        }
        tools
    }

    /// Screen recording tools. `start_recording` always visible,
    /// `stop_recording` only while recording is active.
    fn get_recording_tools(recording_active: bool) -> Vec<Tool> {
        let mut tools = vec![Tool::new(
            "start_recording",
            "Start recording the frontmost app's window at ~5fps. Writes timestamped JPEG frames to the specified output directory. Use stop_recording to end the session and get the frame list.",
            Arc::new(json_to_object(serde_json::json!({
                "type": "object",
                "properties": {
                    "output_dir": {
                        "type": "string",
                        "description": "Directory to write JPEG frames to (created if needed)"
                    },
                    "fps": {
                        "type": "integer",
                        "description": "Frames per second (default: 5)",
                        "default": 5
                    },
                    "max_duration_ms": {
                        "type": "integer",
                        "description": "Auto-stop after this many milliseconds (default: 300000 = 5 min)",
                        "default": 300000
                    }
                },
                "required": ["output_dir"]
            }))),
        )];
        if recording_active {
            tools.push(Tool::new(
                "stop_recording",
                "Stop screen recording and return all frame metadata as a JSON array.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ));
        }
        tools
    }

    #[cfg(feature = "cdp")]
    fn get_cdp_connect_tool() -> Tool {
        Tool::new(
            "cdp_connect",
            "Connect to a Chrome or Electron app via its remote debugging port. The app must be running with --remote-debugging-port=PORT.",
            Arc::new(json_to_object(serde_json::json!({
                "type": "object",
                "required": ["port"],
                "properties": {
                    "port": {
                        "type": "integer",
                        "description": "The remote debugging port number"
                    }
                }
            }))),
        )
    }

    #[cfg(feature = "cdp")]
    fn get_cdp_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "cdp_disconnect",
                "Disconnect from the Chrome/Electron app.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "cdp_take_snapshot",
                "Take an accessibility tree snapshot of the selected browser page. Returns a structured text representation with unique element IDs that can be used with cdp_click and cdp_evaluate_script.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "verbose": {
                            "type": "boolean",
                            "description": "Include all attributes in the snapshot (default: false)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "cdp_evaluate_script",
                "Evaluate a JavaScript function in the selected browser page. Returns the result as JSON. Optionally pass element UIDs from a snapshot as arguments.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["function"],
                    "properties": {
                        "function": {
                            "type": "string",
                            "description": "JavaScript function to evaluate (e.g., '() => document.title' or '(el) => el.innerText')"
                        },
                        "args": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "uid": { "type": "string", "description": "Element UID from cdp_take_snapshot" }
                                }
                            },
                            "description": "Optional element arguments from snapshot UIDs"
                        }
                    }
                }))),
            ),
            Tool::new(
                "cdp_click",
                "Click a DOM element by its UID from a cdp_take_snapshot result. Scrolls the element into view and clicks its center.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["uid"],
                    "properties": {
                        "uid": {
                            "type": "string",
                            "description": "Element UID from cdp_take_snapshot"
                        },
                        "dbl_click": {
                            "type": "boolean",
                            "description": "Double-click instead of single click (default: false)"
                        }
                    }
                }))),
            ),
            Tool::new(
                "cdp_list_pages",
                "List all pages (tabs) in the connected browser. Returns page indices for use with cdp_select_page.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))),
            ),
            Tool::new(
                "cdp_select_page",
                "Select a browser page (tab) by index for subsequent CDP operations. Call cdp_list_pages first to see available indices.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["page_idx"],
                    "properties": {
                        "page_idx": {
                            "type": "integer",
                            "description": "Page index from cdp_list_pages"
                        }
                    }
                }))),
            ),
        ]
    }
}

impl ServerHandler for MacOSDevToolsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            server_info: Implementation {
                name: "native-devtools-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "Native DevTools MCP server for automating desktop apps (macOS/Windows) and Android devices.\n\n\
                 WHICH TOOLS TO USE:\n\
                 - Desktop apps: use tools WITHOUT a prefix (click, find_text, take_screenshot, type_text, etc.)\n\
                 - Android devices: use tools WITH the android_ prefix (android_click, android_find_text, etc.)\n\
                 - App debug protocol: use app_* tools only when given a WebSocket URL to connect to\n\
                 NEVER mix these — desktop tools do not work on Android and vice versa.\n\n\
                 == DESKTOP (macOS/Windows) ==\n\n\
                 CLICKING BY TEXT (PREFERRED): Use find_text to locate UI elements by name, \
                 then click at the returned coordinates.\n\
                 Example: find_text(text='Submit') → click(x=..., y=...).\n\n\
                 CLICKING BY VISUAL POSITION: Use take_screenshot with include_ocr=true. \
                 The OCR results include screen coordinates you can click directly. \
                 For positions not covered by OCR, use the screenshot metadata \
                 (origin_x, origin_y, scale) to convert pixel positions.\n\n\
                 Always call focus_window before clicking to ensure the target window receives input.\n\n\
                 Screenshot best practice: Use take_screenshot with app_name (e.g., app_name='Code') \
                 to capture a specific window. Avoid mode='screen' unless you need to see multiple windows.\n\n\
                 App debug protocol (app_* tools): For element-level precision in apps with an embedded \
                 debug server. Use app_connect with a WebSocket URL first, then app_click, app_type, etc.\n\n\
                 == ANDROID ==\n\n\
                 All Android tools require connecting to a device first:\n\
                 1. android_list_devices — find available devices and their serial numbers\n\
                 2. android_connect(serial='...') — connect (this unlocks all other android_* tools)\n\
                 To switch devices, call android_disconnect first, then android_connect to the new device.\n\n\
                 CLICKING BY TEXT (PREFERRED): Use android_find_text to search the accessibility tree, \
                 then android_click at the returned coordinates.\n\
                 Example: android_find_text(text='Settings') → android_click(x=..., y=...).\n\n\
                 CLICKING BY VISUAL POSITION: Use android_screenshot to see the screen, \
                 then android_click at the desired coordinates.\n\
                 Note: android_screenshot has no OCR — always prefer android_find_text for text elements.\n\n\
                 Android coordinates are absolute screen pixels — no scale conversion needed.\n\
                 Use android_press_key with Android keycodes (e.g., 'KEYCODE_BACK', 'KEYCODE_HOME')."
                    .to_string(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: PaginatedRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let connected = self.is_connected().await;
        #[cfg(feature = "cdp")]
        let cdp_connected = self.is_cdp_connected().await;
        #[cfg(not(feature = "cdp"))]
        let cdp_connected = false;
        Ok(ListToolsResult {
            tools: Self::get_tools(
                connected,
                self.is_android_connected().await,
                cdp_connected,
                self.is_hover_tracking().await,
                self.is_recording().await,
            ),
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let args = request
            .arguments
            .map(Value::Object)
            .unwrap_or(Value::Object(Default::default()));

        match request.name.as_ref() {
            "take_screenshot" => {
                let params: screenshot::TakeScreenshotParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(screenshot::take_screenshot(params, Some(self.screenshot_cache.clone())).await)
            }
            "list_windows" => {
                let params: navigation::ListWindowsParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(navigation::list_windows(params))
            }
            "list_apps" => {
                let params: navigation::ListAppsParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(navigation::list_apps(params))
            }
            "focus_window" => {
                let params: navigation::FocusWindowParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(navigation::focus_window(params))
            }
            "launch_app" => {
                let params: navigation::LaunchAppParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(navigation::launch_app(params))
            }
            "quit_app" => {
                let params: navigation::QuitAppParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(navigation::quit_app(params))
            }
            // App Debug Protocol tools
            "app_connect" => {
                let params: app_tools::AppConnectParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                let result =
                    app_tools::app_connect(params, self.app_client.clone(), context.peer).await;
                Ok(result)
            }
            "app_disconnect" => {
                let result = app_tools::app_disconnect(self.app_client.clone(), context.peer).await;
                Ok(result)
            }
            "app_get_info" => Ok(app_tools::app_get_info(self.app_client.clone()).await),
            "app_get_tree" => {
                let params: app_tools::AppGetTreeParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_get_tree(params, self.app_client.clone()).await)
            }
            "app_query" => {
                let params: app_tools::AppQueryParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_query(params, self.app_client.clone()).await)
            }
            "app_get_element" => {
                let params: app_tools::AppGetElementParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_get_element(params, self.app_client.clone()).await)
            }
            "app_click" => {
                let params: app_tools::AppClickParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_click(params, self.app_client.clone()).await)
            }
            "app_type" => {
                let params: app_tools::AppTypeParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_type(params, self.app_client.clone()).await)
            }
            "app_press_key" => {
                let params: app_tools::AppPressKeyParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_press_key(params, self.app_client.clone()).await)
            }
            "app_focus" => {
                let params: app_tools::AppFocusParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_focus(params, self.app_client.clone()).await)
            }
            "app_screenshot" => {
                let params: app_tools::AppScreenshotParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_screenshot(params, self.app_client.clone()).await)
            }
            "app_list_windows" => Ok(app_tools::app_list_windows(self.app_client.clone()).await),
            "app_focus_window" => {
                let params: app_tools::AppFocusWindowParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(app_tools::app_focus_window(params, self.app_client.clone()).await)
            }
            // System-level input tools
            "click" => {
                let params: input_tools::ClickParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::click(params).await)
            }
            "move_mouse" => {
                let params: input_tools::MoveMouseParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::move_mouse(params).await)
            }
            "drag" => {
                let params: input_tools::DragParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::drag(params).await)
            }
            "scroll" => {
                let params: input_tools::ScrollParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::scroll(params).await)
            }
            "type_text" => {
                let params: input_tools::TypeTextParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::type_text(params).await)
            }
            "press_key" => {
                let params: input_tools::PressKeyParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::press_key(params).await)
            }
            "get_displays" => {
                let params: input_tools::GetDisplaysParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::get_displays(params))
            }
            "find_text" => {
                let params: input_tools::FindTextParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::find_text(params))
            }
            "element_at_point" => {
                let params: input_tools::ElementAtPointParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::element_at_point(params))
            }
            "find_image" => {
                let params: find_image::FindImageParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(find_image::find_image(
                    params,
                    self.screenshot_cache.clone(),
                    self.image_cache.clone(),
                )
                .await)
            }
            "load_image" => {
                let params: load_image::LoadImageParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(load_image::load_image(params, self.image_cache.clone()).await)
            }
            "take_ax_snapshot" => {
                let params: crate::tools::ax_snapshot::TakeAxSnapshotParams =
                    serde_json::from_value(args)
                        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                match crate::tools::ax_snapshot::take_ax_snapshot(params) {
                    Ok(snapshot) => Ok(CallToolResult::success(vec![Content::text(snapshot)])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
                }
            }
            // Android tools
            "android_list_devices" => match crate::android::device::list_devices() {
                Ok(devices) => Ok(CallToolResult::success(vec![Content::text(
                    to_json_pretty(&devices),
                )])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            },
            "android_connect" => {
                let serial = parse_string_field(&args, "serial")?;
                match AndroidDevice::connect(&serial) {
                    Ok(device) => {
                        let msg = format!(
                            "Connected to Android device '{}'. Android tools (android_*) are now available.",
                            device.serial
                        );
                        *self.android_device.write().await = Some(device);
                        let _ = context.peer.notify_tool_list_changed().await;
                        Ok(CallToolResult::success(vec![Content::text(msg)]))
                    }
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
                }
            }
            "android_disconnect" => {
                if self.android_device.write().await.take().is_some() {
                    let _ = context.peer.notify_tool_list_changed().await;
                    Ok(CallToolResult::success(vec![Content::text(
                        "Disconnected from Android device. Android tools (android_*) are no longer available.",
                    )]))
                } else {
                    Ok(CallToolResult::error(vec![Content::text(
                        "No Android device connected.",
                    )]))
                }
            }
            "android_screenshot" => {
                let file_path = args
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                Ok(self
                    .with_android_device(|device| {
                        let shot = match crate::android::screenshot::capture(device) {
                            Ok(s) => s,
                            Err(e) => return CallToolResult::error(vec![Content::text(e)]),
                        };
                        if let Some(ref path) = file_path {
                            return match std::fs::write(path, &shot.png_data) {
                                Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                    "Screenshot saved to {} ({}x{})",
                                    path, shot.width, shot.height
                                ))]),
                                Err(e) => CallToolResult::error(vec![Content::text(format!(
                                    "Failed to save screenshot: {}",
                                    e
                                ))]),
                            };
                        }
                        let (image_data, mime_type) = match screenshot::png_to_jpeg(&shot.png_data)
                        {
                            Ok(jpeg_data) => (jpeg_data, "image/jpeg"),
                            Err(e) => {
                                tracing::warn!("JPEG conversion failed, using PNG: {}", e);
                                (shot.png_data, "image/png")
                            }
                        };
                        let base64_data =
                            base64::engine::general_purpose::STANDARD.encode(&image_data);
                        let mut contents = vec![Content::image(base64_data, mime_type)];
                        contents.push(Content::text(to_json_pretty(&serde_json::json!({
                            "width": shot.width,
                            "height": shot.height,
                            "scale": 1.0,
                            "device": device.serial,
                        }))));
                        CallToolResult::success(contents)
                    })
                    .await)
            }
            "android_click" => {
                let (x, y) = parse_xy(&args)?;
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::input::click(device, x, y) {
                            Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                "Tapped at ({:.0}, {:.0})",
                                x, y
                            ))]),
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_swipe" => {
                #[derive(serde::Deserialize)]
                struct Params {
                    start_x: f64,
                    start_y: f64,
                    end_x: f64,
                    end_y: f64,
                    duration_ms: Option<u32>,
                }
                let p: Params = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::input::swipe(
                            device,
                            p.start_x,
                            p.start_y,
                            p.end_x,
                            p.end_y,
                            p.duration_ms,
                        ) {
                            Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                "Swiped from ({:.0}, {:.0}) to ({:.0}, {:.0})",
                                p.start_x, p.start_y, p.end_x, p.end_y
                            ))]),
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_type_text" => {
                let text = parse_string_field(&args, "text")?;
                let len = text.len();
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::input::type_text(device, &text) {
                            Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                "Typed {} characters",
                                len
                            ))]),
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_press_key" => {
                let key = parse_string_field(&args, "key")?;
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::input::press_key(device, &key) {
                            Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                "Pressed key: {}",
                                key
                            ))]),
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_find_text" => {
                let text = parse_string_field(&args, "text")?;
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::ui_automator::find_text(device, &text) {
                            Ok(result) => {
                                let mut content =
                                    vec![Content::text(to_json_pretty(&result.matches))];
                                if result.matches.is_empty() {
                                    content.push(Content::text(
                                        input_tools::build_no_matches_hint(
                                            &text,
                                            &result.available_elements,
                                        ),
                                    ));
                                }
                                CallToolResult::success(content)
                            }
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_list_apps" => {
                let user_apps_only = args
                    .get("user_apps_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::navigation::list_apps(device, user_apps_only) {
                            Ok(apps) => {
                                CallToolResult::success(vec![Content::text(to_json_pretty(&apps))])
                            }
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_launch_app" => {
                let package_name = parse_string_field(&args, "package_name")?;
                Ok(self
                    .with_android_device(|device| {
                        match crate::android::navigation::launch_app(device, &package_name) {
                            Ok(()) => CallToolResult::success(vec![Content::text(format!(
                                "Launched {}",
                                package_name
                            ))]),
                            Err(e) => CallToolResult::error(vec![Content::text(e)]),
                        }
                    })
                    .await)
            }
            "android_get_display_info" => Ok(self
                .with_android_device(|device| {
                    match crate::android::navigation::get_display_info(device) {
                        Ok(info) => {
                            CallToolResult::success(vec![Content::text(to_json_pretty(&info))])
                        }
                        Err(e) => CallToolResult::error(vec![Content::text(e)]),
                    }
                })
                .await),
            "android_get_current_activity" => Ok(self
                .with_android_device(
                    |device| match crate::android::navigation::get_current_activity(device) {
                        Ok(activity) => {
                            CallToolResult::success(vec![Content::text(to_json_pretty(&activity))])
                        }
                        Err(e) => CallToolResult::error(vec![Content::text(e)]),
                    },
                )
                .await),
            "start_hover_tracking" => {
                // Auto-clean finished tracker (e.g. from max duration timeout)
                let already_active = {
                    let guard = self.hover_tracker.read().await;
                    match guard.as_ref() {
                        Some(t) if t.is_finished() => false, // will clean up below
                        Some(_) => true,
                        None => false,
                    }
                };
                if already_active {
                    return Ok(CallToolResult::error(vec![Content::text(
                        "Hover tracking is already active. Use stop_hover_tracking to end the current session first.",
                    )]));
                }
                // Clean up any finished tracker before starting a new one
                if self.hover_tracker.read().await.is_some() {
                    self.hover_tracker.write().await.take();
                }

                let app_name = args
                    .get("app_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let poll_interval_ms = args
                    .get("poll_interval_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .clamp(10, 10_000) as u32;
                let max_duration_ms = args
                    .get("max_duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60000)
                    .clamp(100, u32::MAX as u64) as u32;
                let min_dwell_ms = args
                    .get("min_dwell_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(300)
                    .clamp(0, 10_000) as u32;

                let events = Arc::new(std::sync::Mutex::new(Vec::new()));
                let cancel = tokio_util::sync::CancellationToken::new();

                let task_handle = crate::tools::hover_tracker::start_polling(
                    events.clone(),
                    cancel.clone(),
                    app_name.clone(),
                    poll_interval_ms,
                    max_duration_ms,
                    min_dwell_ms,
                );

                let tracker =
                    crate::tools::hover_tracker::HoverTracker::new(events, task_handle, cancel);
                *self.hover_tracker.write().await = Some(tracker);
                let _ = context.peer.notify_tool_list_changed().await;

                let msg = format!(
                    "Hover tracking started (poll: {}ms, max: {}ms, dwell: {}ms{}). Use get_hover_events to read transitions, stop_hover_tracking to end.",
                    poll_interval_ms,
                    max_duration_ms,
                    min_dwell_ms,
                    app_name.map_or(String::new(), |a| format!(", app: {}", a)),
                );
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            "get_hover_events" => {
                // Single lock: check auto-stop and drain events together
                let result = {
                    let guard = self.hover_tracker.read().await;
                    guard.as_ref().map(|t| {
                        let auto_stopped = t.is_finished();
                        let events = t.drain_events();
                        (auto_stopped, events)
                    })
                };

                match result {
                    Some((auto_stopped, events)) => {
                        let json = to_json_pretty(&events);

                        if auto_stopped {
                            self.hover_tracker.write().await.take();
                            let _ = context.peer.notify_tool_list_changed().await;
                        }

                        // Always return the JSON array for consistent parsing.
                        // The timeout sentinel event (with timeout: true) signals
                        // auto-stop within the event stream itself.
                        Ok(CallToolResult::success(vec![Content::text(json)]))
                    }
                    None => Ok(CallToolResult::error(vec![Content::text(
                        "No hover tracking session is active. Use start_hover_tracking first.",
                    )])),
                }
            }
            "stop_hover_tracking" => {
                let tracker = self.hover_tracker.write().await.take();
                match tracker {
                    Some(tracker) => {
                        let events = tracker.cancel_and_drain().await;
                        let _ = context.peer.notify_tool_list_changed().await;
                        // Return raw JSON array for consistent parsing with get_hover_events
                        Ok(CallToolResult::success(vec![Content::text(
                            to_json_pretty(&events),
                        )]))
                    }
                    None => Ok(CallToolResult::error(vec![Content::text(
                        "No hover tracking session is active.",
                    )])),
                }
            }
            "start_recording" => {
                // Check if a previous recording auto-stopped (max_duration elapsed).
                // If so, drain remaining frames (log count) and clear stale state.
                {
                    let guard = self.screen_recorder.read().await;
                    if let Some(recorder) = guard.as_ref() {
                        if recorder.is_finished() {
                            let stale_count = recorder.drain_frames().len();
                            if stale_count > 0 {
                                tracing::warn!(
                                    "Discarding {stale_count} frames from auto-stopped recording \
                                     (stop_recording was not called)"
                                );
                            }
                            drop(guard);
                            self.screen_recorder.write().await.take();
                            let _ = context.peer.notify_tool_list_changed().await;
                        } else {
                            return Ok(CallToolResult::error(vec![Content::text(
                                "Recording is already active. Use stop_recording to end the current session first.",
                            )]));
                        }
                    }
                }

                let output_dir = parse_string_field(&args, "output_dir")?;
                let fps = args.get("fps").and_then(|v| v.as_u64()).unwrap_or(5) as u32;
                let max_duration_ms = args
                    .get("max_duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(300_000)
                    .clamp(1_000, u32::MAX as u64) as u32;

                let output_path = std::path::PathBuf::from(&output_dir);
                if let Err(e) = std::fs::create_dir_all(&output_path) {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to create output directory: {e}"
                    ))]));
                }
                // Probe-write to fail fast if the directory is not writable.
                match tempfile::tempfile_in(&output_path) {
                    Ok(_) => {} // drops and deletes automatically
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Output directory is not writable: {e}"
                        ))]));
                    }
                }

                let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let cancel = tokio_util::sync::CancellationToken::new();

                let task_handle = crate::tools::screen_recorder::start_recording(
                    frames.clone(),
                    cancel.clone(),
                    output_path,
                    fps,
                    max_duration_ms,
                );

                let recorder =
                    crate::tools::screen_recorder::ScreenRecorder::new(frames, task_handle, cancel);
                *self.screen_recorder.write().await = Some(recorder);
                let _ = context.peer.notify_tool_list_changed().await;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Recording started ({fps}fps, max: {max_duration_ms}ms, dir: {output_dir}). Use stop_recording to end.",
                ))]))
            }
            "stop_recording" => {
                let recorder = self.screen_recorder.write().await.take();
                match recorder {
                    Some(recorder) => {
                        let frames = recorder.cancel_and_drain().await;
                        let _ = context.peer.notify_tool_list_changed().await;
                        Ok(CallToolResult::success(vec![Content::text(
                            to_json_pretty(&frames),
                        )]))
                    }
                    None => Ok(CallToolResult::error(vec![Content::text(
                        "No recording session is active.",
                    )])),
                }
            }
            #[cfg(feature = "cdp")]
            "cdp_connect" => {
                let port = args
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .map(|p| p as u16)
                    .ok_or_else(|| {
                        McpError::invalid_params("missing required param: port", None)
                    })?;
                match crate::cdp::CdpClient::connect(port).await {
                    Ok(client) => {
                        let page_info = if let Some(page) = client.selected_page.as_ref() {
                            let url = page.url().await.ok().flatten().unwrap_or_default();
                            format!("Selected page: {}", url)
                        } else {
                            "No pages found".to_string()
                        };
                        *self.cdp_client.write().await = Some(client);
                        let _ = context.peer.notify_tool_list_changed().await;
                        Ok(CallToolResult::success(vec![Content::text(format!(
                            "Connected to Chrome/Electron on port {}. CDP tools are now available.\n{}",
                            port, page_info
                        ))]))
                    }
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
                }
            }
            #[cfg(feature = "cdp")]
            "cdp_disconnect" => {
                if let Some(client) = self.cdp_client.write().await.take() {
                    client.disconnect();
                    let _ = context.peer.notify_tool_list_changed().await;
                    Ok(CallToolResult::success(vec![Content::text(
                        "Disconnected from Chrome/Electron. CDP tools are no longer available.",
                    )]))
                } else {
                    Ok(CallToolResult::error(vec![Content::text(
                        "No CDP connection active.",
                    )]))
                }
            }
            #[cfg(feature = "cdp")]
            "cdp_take_snapshot" => {
                Ok(crate::cdp::tools::cdp_take_snapshot(self.cdp_client.clone()).await)
            }
            #[cfg(feature = "cdp")]
            "cdp_evaluate_script" => {
                let function = parse_string_field(&args, "function")?;
                let script_args = args.get("args").and_then(|v| v.as_array()).cloned();
                Ok(crate::cdp::tools::cdp_evaluate_script(
                    function,
                    script_args,
                    self.cdp_client.clone(),
                )
                .await)
            }
            #[cfg(feature = "cdp")]
            "cdp_click" => {
                let uid = parse_string_field(&args, "uid")?;
                let dbl_click = args
                    .get("dbl_click")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(crate::cdp::tools::cdp_click(uid, dbl_click, self.cdp_client.clone()).await)
            }
            #[cfg(feature = "cdp")]
            "cdp_list_pages" => {
                Ok(crate::cdp::tools::cdp_list_pages(self.cdp_client.clone()).await)
            }
            #[cfg(feature = "cdp")]
            "cdp_select_page" => {
                let page_idx = args
                    .get("page_idx")
                    .and_then(|v| v.as_u64())
                    .map(|p| p as usize)
                    .ok_or_else(|| {
                        McpError::invalid_params("missing required param: page_idx", None)
                    })?;
                Ok(crate::cdp::tools::cdp_select_page(page_idx, self.cdp_client.clone()).await)
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            )),
        }
    }
}
