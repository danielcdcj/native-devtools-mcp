use crate::app_protocol::AppProtocolClient;
use crate::tools::{app_protocol as app_tools, input, navigation, screenshot};
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
            Tool::new(
                "click",
                "Click at the specified screen coordinates. Supports left, right, middle clicks and double-clicks. Use synthetic=true to click without moving the mouse cursor.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "X coordinate to click"
                        },
                        "y": {
                            "type": "number",
                            "description": "Y coordinate to click"
                        },
                        "button": {
                            "type": "string",
                            "enum": ["left", "right", "middle"],
                            "description": "Mouse button to click",
                            "default": "left"
                        },
                        "double_click": {
                            "type": "boolean",
                            "description": "Whether to double-click",
                            "default": false
                        },
                        "synthetic": {
                            "type": "boolean",
                            "description": "If true, use Accessibility API to click without moving the mouse cursor",
                            "default": false
                        }
                    }
                }))),
            ),
            Tool::new(
                "type_text",
                "Type a string of text at the current cursor position.",
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
                "Press a key or key combination. Examples: 'Enter', 'Tab', 'Cmd+C', 'Ctrl+Shift+A', 'F5'.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key or key combination to press (e.g., 'Enter', 'Cmd+C')"
                        }
                    }
                }))),
            ),
            Tool::new(
                "scroll",
                "Scroll at the specified screen coordinates. Positive delta_y scrolls down.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "X coordinate for scroll position"
                        },
                        "y": {
                            "type": "number",
                            "description": "Y coordinate for scroll position"
                        },
                        "delta_x": {
                            "type": "integer",
                            "description": "Horizontal scroll amount (positive = right)",
                            "default": 0
                        },
                        "delta_y": {
                            "type": "integer",
                            "description": "Vertical scroll amount (positive = down)",
                            "default": 0
                        }
                    }
                }))),
            ),
            Tool::new(
                "drag",
                "Drag from one screen coordinate to another (left mouse button).",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["from_x", "from_y", "to_x", "to_y"],
                    "properties": {
                        "from_x": {
                            "type": "number",
                            "description": "Starting X coordinate"
                        },
                        "from_y": {
                            "type": "number",
                            "description": "Starting Y coordinate"
                        },
                        "to_x": {
                            "type": "number",
                            "description": "Ending X coordinate"
                        },
                        "to_y": {
                            "type": "number",
                            "description": "Ending Y coordinate"
                        }
                    }
                }))),
            ),
            Tool::new(
                "move_mouse",
                "Move the mouse cursor to the specified screen coordinates (for hover effects).",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "X coordinate"
                        },
                        "y": {
                            "type": "number",
                            "description": "Y coordinate"
                        }
                    }
                }))),
            ),
            Tool::new(
                "wait",
                "Wait for specified milliseconds before continuing.",
                Arc::new(json_to_object(serde_json::json!({
                    "type": "object",
                    "required": ["ms"],
                    "properties": {
                        "ms": {
                            "type": "integer",
                            "description": "Milliseconds to wait"
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
        ]
    }
}

impl ServerHandler for MacOSDevToolsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "macos-devtools-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "macOS DevTools MCP server for testing native applications. \
                 Use take_screenshot to capture the screen, list_windows to see available windows, \
                 and input tools (click, type_text, press_key, etc.) to interact with the UI. \
                 Requires Screen Recording and Accessibility permissions."
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

        let result = match request.name.as_ref() {
            "take_screenshot" => {
                let params: screenshot::TakeScreenshotParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                screenshot::take_screenshot(params)
            }
            "list_windows" => {
                let params: navigation::ListWindowsParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                navigation::list_windows(params)
            }
            "list_apps" => {
                let params: navigation::ListAppsParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                navigation::list_apps(params)
            }
            "focus_window" => {
                let params: navigation::FocusWindowParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                navigation::focus_window(params)
            }
            "click" => {
                let params: input::ClickParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::click(params)
            }
            "type_text" => {
                let params: input::TypeTextParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::type_text(params)
            }
            "press_key" => {
                let params: input::PressKeyParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::press_key(params)
            }
            "scroll" => {
                let params: input::ScrollParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::scroll(params)
            }
            "drag" => {
                let params: input::DragParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::drag(params)
            }
            "move_mouse" => {
                let params: input::MoveMouseParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::move_mouse(params)
            }
            "wait" => {
                let params: input::WaitParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                input::wait(params)
            }
            // App Debug Protocol tools
            "app_connect" => {
                let params: app_tools::AppConnectParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_connect(params, self.app_client.clone()).await);
            }
            "app_disconnect" => {
                return Ok(app_tools::app_disconnect(self.app_client.clone()).await);
            }
            "app_get_info" => {
                return Ok(app_tools::app_get_info(self.app_client.clone()).await);
            }
            "app_get_tree" => {
                let params: app_tools::AppGetTreeParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_get_tree(params, self.app_client.clone()).await);
            }
            "app_query" => {
                let params: app_tools::AppQueryParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_query(params, self.app_client.clone()).await);
            }
            "app_get_element" => {
                let params: app_tools::AppGetElementParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_get_element(params, self.app_client.clone()).await);
            }
            "app_click" => {
                let params: app_tools::AppClickParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_click(params, self.app_client.clone()).await);
            }
            "app_type" => {
                let params: app_tools::AppTypeParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_type(params, self.app_client.clone()).await);
            }
            "app_press_key" => {
                let params: app_tools::AppPressKeyParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_press_key(params, self.app_client.clone()).await);
            }
            "app_focus" => {
                let params: app_tools::AppFocusParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_focus(params, self.app_client.clone()).await);
            }
            "app_screenshot" => {
                let params: app_tools::AppScreenshotParams = serde_json::from_value(args)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                return Ok(app_tools::app_screenshot(params, self.app_client.clone()).await);
            }
            "app_list_windows" => {
                return Ok(app_tools::app_list_windows(self.app_client.clone()).await);
            }
            _ => {
                return Err(McpError::invalid_params(
                    format!("Unknown tool: {}", request.name),
                    None,
                ));
            }
        };

        Ok(result)
    }
}
