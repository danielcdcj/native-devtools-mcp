//! `ax_click` tool — dispatch `AXPress` against an element resolved from a
//! generation-tagged uid.

use crate::macos::ax::{element_bbox, press_element, AXDispatchError, AXRef};
use crate::tools::ax_session::{AxSession, LookupError};
use crate::tools::ax_snapshot::Rect;
use rmcp::model::{CallToolResult, Content};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct AxClickParams {
    pub uid: String,
}

/// Build a structured success response: `{ ok: true, dispatched_via, bbox? }`.
pub(crate) fn success_result(bbox: Option<Rect>) -> CallToolResult {
    let body = match bbox {
        Some(r) => json!({
            "ok": true,
            "dispatched_via": "AXPress",
            "bbox": { "x": r.x, "y": r.y, "w": r.w, "h": r.h }
        }),
        None => json!({ "ok": true, "dispatched_via": "AXPress" }),
    };
    CallToolResult::success(vec![Content::text(body.to_string())])
}

/// Build a structured error response.
pub(crate) fn error_result(code: &str, message: &str, fallback: Option<Rect>) -> CallToolResult {
    let fb = match fallback {
        Some(r) => json!({ "x": r.x + r.w / 2.0, "y": r.y + r.h / 2.0 }),
        None => serde_json::Value::Null,
    };
    let body = json!({ "error": { "code": code, "message": message, "fallback": fb } });
    CallToolResult::error(vec![Content::text(body.to_string())])
}

/// Handle an `ax_click` tool call.
///
/// Returns `CallToolResult::success` with `{ ok: true, dispatched_via, bbox }`
/// on success, or `CallToolResult::error` with `{ error: { code, message,
/// fallback } }` on any dispatch failure. Accessibility-permission failures
/// use the same plain-text path as the coord-based `click` / `type_text`
/// tools for consistency with clickweave's error UI.
pub async fn ax_click(params: AxClickParams, session: Arc<AxSession>) -> CallToolResult {
    // Permission first — use the same plain-text path as `click` / `type_text`.
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

    // Resolve uid.
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

    // Capture the pre-dispatch bbox so we can still return a fallback even if
    // the element disappears mid-dispatch.
    let pre_bbox = unsafe { element_bbox(ax_ref.as_raw()) };

    match press_element(&ax_ref) {
        Ok(()) => {
            // Per design (§Data flow): success bbox is the element's CURRENT
            // (post-dispatch) bbox for telemetry / logging of the actual
            // dispatch site. Snapshot bbox remains the pre-action source for
            // overlay animation clickweave-side. Fall back to the pre-bbox
            // if the element no longer exposes position+size post-dispatch.
            let post_bbox = unsafe { element_bbox(ax_ref.as_raw()) }.or(pre_bbox);
            success_result(post_bbox)
        }
        Err(AXDispatchError::NotDispatchable) => error_result(
            "not_dispatchable",
            "element does not support AXPress",
            pre_bbox,
        ),
        Err(AXDispatchError::AxError(code)) => error_result(
            "ax_error",
            &format!("AXUIElementPerformAction failed with AX error {}", code),
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
    fn success_with_bbox_serializes_expected_shape() {
        let r = success_result(Some(Rect {
            x: 412.0,
            y: 285.0,
            w: 64.0,
            h: 32.0,
        }));
        assert_eq!(r.is_error, Some(false));
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["dispatched_via"], "AXPress");
        assert_eq!(body["bbox"]["x"], 412.0);
        assert_eq!(body["bbox"]["y"], 285.0);
        assert_eq!(body["bbox"]["w"], 64.0);
        assert_eq!(body["bbox"]["h"], 32.0);
    }

    #[test]
    fn success_without_bbox_omits_bbox_field() {
        let r = success_result(None);
        assert_eq!(r.is_error, Some(false));
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["dispatched_via"], "AXPress");
        assert!(body.get("bbox").is_none());
    }

    #[test]
    fn error_with_fallback_centers_on_bbox() {
        let r = error_result(
            "not_dispatchable",
            "element does not support AXPress",
            Some(Rect {
                x: 100.0,
                y: 200.0,
                w: 50.0,
                h: 40.0,
            }),
        );
        assert_eq!(r.is_error, Some(true));
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["error"]["code"], "not_dispatchable");
        assert_eq!(body["error"]["message"], "element does not support AXPress");
        // Center = (100 + 50/2, 200 + 40/2) = (125, 220).
        assert_eq!(body["error"]["fallback"]["x"], 125.0);
        assert_eq!(body["error"]["fallback"]["y"], 220.0);
    }

    #[test]
    fn error_without_fallback_renders_null() {
        let r = error_result("snapshot_expired", "stale uid", None);
        assert_eq!(r.is_error, Some(true));
        let body: serde_json::Value = serde_json::from_str(&text_body(&r)).unwrap();
        assert_eq!(body["error"]["code"], "snapshot_expired");
        assert!(body["error"]["fallback"].is_null());
    }
}
