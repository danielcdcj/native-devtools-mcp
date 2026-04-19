//! macOS AX dispatch integration tests.
//!
//! Requires live apps (Calculator, TextEdit, Terminal). All tests are
//! `#[ignore]`-gated to match the existing AX / CDP smoke pattern — run
//! locally with:
//!
//!     cargo test --test ax_dispatch_tests -- --ignored --nocapture
//!
//! These tests assert the headline invariants from the design doc:
//! - Generation tagging defeats stale-uid dispatch (Calculator).
//! - ax_set_value writes AXValue without stealing focus (TextEdit + Terminal).
//! - ax_click presses without stealing focus (Calculator + Terminal).
//! - uid_not_found is raised for syntactically valid-but-missing uids.
//! - Bbox is emitted on snapshot lines whose elements expose position+size.
//!
//! # Execution model
//!
//! The existing CDP harness at `tests/harness/mod.rs` is CDP-specific
//! (spawns headless Chrome) and does not drive `ServerHandler::call_tool`.
//! Rather than build a full server harness, these tests drive the tool
//! functions directly — each tool handler is already `pub` in
//! `native_devtools_mcp::tools::*` and takes an explicit
//! `Arc<AxSession>`, so we share one session across the calls that make up
//! a scenario. This is a deliberate tradeoff: we lose end-to-end
//! `call_tool` dispatch coverage in favor of a copy-pasteable test that
//! compiles against the current tree. End-to-end dispatch is already
//! covered by the tool-gating tests.

#![cfg(target_os = "macos")]

use native_devtools_mcp::macos::ax::collect_ax_tree_indexed;
use native_devtools_mcp::tools::ax_click::{ax_click, AxClickParams};
use native_devtools_mcp::tools::ax_select::{ax_select, AxSelectParams};
use native_devtools_mcp::tools::ax_session::AxSession;
use native_devtools_mcp::tools::ax_set_value::{ax_set_value, AxSetValueParams};
use native_devtools_mcp::tools::ax_snapshot::format_snapshot;
use native_devtools_mcp::tools::navigation::{
    focus_window, list_apps, FocusWindowParams, ListAppsParams,
};
use rmcp::model::CallToolResult;
use std::sync::Arc;

/// Build a fresh `AxSession` for one scenario — isolates tests from each
/// other (no shared generation counter, no bleed through).
fn new_session() -> Arc<AxSession> {
    Arc::new(AxSession::new())
}

/// Take a snapshot against the shared session, matching the server's
/// `call_tool` arm: walk AX tree, swap into session, format with generation.
async fn snapshot(session: &Arc<AxSession>, app_name: Option<&str>) -> String {
    let (nodes, refs) =
        collect_ax_tree_indexed(app_name).expect("collect_ax_tree_indexed should succeed");
    let generation = session.create_snapshot(refs).await;
    format_snapshot(&nodes, Some(generation))
}

/// Bring an app to front using the coord-based focus_window tool.
fn focus(app_name: &str) -> CallToolResult {
    focus_window(FocusWindowParams {
        window_id: None,
        app_name: Some(app_name.to_string()),
        pid: None,
    })
}

/// List currently running apps via the `list_apps` tool.
fn list_apps_all() -> CallToolResult {
    list_apps(ListAppsParams {
        app_name: None,
        user_apps_only: None,
    })
}

fn extract_text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("")
}

fn parse_json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).expect("response body should be JSON")
}

fn extract_uid_for_named_button(snapshot: &str, button_name: &str) -> String {
    for line in snapshot.lines() {
        if line.contains(&format!("\"{}\"", button_name)) {
            if let Some(after) = line.split_whitespace().next() {
                if let Some(uid) = after.strip_prefix("uid=") {
                    return uid.to_string();
                }
            }
        }
    }
    panic!(
        "no uid for button {} in snapshot:\n{}",
        button_name, snapshot
    );
}

/// Find the line in a snapshot corresponding to the Calculator display
/// (text/staticText role) and extract its `value="..."` attribute, if any.
/// Returns None when the display is blank or absent.
///
/// Calculator's display is a text/readout element whose `AXValue` is the
/// shown number. This is an actual oracle — the "5" button label is always
/// present in the tree regardless of which button was pressed.
fn extract_calculator_display_value(snapshot: &str) -> Option<String> {
    // The display line looks something like:
    //   `uid=aNgG text "Display" value="5"`
    // or without a name:
    //   `uid=aNgG text value="5"`
    for line in snapshot.lines() {
        let depth = line.len() - line.trim_start().len();
        if depth > 6 {
            continue; // heuristic: display is near the top of the tree
        }
        let trimmed = line.trim_start();
        // Line must be an AX text node and carry a `value="..."` attribute.
        if !(trimmed.contains(" text ") || trimmed.contains(" text\t")) {
            continue;
        }
        if let Some(v_start) = trimmed.find("value=\"") {
            let rest = &trimmed[v_start + "value=\"".len()..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

// === Happy-path: Calculator + ax_click ===

#[tokio::test]
#[ignore]
async fn ax_click_presses_calculator_five_button() {
    // Requires Calculator to be running; start with a cleared display
    // (press AC / Clear before running to keep the test deterministic).
    let session = new_session();

    let snap_text = snapshot(&session, Some("Calculator")).await;
    let five_uid = extract_uid_for_named_button(&snap_text, "5");
    let before = extract_calculator_display_value(&snap_text).unwrap_or_default();

    let click = ax_click(AxClickParams { uid: five_uid }, session.clone()).await;
    let body = parse_json(&extract_text(&click));
    assert_eq!(body["ok"], true);
    assert_eq!(body["dispatched_via"], "AXPress");

    // Oracle: the display value must CHANGE and end in "5" — merely
    // finding the "5" button label in the tree proves nothing because it
    // is always present regardless of dispatch.
    let snap2_text = snapshot(&session, Some("Calculator")).await;
    let after = extract_calculator_display_value(&snap2_text)
        .expect("Calculator display should expose a value after AXPress");
    assert_ne!(
        before, after,
        "display value must change after AXPress (before={before:?}, after={after:?})"
    );
    assert!(
        after.ends_with('5'),
        "display should end in '5' after AXPress on the 5 button (got {after:?})"
    );
}

// === Stale-gen replay — the D1.C1 regression test ===

#[tokio::test]
#[ignore]
async fn ax_click_stale_generation_returns_snapshot_expired() {
    let session = new_session();

    let snap1 = snapshot(&session, Some("Calculator")).await;
    let five_g1 = extract_uid_for_named_button(&snap1, "5");

    // Fresh snapshot bumps the generation. Don't use its uids.
    let _snap2 = snapshot(&session, Some("Calculator")).await;

    let click = ax_click(AxClickParams { uid: five_g1 }, session.clone()).await;
    assert_eq!(click.is_error, Some(true));
    let body = parse_json(&extract_text(&click));
    assert_eq!(
        body["error"]["code"], "snapshot_expired",
        "stale gen-1 uid must not resolve to gen-2 element"
    );
}

// === uid_not_found ===

#[tokio::test]
#[ignore]
async fn ax_click_unknown_uid_in_current_gen_returns_uid_not_found() {
    let session = new_session();

    let snap_text = snapshot(&session, Some("Calculator")).await;
    // Extract the current generation.
    let first_line = snap_text.lines().next().expect("non-empty snapshot");
    let uid_token = first_line
        .split_whitespace()
        .next()
        .unwrap()
        .strip_prefix("uid=")
        .unwrap();
    let (_, gen_part) = uid_token.split_once('g').unwrap();
    let gen: u64 = gen_part.parse().unwrap();

    let missing = format!("a99999g{}", gen);
    let click = ax_click(AxClickParams { uid: missing }, session.clone()).await;
    assert_eq!(click.is_error, Some(true));
    let body = parse_json(&extract_text(&click));
    assert_eq!(body["error"]["code"], "uid_not_found");
}

// === Not-dispatchable ===

#[tokio::test]
#[ignore]
async fn ax_click_on_decorative_label_returns_not_dispatchable_with_fallback() {
    let session = new_session();

    let snap_text = snapshot(&session, Some("Calculator")).await;
    // Find a static-text / generic node with a bbox.
    let decorative_line = snap_text
        .lines()
        .find(|l| (l.contains(" text ") || l.contains(" generic ")) && l.contains("bbox=("))
        .expect("calculator should contain at least one non-pressable node with a bbox");
    let decorative_uid = decorative_line
        .split_whitespace()
        .next()
        .unwrap()
        .strip_prefix("uid=")
        .unwrap()
        .to_string();

    let click = ax_click(
        AxClickParams {
            uid: decorative_uid,
        },
        session.clone(),
    )
    .await;
    assert_eq!(click.is_error, Some(true));
    let body = parse_json(&extract_text(&click));
    assert_eq!(body["error"]["code"], "not_dispatchable");
    assert!(
        body["error"]["fallback"].is_object(),
        "fallback should be populated when bbox is readable"
    );
    assert!(body["error"]["fallback"]["x"].as_f64().unwrap() > 0.0);
    assert!(body["error"]["fallback"]["y"].as_f64().unwrap() > 0.0);
}

// === Bbox presence on snapshot ===

#[tokio::test]
#[ignore]
async fn take_ax_snapshot_emits_bbox_on_positioned_nodes() {
    let session = new_session();
    let snap_text = snapshot(&session, Some("Calculator")).await;
    let five_line = snap_text
        .lines()
        .find(|l| l.contains("\"5\""))
        .expect("Calculator should expose a '5' button");
    assert!(
        five_line.contains("bbox=("),
        "positioned button should carry a bbox: {}",
        five_line
    );
    // Parse the bbox for format validation — four comma-separated numerics.
    let bbox_start = five_line.find("bbox=(").unwrap() + "bbox=(".len();
    let bbox_end = five_line[bbox_start..].find(')').unwrap();
    let parts: Vec<&str> = five_line[bbox_start..bbox_start + bbox_end]
        .split(',')
        .collect();
    assert_eq!(parts.len(), 4, "bbox must have four fields");
    for p in parts {
        p.parse::<i64>().expect("bbox components must be integers");
    }
}

/// Return the name of the currently-active (frontmost) app by calling the
/// `list_apps` tool and inspecting each entry's `is_active` boolean.
fn active_app_name() -> String {
    let r = list_apps_all();
    let text = extract_text(&r);
    let apps: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("list_apps should return a JSON array");
    let active = apps
        .iter()
        .find(|a| a.get("is_active").and_then(|v| v.as_bool()) == Some(true))
        .expect("at least one app should be is_active=true");
    active
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .expect("active app should have a name")
}

// === Focus preservation — ax_click (AXPress path) ===

#[tokio::test]
#[ignore]
async fn ax_click_preserves_focus_while_calculator_stays_background() {
    // Requires Calculator and Terminal both running, and Calculator's
    // display cleared (press AC) before starting so the display oracle is
    // deterministic.
    let session = new_session();

    let snap_text = snapshot(&session, Some("Calculator")).await;
    let five_uid = extract_uid_for_named_button(&snap_text, "5");
    let before = extract_calculator_display_value(&snap_text).unwrap_or_default();

    // Bring Terminal to front.
    let _focus = focus("Terminal");

    // Sanity: Terminal really is frontmost before we dispatch.
    assert_eq!(
        active_app_name(),
        "Terminal",
        "precondition: Terminal should be frontmost before ax_click"
    );

    // Click Calculator's 5 button without stealing focus.
    let click = ax_click(AxClickParams { uid: five_uid }, session.clone()).await;
    let body = parse_json(&extract_text(&click));
    assert_eq!(body["ok"], true);

    // Focus invariant: Terminal must STILL be the active app after dispatch.
    assert_eq!(
        active_app_name(),
        "Terminal",
        "ax_click must not steal focus from Terminal"
    );

    // Dispatch invariant: Calculator's display value actually changed and
    // ends in "5". Label presence alone is not a valid oracle.
    let snap2_text = snapshot(&session, Some("Calculator")).await;
    let after = extract_calculator_display_value(&snap2_text)
        .expect("Calculator display should have a value after AXPress");
    assert_ne!(
        before, after,
        "display value must change after AXPress (before={before:?}, after={after:?})"
    );
    assert!(
        after.ends_with('5'),
        "display should end in '5' after pressing the 5 button (got {after:?})"
    );
}

// === Focus preservation — ax_set_value (TextEdit) ===

#[tokio::test]
#[ignore]
async fn ax_set_value_preserves_focus_writing_textedit_while_terminal_is_front() {
    let session = new_session();

    // Snapshot TextEdit FIRST, while we can still find it via app_name.
    // app_name targets the snapshot at a specific app regardless of
    // frontmost state — load-bearing because focus moves to Terminal next.
    let snap_text = snapshot(&session, Some("TextEdit")).await;
    // Find the document text area uid.
    let doc_line = snap_text
        .lines()
        .find(|l| l.contains("textbox") || l.contains("textarea"))
        .expect("TextEdit should expose a document text area");
    let doc_uid = doc_line
        .split_whitespace()
        .next()
        .unwrap()
        .strip_prefix("uid=")
        .unwrap()
        .to_string();

    // Switch focus to Terminal.
    let _ = focus("Terminal");

    // Precondition: Terminal really is frontmost.
    assert_eq!(
        active_app_name(),
        "Terminal",
        "precondition: Terminal should be frontmost before ax_set_value"
    );

    // Dispatch value write.
    let set = ax_set_value(
        AxSetValueParams {
            uid: doc_uid,
            text: "hello".to_string(),
        },
        session.clone(),
    )
    .await;
    let body = parse_json(&extract_text(&set));
    assert_eq!(body["ok"], true, "body was {}", body);
    assert_eq!(body["dispatched_via"], "AXSetAttributeValue");

    // Focus invariant: Terminal must still be frontmost.
    assert_eq!(
        active_app_name(),
        "Terminal",
        "ax_set_value must not steal focus from Terminal"
    );

    // Dispatch invariant: TextEdit's document value is now "hello". Use a
    // fresh session for the verification snapshot so we don't bump the
    // session the previous dispatch used.
    let verify_session = new_session();
    let snap2_text = snapshot(&verify_session, Some("TextEdit")).await;
    assert!(
        snap2_text.contains("value=\"hello\""),
        "TextEdit should reflect the written value"
    );
}

// === ax_select — sidebar row selection in System Settings ===

/// Open a System Settings pane in the background (no focus steal) so the
/// test runs deterministically without cursor/focus contention.
fn open_system_settings_pane(pane_id: &str) {
    // `open -g` launches in background; `x-apple.systempreferences:<pane>`
    // is the documented URL scheme for System Settings panes. We use
    // `com.apple.preference.security` because Privacy & Security is a
    // commonly-present pane across macOS versions.
    let url = format!("x-apple.systempreferences:{}", pane_id);
    let status = std::process::Command::new("open")
        .args(["-g", &url])
        .status()
        .expect("`open` should be invocable");
    assert!(
        status.success(),
        "`open -g {}` should succeed; got {:?}",
        url,
        status
    );
    // Give Settings a moment to reach the snapshot-worthy state. The
    // AX tree is not populated instantly after pane switch.
    std::thread::sleep(std::time::Duration::from_millis(1500));
}

/// Find a row line in a snapshot whose subtree contains a cell with the
/// given text. Returns `(uid, selected)` for the row or `None` when no
/// such row exists. Heuristic: scan for lines with role `row ` and look at
/// the immediately following lines (greater indent) for a cell whose name
/// or value equals `text`.
fn find_sidebar_row(snapshot: &str, text: &str) -> Option<(String, bool)> {
    let lines: Vec<&str> = snapshot.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !(line.contains(" row ") || line.contains(" row\t")) {
            continue;
        }
        // Parse uid and selected state from this row line.
        let uid_token = line.split_whitespace().next()?;
        let uid = uid_token.strip_prefix("uid=")?.to_string();
        let selected = line.contains("selected");
        // Row indent — children must be strictly deeper.
        let row_indent = line.len() - line.trim_start().len();
        // Scan descendants until indent returns to <= row_indent.
        for follow in lines.iter().skip(i + 1) {
            let follow_indent = follow.len() - follow.trim_start().len();
            if follow_indent <= row_indent {
                break;
            }
            if follow.contains(&format!("\"{}\"", text)) {
                return Some((uid, selected));
            }
        }
    }
    None
}

/// Smoke test: open System Settings' Privacy & Security pane, pick any
/// sidebar row whose text differs from the currently-selected one,
/// dispatch `ax_select` against it, and assert the selection moved. The
/// test is tolerant of locale and version drift — it does not require a
/// specific row to be present, only that at least two rows exist and we
/// can flip between them.
#[tokio::test]
#[ignore]
async fn ax_select_moves_sidebar_selection_in_system_settings() {
    open_system_settings_pane("com.apple.preference.security");

    let session = new_session();
    let snap_text = snapshot(&session, Some("System Settings")).await;

    // Collect all sidebar rows and their selected state.
    let mut rows: Vec<(String, bool, String)> = Vec::new();
    let lines: Vec<&str> = snap_text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !(line.contains(" row ") || line.contains(" row\t")) {
            continue;
        }
        let Some(uid_token) = line.split_whitespace().next() else {
            continue;
        };
        let Some(uid) = uid_token.strip_prefix("uid=") else {
            continue;
        };
        let selected = line.contains("selected");
        // Extract any descendant label as a readable identifier for this row.
        let row_indent = line.len() - line.trim_start().len();
        let mut label = String::new();
        for follow in lines.iter().skip(i + 1) {
            let follow_indent = follow.len() - follow.trim_start().len();
            if follow_indent <= row_indent {
                break;
            }
            if let Some(start) = follow.find('"') {
                let rest = &follow[start + 1..];
                if let Some(end) = rest.find('"') {
                    if end > 0 {
                        label = rest[..end].to_string();
                        break;
                    }
                }
            }
        }
        rows.push((uid.to_string(), selected, label));
    }
    assert!(
        rows.len() >= 2,
        "System Settings sidebar should expose at least two rows; got {:?}",
        rows
    );

    let previously_selected = rows.iter().find(|(_, sel, _)| *sel);
    // Pick a target row: any row that is NOT the currently-selected one.
    let target = rows
        .iter()
        .find(|(_, sel, _)| !*sel)
        .expect("at least one sidebar row should be non-selected to target");
    let target_uid = target.0.clone();
    let target_label = target.2.clone();

    // Dispatch the selection.
    let result = ax_select(
        AxSelectParams {
            uid: target_uid.clone(),
        },
        session.clone(),
    )
    .await;
    let body = parse_json(&extract_text(&result));
    assert_eq!(
        body["ok"], true,
        "ax_select should succeed on a live sidebar row; body={}",
        body
    );
    assert_eq!(body["dispatched_via"], "AXSelectedRows");

    // Re-snapshot and verify the selection moved. The new snapshot lives
    // in a fresh session so the uids don't overlap, and we match rows by
    // label rather than uid.
    let verify_session = new_session();
    let snap2_text = snapshot(&verify_session, Some("System Settings")).await;

    // The target row must now be selected.
    let (_new_uid, now_selected) =
        find_sidebar_row(&snap2_text, &target_label).unwrap_or_else(|| {
            panic!("target row (label={target_label:?}) missing from post-dispatch snapshot")
        });
    assert!(
        now_selected,
        "target row (label={:?}) should be selected after ax_select",
        target_label
    );

    // The previously-selected row (if any) must no longer be selected.
    if let Some((_prev_uid, _prev_sel, prev_label)) = previously_selected {
        if prev_label != &target_label {
            if let Some((_, still_selected)) = find_sidebar_row(&snap2_text, prev_label) {
                assert!(
                    !still_selected,
                    "previously-selected row (label={:?}) should no longer be selected",
                    prev_label
                );
            }
        }
    }
}

/// Dispatching `ax_select` at a uid that has no `AXRow` ancestor must
/// return the `no_row_ancestor` error envelope with the fallback bbox set
/// to the starting element's centre — not panic, not succeed.
#[tokio::test]
#[ignore]
async fn ax_select_on_non_row_element_returns_no_row_ancestor() {
    // Calculator has no AXRow anywhere in its tree, so every uid is a
    // negative case.
    let session = new_session();
    let snap_text = snapshot(&session, Some("Calculator")).await;
    let five_uid = extract_uid_for_named_button(&snap_text, "5");

    let result = ax_select(AxSelectParams { uid: five_uid }, session.clone()).await;
    assert_eq!(result.is_error, Some(true));
    let body = parse_json(&extract_text(&result));
    assert_eq!(body["error"]["code"], "no_row_ancestor");
    // Fallback centre should be populated from the button's bbox.
    assert!(
        body["error"]["fallback"].is_object(),
        "fallback should be populated when the starting element has a bbox"
    );
}
