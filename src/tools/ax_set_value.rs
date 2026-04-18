//! `ax_set_value` tool — write `kAXValueAttribute` on an element resolved
//! from a generation-tagged uid. This is value assignment, not key-event
//! typing (no IME, no undo stack, no keydown/keyup events). See design doc.

use crate::macos::ax::{element_bbox, set_value_attribute, AXDispatchError, AXRef};
use crate::tools::ax_session::{AxSession, LookupError};
use crate::tools::ax_snapshot::Rect;
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct AxSetValueParams {
    pub uid: String,
    pub text: String,
}

pub(crate) fn success_result(bbox: Option<Rect>) -> CallToolResult {
    let body = match bbox {
        Some(r) => json!({
            "ok": true,
            "dispatched_via": "AXSetAttributeValue",
            "bbox": { "x": r.x, "y": r.y, "w": r.w, "h": r.h }
        }),
        None => json!({ "ok": true, "dispatched_via": "AXSetAttributeValue" }),
    };
    CallToolResult::success(vec![Content::text(body.to_string())])
}

pub(crate) fn error_result(code: &str, message: &str, fallback: Option<Rect>) -> CallToolResult {
    let fb = match fallback {
        Some(r) => json!({ "x": r.x + r.w / 2.0, "y": r.y + r.h / 2.0 }),
        None => serde_json::Value::Null,
    };
    let body = json!({ "error": { "code": code, "message": message, "fallback": fb } });
    CallToolResult::error(vec![Content::text(body.to_string())])
}

pub async fn ax_set_value(params: AxSetValueParams, session: Arc<AxSession>) -> CallToolResult {
    if !crate::platform::input::check_accessibility_permission() {
        return CallToolResult::error(vec![Content::text(
            "Accessibility permission required.\n\n\
             Grant permission to your MCP client (e.g., Claude Desktop, VS Code, Terminal) in:\n\
             System Settings → Privacy & Security → Accessibility\n\n\
             The permission must be granted to the app that runs this MCP server, \
             not to the server binary itself."
                .to_string(),
        )]);
    }

    let ax_ref: AXRef = match session.lookup(&params.uid).await {
        Ok(r) => r,
        Err(LookupError::SnapshotExpired { reason }) => {
            return error_result("snapshot_expired", &reason, None);
        }
        Err(LookupError::UidNotFound) => {
            return error_result(
                "uid_not_found",
                &format!("uid {} is not present in the current snapshot", params.uid),
                None,
            );
        }
    };

    let pre_bbox = unsafe { element_bbox(ax_ref.as_raw()) };

    match set_value_attribute(&ax_ref, &params.text) {
        Ok(()) => {
            // Success bbox is the post-dispatch bbox for telemetry; snapshot
            // bbox remains the pre-action animation source (see design
            // doc §Data flow). Fall back to pre-bbox if the element no
            // longer exposes position+size.
            let post_bbox = unsafe { element_bbox(ax_ref.as_raw()) }.or(pre_bbox);
            success_result(post_bbox)
        }
        Err(AXDispatchError::NotDispatchable) => error_result(
            "not_dispatchable",
            "element does not expose a writable kAXValueAttribute",
            pre_bbox,
        ),
        Err(AXDispatchError::AxError(code)) => error_result(
            "ax_error",
            &format!("AXUIElementSetAttributeValue failed with AX error {}", code),
            pre_bbox,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_body(r: &CallToolResult) -> String {
        r.content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn success_reports_set_attribute_value() {
        let r = success_result(Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 24.0,
        }));
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["dispatched_via"], "AXSetAttributeValue");
    }

    #[test]
    fn error_fallback_centers_correctly() {
        let r = error_result(
            "not_dispatchable",
            "m",
            Some(Rect {
                x: 10.0,
                y: 20.0,
                w: 100.0,
                h: 40.0,
            }),
        );
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["error"]["fallback"]["x"], 60.0);
        assert_eq!(body["error"]["fallback"]["y"], 40.0);
    }
}
