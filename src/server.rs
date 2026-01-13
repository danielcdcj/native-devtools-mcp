use crate::app_protocol::AppProtocolClient;
use crate::tools::{app_protocol as app_tools, navigation, screenshot};
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
                 Connect to apps with AppDebugKit embedded using app_connect, then use \
                 app_* tools to interact with the UI. Use take_screenshot and list_windows \
                 for visual verification."
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
            _ => Err(McpError::invalid_params(
                format!("Unknown tool: {}", request.name),
                None,
            )),
        }
    }
}
