use crate::app_protocol::AppProtocolClient;
use crate::tools::{
    app_protocol as app_tools, find_image, image_cache::ImageCache, input as input_tools,
    load_image, navigation, screenshot, screenshot_cache::ScreenshotCache,
};
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
        }
    }

    /// Check if currently connected to an app debug server
    async fn is_connected(&self) -> bool {
        self.app_client.read().await.is_some()
    }

    /// Get tools available based on connection state.
    /// Base tools and app_connect are always available.
    /// Other app_* tools are only available when connected.
    pub fn get_tools(connected: bool) -> Vec<Tool> {
        let mut tools = Self::get_base_tools();
        tools.push(Self::get_app_connect_tool());
        if connected {
            tools.extend(Self::get_app_tools());
        }
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
                    "properties": {}
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
                "PREFERRED for clicking buttons/labels by name. Finds text on screen using OCR and returns screen coordinates ready for the click tool. Use this instead of visually estimating coordinates from screenshots. Requires macOS 10.15+ or Windows 10 1903+.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to search for (case-insensitive)"
                        },
                        "display_id": {
                            "type": "integer",
                            "description": "Display ID to search on. Use get_displays to list available displays. If omitted, searches the main display."
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
                            "description": "Minimum match score 0.0-1.0 (default: 0.88 fast, 0.85 accurate)"
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
                "Native DevTools MCP server for testing native applications on macOS and Windows.\n\n\
                 Two approaches for UI interaction:\n\
                 1. AppDebugKit (app_* tools): For apps with AppDebugKit embedded (macOS only). \
                    Use app_connect first, then app_click, app_type, etc. for element-level precision.\n\
                 2. CGEvent/SendInput (click, type_text, etc.): For any app (egui, Electron, etc.). \
                    Requires Accessibility permission on macOS.\n\n\
                 CLICKING BY TEXT (PREFERRED): When asked to click a button or UI element by name, \
                 use find_text first to get accurate screen coordinates, then click at those coordinates. \
                 Example: find_text(text='Submit') returns coordinates, then click(x=..., y=...). \
                 This is more reliable than visually estimating coordinates from screenshots.\n\n\
                 CLICKING BY VISUAL POSITION: When you need to click at a specific visual location \
                 (not identified by text), use take_screenshot with include_ocr=true. The OCR results \
                 include screen coordinates you can click directly. For positions not covered by OCR, \
                 use the screenshot metadata (origin_x, origin_y, scale) to convert pixel positions.\n\n\
                 IMPORTANT: Always use focus_window before clicking to ensure the target window receives the click.\n\n\
                 Screenshot best practice: Use take_screenshot with app_name (e.g., app_name='Code' for VSCode) \
                 to capture a specific window. Avoid mode='screen' unless you need to see multiple windows."
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
        Ok(ListToolsResult {
            tools: Self::get_tools(connected),
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
            _ => Err(McpError::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            )),
        }
    }
}
