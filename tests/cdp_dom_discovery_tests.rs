//! Integration tests for the CDP DOM discovery pipeline.
//!
//! # What this covers
//!
//! Unit tests already exercise the pure Rust logic (UID assignment, prefix
//! parsing, snapshot conversion). These tests cover the *live* pipeline:
//!
//!   Runtime.evaluate (JS walker) → DOM.describeNode (backendNodeId) →
//!   SnapshotMap (`d<N>`) → action-tool UID resolution → click/eval.
//!
//! Failures in that chain (element removed between calls, shadow root not
//! descended, prefix parsing bug, stale-map lookup) would silently mis-target
//! elements — nothing in the unit-test surface catches that.
//!
//! # Approach (A): real headless Chrome
//!
//! A mock CDP server would have to re-implement shadow DOM traversal, iframe
//! traversal, `DOM.describeNode`, and the chromiumoxide protocol just to make
//! the scenarios meaningful — that's essentially reimplementing a browser.
//! A real headless Chrome gives us authentic coverage of the pipeline the
//! production tools walk.
//!
//! # Gating
//!
//! All scenarios are `#[ignore]`d. They require:
//!
//! - A Google Chrome / Chromium binary on the host (macOS or Linux;
//!   Windows is currently unsupported by the harness).
//! - Permission to bind an ephemeral loopback TCP port. Sandboxed
//!   environments that disable local listeners will see every scenario
//!   panic with "could not acquire a free port" rather than skip.
//!
//! CI jobs that meet both should run them with:
//!
//! ```bash
//! cargo test --test cdp_dom_discovery_tests -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` keeps Chrome instances from fighting over a shared
//! `user-data-dir`. Each test still gets its own temp profile; the flag is
//! belt-and-braces insurance against stray global state (dialog handlers,
//! etc).
//!
//! If Chrome is not installed, every test short-circuits with a stderr
//! skip note. Any other launch failure (temp dir, port, spawn, debug-port
//! wait, connect) panics so a harness regression fails loud.

#![cfg(feature = "cdp")]

mod harness;

use harness::{
    content_text, Harness, HTML_CONTENTEDITABLE, HTML_CUSTOM_BUTTON, HTML_DUPLICATE_LABELS,
    HTML_SHADOW_AND_IFRAME,
};
use native_devtools_mcp::cdp::tools::cdp_find_elements;

/// Contenteditable editor found by placeholder only.
///
/// An AX-invisible input (no <input>, no role="textbox", just a <div
/// contenteditable data-placeholder="Write something…">) must be surfaced by
/// the DOM walker — the AX tree won't expose a meaningful label.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn contenteditable_found_by_placeholder() {
    let Some(mut h) = Harness::launch_or_skip().await else {
        return;
    };
    h.navigate(HTML_CONTENTEDITABLE).await;

    let result =
        cdp_find_elements("Write something".into(), None, Some(10), h.client_handle()).await;

    assert_eq!(
        result.is_error,
        Some(false),
        "find_elements failed: {:?}",
        result
    );
    let text = content_text(&result);
    assert!(
        text.contains("\"uid\": \"d1\""),
        "expected d1 match, got:\n{text}"
    );
    assert!(
        text.contains("Write something"),
        "expected placeholder label in response:\n{text}"
    );
    assert!(
        text.contains("\"role\": \"textbox\""),
        "contenteditable should surface as textbox role:\n{text}"
    );
}

/// Custom <div role="button" aria-label="Close"> with no visible text.
///
/// The DOM walker must pick up the aria-label as the element's semantic
/// name even though `textContent` is empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn custom_button_uses_aria_label() {
    let Some(mut h) = Harness::launch_or_skip().await else {
        return;
    };
    h.navigate(HTML_CUSTOM_BUTTON).await;

    let result = cdp_find_elements(
        "Close".into(),
        Some("button".into()),
        Some(10),
        h.client_handle(),
    )
    .await;
    assert_eq!(
        result.is_error,
        Some(false),
        "find_elements failed: {:?}",
        result
    );
    let text = content_text(&result);
    assert!(
        text.contains("\"uid\": \"d1\""),
        "expected d1 for aria-labelled button, got:\n{text}"
    );
    assert!(
        text.contains("\"label\": \"Close\""),
        "expected aria-label surfaced as label:\n{text}"
    );
    assert!(
        text.contains("\"role\": \"button\""),
        "role must be button:\n{text}"
    );
}

/// Two "Search" controls — one in a <nav> sidebar, one in <main>.
///
/// The DOM walker's `parentRole` / `parentName` context lets downstream
/// consumers disambiguate; here we verify both matches come back and that
/// their parent_role differs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn duplicate_labels_disambiguated_by_parent() {
    let Some(mut h) = Harness::launch_or_skip().await else {
        return;
    };
    h.navigate(HTML_DUPLICATE_LABELS).await;

    let result = cdp_find_elements("Search".into(), None, Some(10), h.client_handle()).await;
    assert_eq!(
        result.is_error,
        Some(false),
        "find_elements failed: {:?}",
        result
    );
    let text = content_text(&result);

    let json: serde_json::Value = serde_json::from_str(&text).expect("find_elements returns JSON");
    let matches = json["matches"].as_array().expect("matches array");

    // We expect at least 2 "Search" elements (sidebar input + main button).
    // There may be extra matches from aria-labelled containers; filter to
    // just the two interactive controls.
    let parents: Vec<String> = matches
        .iter()
        .map(|m| m["parent_role"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        parents.iter().any(|p| p == "nav"),
        "expected a match parented by <nav>, got parents={parents:?}"
    );
    assert!(
        parents.iter().any(|p| p == "main"),
        "expected a match parented by <main>, got parents={parents:?}"
    );
}

/// Open shadow root + same-origin iframe traversal.
///
/// The JS walker recurses into every element with a `shadowRoot` and into
/// every same-origin iframe's `contentDocument`. A regression that drops
/// shadow descent or iframe descent shows up here as a missing match.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn shadow_root_and_iframe_traversed() {
    let Some(mut h) = Harness::launch_or_skip().await else {
        return;
    };
    h.navigate(HTML_SHADOW_AND_IFRAME).await;

    // `ShadowBtn` lives inside a custom element's open shadow root.
    let shadow = cdp_find_elements(
        "ShadowBtn".into(),
        Some("button".into()),
        Some(10),
        h.client_handle(),
    )
    .await;
    assert_eq!(
        shadow.is_error,
        Some(false),
        "find_elements (shadow) failed: {:?}",
        shadow
    );
    assert_matches_label(&shadow, "ShadowBtn");

    // `IframeBtn` lives inside a same-origin iframe (srcdoc).
    let iframe = cdp_find_elements(
        "IframeBtn".into(),
        Some("button".into()),
        Some(10),
        h.client_handle(),
    )
    .await;
    assert_eq!(
        iframe.is_error,
        Some(false),
        "find_elements (iframe) failed: {:?}",
        iframe
    );
    assert_matches_label(&iframe, "IframeBtn");
}

/// Parse a `cdp_find_elements` response and assert that its `matches`
/// array contains at least one entry whose label equals `expected`.
/// The plain `inventory` field is ignored on purpose — it's populated
/// before query/visibility filtering, so a regression that empties
/// `matches` but leaves `inventory` would otherwise silently pass.
fn assert_matches_label(result: &rmcp::model::CallToolResult, expected: &str) {
    let body = content_text(result);
    let parsed: serde_json::Value =
        serde_json::from_str(&body).expect("find_elements returns JSON");
    let matches = parsed
        .get("matches")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("response missing `matches` array:\n{body}"));
    let found = matches.iter().any(|m| {
        m.get("label")
            .and_then(|l| l.as_str())
            .is_some_and(|l| l == expected)
    });
    assert!(
        found,
        "no entry in `matches` with label={expected:?}; body:\n{body}"
    );
}
