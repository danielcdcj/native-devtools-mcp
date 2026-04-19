//! `ax_click` tool — dispatch `AXPress` against an element resolved from a
//! generation-tagged uid.

use crate::macos::ax::{element_bbox, press_element, AXDispatchError, AXRef};
use crate::tools::ax_response;
use crate::tools::ax_session::{AxSession, LookupError};
use rmcp::model::CallToolResult;
use serde::Deserialize;
use std::sync::Arc;

const DISPATCHED_VIA: &str = "AXPress";

#[derive(Deserialize)]
pub struct AxClickParams {
    pub uid: String,
}

/// Handle an `ax_click` tool call.
///
/// Returns `CallToolResult::success` with `{ ok: true, dispatched_via, bbox }`
/// on success, or `CallToolResult::error` with `{ error: { code, message,
/// fallback } }` on any dispatch failure. Accessibility-permission failures
/// share the plain-text path with the coord-based `click` / `type_text`
/// tools so the user-visible message is identical.
pub async fn ax_click(params: AxClickParams, session: Arc<AxSession>) -> CallToolResult {
    if let Some(err) = crate::tools::input::check_permission() {
        return err;
    }

    // `dispatch` holds the session's read lock across the closure so a
    // concurrent `take_ax_snapshot` cannot publish a fresh generation
    // mid-press. This is the atomic half of the "fresh snapshot invalidates
    // prior uids" contract.
    let outcome = session
        .dispatch(&params.uid, |ax_ref: &AXRef| {
            // Capture pre-dispatch bbox so `not_dispatchable` / `ax_error`
            // still get a fallback centre if the element disappears
            // mid-dispatch.
            let pre_bbox = unsafe { element_bbox(ax_ref.as_raw()) };

            match press_element(ax_ref) {
                Ok(()) => {
                    // On success, return the element's CURRENT bbox for
                    // telemetry. The snapshot's bbox remains the pre-action
                    // animation source on the client side. Fall back to
                    // pre_bbox if the element no longer exposes
                    // position+size post-dispatch.
                    let post_bbox = unsafe { element_bbox(ax_ref.as_raw()) }.or(pre_bbox);
                    ax_response::success(DISPATCHED_VIA, post_bbox)
                }
                Err(AXDispatchError::NotDispatchable) => ax_response::error(
                    "not_dispatchable",
                    "element does not support AXPress",
                    pre_bbox,
                ),
                Err(AXDispatchError::AxError(code)) => ax_response::error(
                    "ax_error",
                    &format!("AXUIElementPerformAction failed with AX error {}", code),
                    pre_bbox,
                ),
            }
        })
        .await;

    match outcome {
        Ok(result) => result,
        Err(LookupError::SnapshotExpired { reason }) => {
            ax_response::error("snapshot_expired", &reason, None)
        }
        Err(LookupError::UidNotFound) => ax_response::error(
            "uid_not_found",
            &format!("uid {} is not present in the current snapshot", params.uid),
            None,
        ),
    }
}
