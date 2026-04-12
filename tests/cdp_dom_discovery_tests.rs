//! Integration tests for the CDP DOM discovery pipeline.
//!
//! # What this covers
//!
//! Unit tests already exercise the pure Rust logic (UID assignment, prefix
//! parsing, snapshot conversion). These tests cover the *live* pipeline:
//!
//!   Runtime.evaluate (JS walker) → DOM.describeNode (backendNodeId) →
//!   SnapshotMap (`d<N>` / `a<N>`) → action-tool UID resolution → click/eval.
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
//! All scenarios are `#[ignore]`d because they require a Google Chrome binary
//! on the host. CI jobs that have Chrome should run them with:
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
//! If Chrome is not installed or not on the platform we expect (macOS /
//! Linux), every test short-circuits to `skip!` and records why.

#![cfg(feature = "cdp")]

mod harness;

use harness::{
    content_text, Harness, HTML_CONTENTEDITABLE, HTML_CUSTOM_BUTTON, HTML_DUPLICATE_LABELS,
    HTML_MIXED_AX_DOM, HTML_SHADOW_AND_IFRAME,
};
use native_devtools_mcp::cdp::tools::{
    cdp_click, cdp_find_elements, cdp_take_ax_snapshot, cdp_take_dom_snapshot,
};

/// Contenteditable editor found by placeholder only.
///
/// An AX-invisible input (no <input>, no role="textbox", just a <div
/// contenteditable data-placeholder="Write something…">) must be surfaced by
/// the DOM walker — the AX tree won't expose a meaningful label.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn contenteditable_found_by_placeholder() {
    let Some(mut h) = Harness::launch().await else {
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
    let Some(mut h) = Harness::launch().await else {
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
    let Some(mut h) = Harness::launch().await else {
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
    let Some(mut h) = Harness::launch().await else {
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
    assert!(
        content_text(&shadow).contains("ShadowBtn"),
        "shadow-root button not found:\n{}",
        content_text(&shadow)
    );

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
    assert!(
        content_text(&iframe).contains("IframeBtn"),
        "iframe button not found:\n{}",
        content_text(&iframe)
    );
}

/// Mixed AX/DOM workflow end-to-end.
///
/// 1. Take an AX snapshot — populates `last_ax_snapshot` and yields `a<N>`
///    UIDs for semantically-tagged elements.
/// 2. Take a DOM snapshot — populates `last_dom_snapshot` independently and
///    yields `d<N>` UIDs for *every* interactive element on the page,
///    including ones AX missed.
/// 3. Click a button via its `a<N>` UID, verify it lands on the AX-labelled
///    button (page marks `#ax-hit`).
/// 4. Click a contenteditable via its `d<N>` UID, verify the DOM snapshot
///    targeting hit the contenteditable and not the AX button.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Chrome — run with `cargo test -- --ignored`"]
async fn mixed_ax_and_dom_uids_resolve_independently() {
    let Some(mut h) = Harness::launch().await else {
        return;
    };
    h.navigate(HTML_MIXED_AX_DOM).await;

    // --- AX snapshot + click ---
    let ax_out = cdp_take_ax_snapshot(h.client_handle()).await;
    assert_eq!(ax_out.is_error, Some(false));
    let ax_text = content_text(&ax_out);

    // Find the a-prefixed UID of the button labelled "AxButton".
    let ax_uid = harness::find_ax_uid(&ax_text, "button", "AxButton")
        .unwrap_or_else(|| panic!("could not find 'AxButton' in AX snapshot:\n{ax_text}"));

    let click_ax = cdp_click(ax_uid.clone(), false, false, h.client_handle()).await;
    assert_eq!(
        click_ax.is_error,
        Some(false),
        "click {ax_uid} failed: {click_ax:?}"
    );

    let ax_hit = h
        .eval_bool("document.getElementById('ax-hit').dataset.clicked === '1'")
        .await;
    assert!(ax_hit, "AX UID {ax_uid} click did not register on #ax-hit");

    // --- DOM snapshot + click ---
    let dom_out = cdp_take_dom_snapshot(None, h.client_handle()).await;
    assert_eq!(dom_out.is_error, Some(false));
    let dom_text = content_text(&dom_out);

    // Find the d-prefixed UID of the contenteditable whose placeholder is
    // "EditorHere". The AX tree typically hides this.
    let dom_uid = harness::find_dom_uid(&dom_text, "textbox", "EditorHere").unwrap_or_else(|| {
        panic!("could not find 'EditorHere' textbox in DOM snapshot:\n{dom_text}")
    });

    let click_dom = cdp_click(dom_uid.clone(), false, false, h.client_handle()).await;
    assert_eq!(
        click_dom.is_error,
        Some(false),
        "click {dom_uid} failed: {click_dom:?}"
    );

    // The contenteditable receives focus on click; we attached a `focus`
    // handler that stamps `#editor-hit`.
    let editor_hit = h
        .eval_bool("document.getElementById('editor-hit').dataset.focused === '1'")
        .await;
    assert!(
        editor_hit,
        "DOM UID {dom_uid} click did not focus contenteditable"
    );

    // Sanity: the two UIDs must have different prefixes (regression guard
    // against the `a<N>` / `d<N>` prefix collapse).
    assert!(
        ax_uid.starts_with('a'),
        "AX UID should start with 'a': {ax_uid}"
    );
    assert!(
        dom_uid.starts_with('d'),
        "DOM UID should start with 'd': {dom_uid}"
    );

    // Sanity: snapshot state should have both maps populated simultaneously.
    {
        let handle = h.client_handle();
        let guard = handle.read().await;
        let client = guard.as_ref().expect("client present");
        assert!(
            client.last_ax_snapshot.is_some(),
            "AX snapshot should be retained after DOM snapshot"
        );
        assert!(
            client.last_dom_snapshot.is_some(),
            "DOM snapshot should be retained after AX snapshot"
        );
    }
}
