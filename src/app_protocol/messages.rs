use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request sent to app's debug server
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolRequest {
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// Response from app's debug server
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<ProtocolError>,
}

/// Error in protocol response
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolError {
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// Rect structure matching the Swift side
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// View node in the hierarchy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewNode {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: String,
    pub class_name: String,
    pub frame: Rect,
    pub bounds: Rect,
    pub is_hidden: bool,
    pub is_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ViewNode>,
}

/// Window information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub frame: Rect,
    pub is_key: bool,
    pub is_main: bool,
    pub is_visible: bool,
}

/// Runtime info from the app
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub app_name: String,
    pub bundle_id: String,
    pub version: String,
    pub protocol_version: String,
    pub pid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_window_id: Option<String>,
}

/// Screenshot result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotResult {
    pub data: String,
    pub width: i32,
    pub height: i32,
}
