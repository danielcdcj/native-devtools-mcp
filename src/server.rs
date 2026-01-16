use crate::app_protocol::AppProtocolClient;
use crate::tools::{app_protocol as app_tools, input as input_tools, navigation, screenshot};
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
}

impl MacOSDevToolsServer {
    pub fn new() -> Self {
        Self {
            app_client: Arc::new(RwLock::new(None)),
        }
    }

    fn get_tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "take_screenshot",
                "Capture a screenshot of the screen, a specific window, or a region. Returns a base64-encoded PNG image.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["screen", "window", "region"],
                            "description": "Capture mode: 'screen' for full screen, 'window' for specific window, 'region' for rectangular area",
                            "default": "screen"
                        },
                        "window_id": {
                            "type": "integer",
                            "description": "Window ID to capture (required for mode='window')"
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
            // App Debug Protocol tools
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
                        }
                    }
                }))),
            ),
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
            // System-level input tools (CGEvent-based, works with any app)
            Tool::new(
                "click",
                "Click at screen coordinates using CGEvent. Works with any app (egui, Electron, etc.). Requires Accessibility permission.",
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
                            "description": "X pixel coordinate from screenshot (use with screenshot_window_id)"
                        },
                        "screenshot_y": {
                            "type": "number",
                            "description": "Y pixel coordinate from screenshot (use with screenshot_window_id)"
                        },
                        "screenshot_window_id": {
                            "type": "integer",
                            "description": "Window ID the screenshot was taken from (for coordinate scaling)"
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
                "Move mouse cursor to screen coordinates. Requires Accessibility permission.",
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
                "Drag from one point to another. Requires Accessibility permission.",
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
                "Scroll at a position. Requires Accessibility permission.",
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
                "Type text at the current cursor position using CGEvent. Works with any app. Requires Accessibility permission.",
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
                "Press a key combination using CGEvent. Works with any app. Requires Accessibility permission.",
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
                "Find text on screen using OCR. Returns screen coordinates for clicking. Requires: brew install tesseract",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to search for (case-insensitive)"
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
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "native-devtools-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "macOS DevTools MCP server for testing native applications.\n\n\
                 Two approaches for UI interaction:\n\
                 1. AppDebugKit (app_* tools): For apps with AppDebugKit embedded. \
                    Use app_connect first, then app_click, app_type, etc. for element-level precision.\n\
                 2. CGEvent (click, type_text, etc.): For any app (egui, Electron, etc.). \
                    Use take_screenshot to see UI, then click at coordinates. Requires Accessibility permission.\n\n\
                 Use get_displays to understand screen layout and coordinate systems."
                    .to_string(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: PaginatedRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: Self::get_tools(),
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let args = request
            .arguments
            .map(Value::Object)
            .unwrap_or(Value::Object(Default::default()));

        match request.name.as_ref() {
            "take_screenshot" => {
                let params: screenshot::TakeScreenshotParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(screenshot::take_screenshot(params))
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
                Ok(app_tools::app_connect(params, self.app_client.clone()).await)
            }
            "app_disconnect" => Ok(app_tools::app_disconnect(self.app_client.clone()).await),
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
            // System-level input tools (CGEvent-based)
            "click" => {
                let params: input_tools::ClickParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::click(params))
            }
            "move_mouse" => {
                let params: input_tools::MoveMouseParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::move_mouse(params))
            }
            "drag" => {
                let params: input_tools::DragParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::drag(params))
            }
            "scroll" => {
                let params: input_tools::ScrollParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::scroll(params))
            }
            "type_text" => {
                let params: input_tools::TypeTextParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::type_text(params))
            }
            "press_key" => {
                let params: input_tools::PressKeyParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                Ok(input_tools::press_key(params))
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
            _ => Err(McpError::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            )),
        }
    }
}
