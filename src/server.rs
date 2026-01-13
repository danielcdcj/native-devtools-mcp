use crate::tools::{input, navigation, screenshot};
use rmcp::{
    Error as McpError,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, Implementation, ListToolsResult,
        PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
};
use serde_json::Value;
use std::sync::Arc;

fn json_to_object(value: Value) -> rmcp::model::JsonObject {
    match value {
        Value::Object(map) => map,
        _ => Default::default(),
    }
}

#[derive(Clone)]
pub struct MacOSDevToolsServer;

impl MacOSDevToolsServer {
    pub fn new() -> Self {
        Self
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
                "Click at the specified screen coordinates. Supports left, right, middle clicks and double-clicks.",
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

    fn list_tools(
        &self,
        _request: PaginatedRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListToolsResult {
                tools: Self::get_tools(),
                next_cursor: None,
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let args = request
                .arguments
                .map(|m| Value::Object(m))
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
}
