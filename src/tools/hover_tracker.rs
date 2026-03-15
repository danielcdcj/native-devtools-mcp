//! Hover tracking state and event types.
//!
//! Manages a background polling task that tracks cursor position and
//! accessibility element changes, emitting events on transitions.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::now_millis;

/// A hover dwell event — emitted when the cursor leaves an element (or tracking ends).
#[derive(Debug, Clone, Serialize)]
pub struct HoverEvent {
    /// Absolute Unix milliseconds when the cursor first arrived at this element
    pub timestamp_ms: u64,
    /// Cursor position when the element was first entered
    pub cursor: CursorPosition,
    /// The accessibility element that was hovered
    pub element: HoverElement,
    /// How long the cursor stayed on this element (ms)
    pub dwell_ms: u64,
    /// If true, tracking auto-stopped due to max duration
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub timeout: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

/// Accessibility element info captured during hover.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct HoverElement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<ElementBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Active hover tracking session.
pub struct HoverTracker {
    events: Arc<Mutex<Vec<HoverEvent>>>,
    task_handle: JoinHandle<()>,
    cancel: CancellationToken,
}

impl HoverTracker {
    pub fn new(
        events: Arc<Mutex<Vec<HoverEvent>>>,
        task_handle: JoinHandle<()>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            events,
            task_handle,
            cancel,
        }
    }

    /// Check if the background polling task has finished (due to timeout or error).
    pub fn is_finished(&self) -> bool {
        self.task_handle.is_finished()
    }

    /// Drain all buffered events, returning them and clearing the buffer.
    pub fn drain_events(&self) -> Vec<HoverEvent> {
        let mut events = self.events.lock().unwrap();
        events.drain(..).collect()
    }

    /// Cancel tracking, await task shutdown, then drain remaining events.
    ///
    /// Drains after the task finishes to avoid losing late events from
    /// in-flight `spawn_blocking` calls. Aborts the task if it doesn't
    /// stop within 500ms (e.g. slow AX query).
    pub async fn cancel_and_drain(self) -> Vec<HoverEvent> {
        self.cancel.cancel();
        let Self {
            events,
            mut task_handle,
            ..
        } = self;
        if tokio::time::timeout(std::time::Duration::from_millis(500), &mut task_handle)
            .await
            .is_err()
        {
            task_handle.abort();
        }
        let mut buf = events.lock().unwrap();
        buf.drain(..).collect()
    }
}

/// Max characters for string fields in hover events.
/// Keeps output compact — full element text (e.g. terminal buffers) is noise for hover tracking.
const MAX_FIELD_LEN: usize = 100;

/// Truncate a string to `MAX_FIELD_LEN`, appending "…" if truncated.
fn truncate_field(s: &str) -> String {
    if s.len() <= MAX_FIELD_LEN {
        s.to_string()
    } else {
        // Find a char boundary at or before MAX_FIELD_LEN
        let mut end = MAX_FIELD_LEN;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// Parse a `serde_json::Value` (from `element_at_point`) into a `HoverElement`.
pub fn parse_hover_element(value: &serde_json::Value) -> HoverElement {
    let str_field = |key: &str| -> Option<String> {
        value.get(key).and_then(|v| v.as_str()).map(truncate_field)
    };

    HoverElement {
        name: str_field("name"),
        role: str_field("role"),
        label: str_field("label"),
        bounds: value.get("bounds").map(|b| ElementBounds {
            x: b.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            y: b.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
            width: b.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0),
            height: b.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }),
        app_name: str_field("app_name"),
        pid: value.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32),
    }
}

/// Check if two elements are the same (by role + name + bounds).
pub fn elements_equal(a: &HoverElement, b: &HoverElement) -> bool {
    a.role == b.role && a.name == b.name && a.bounds == b.bounds
}

/// Snapshot of an element that has met (or is being evaluated against) the dwell threshold.
/// Used for both the confirmed hover and the candidate hover.
struct HoverEntry {
    element: HoverElement,
    since: Instant,
    enter_ms: u64,
    cursor: (f64, f64),
}

impl HoverEntry {
    /// Build a leave/timeout event from this entry's stored state.
    /// `left_at` is the monotonic instant the cursor departed this element.
    fn into_event(self, left_at: Instant, timeout: bool) -> HoverEvent {
        HoverEvent {
            timestamp_ms: self.enter_ms,
            cursor: CursorPosition {
                x: self.cursor.0,
                y: self.cursor.1,
            },
            element: self.element,
            dwell_ms: left_at.duration_since(self.since).as_millis() as u64,
            timeout,
        }
    }
}

/// Start the hover polling background task.
///
/// Polls cursor position + element_at_point every `poll_interval_ms`,
/// pushing a `HoverEvent` when the element under the cursor changes and
/// the cursor has dwelled on the new element for at least `min_dwell_ms`.
/// This filters out pass-through elements during fast mouse movement.
/// Stops when `cancel` is triggered or `max_duration_ms` elapses.
pub fn start_polling(
    events: Arc<Mutex<Vec<HoverEvent>>>,
    cancel: CancellationToken,
    app_name: Option<String>,
    poll_interval_ms: u32,
    max_duration_ms: u32,
    min_dwell_ms: u32,
) -> JoinHandle<()> {
    // Use Arc<str> to avoid cloning the string on every poll tick
    let app_name: Option<Arc<str>> = app_name.map(|s| Arc::from(s.as_str()));

    tokio::spawn(async move {
        let start = Instant::now();
        let max_duration = std::time::Duration::from_millis(max_duration_ms as u64);
        let poll_interval = std::time::Duration::from_millis(poll_interval_ms as u64);
        let min_dwell = std::time::Duration::from_millis(min_dwell_ms as u64);

        // The element currently being hovered (confirmed after meeting dwell threshold).
        // We emit an event about this element when the cursor leaves it.
        let mut confirmed: Option<HoverEntry> = None;

        // A candidate element that differs from confirmed but hasn't met dwell threshold yet.
        let mut candidate: Option<HoverEntry> = None;

        loop {
            // Check cancellation
            if cancel.is_cancelled() {
                return;
            }

            // Check max duration
            if start.elapsed() >= max_duration {
                // Emit the confirmed element's dwell before stopping
                if let Some(entry) = confirmed {
                    events
                        .lock()
                        .unwrap()
                        .push(entry.into_event(Instant::now(), true));
                }
                return;
            }

            // Get cursor position + element in a single spawn_blocking call
            let app = app_name.clone();
            let poll_result = tokio::task::spawn_blocking(move || {
                let cursor = get_cursor_position_sync()?;
                let element = element_at_point_for_hover(cursor.0, cursor.1, app.as_deref())?;
                Ok::<_, String>((cursor, element))
            })
            .await;

            let (cursor, current_element) = match poll_result {
                Ok(Ok(result)) => result,
                _ => {
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }
            };

            // Is this element different from the last confirmed one?
            let differs_from_confirmed = match &confirmed {
                Some(c) => !elements_equal(&c.element, &current_element),
                None => true, // First element is always new
            };

            if !differs_from_confirmed {
                // Cursor moved back to confirmed element — discard candidate
                candidate = None;
                tokio::time::sleep(poll_interval).await;
                continue;
            }

            // Element differs from confirmed — check candidate state
            let cand_matches = candidate
                .as_ref()
                .map_or(false, |c| elements_equal(&c.element, &current_element));

            if cand_matches {
                let cand = candidate.as_ref().unwrap();
                // Same candidate — check if dwell threshold met
                if cand.since.elapsed() >= min_dwell {
                    // Emit event about the element being LEFT
                    if let Some(prev) = confirmed.take() {
                        events
                            .lock()
                            .unwrap()
                            .push(prev.into_event(cand.since, false));
                    }
                    // Promote candidate to confirmed
                    confirmed = candidate.take();
                }
                // else: keep waiting
            } else {
                // New candidate (or different from previous candidate)
                candidate = Some(HoverEntry {
                    element: current_element,
                    since: Instant::now(),
                    enter_ms: now_millis(),
                    cursor,
                });
            }

            tokio::time::sleep(poll_interval).await;
        }
    })
}

/// Get cursor position synchronously (fast CGEvent read, no spawn_blocking needed).
fn get_cursor_position_sync() -> Result<(f64, f64), String> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::input::get_cursor_position()
    }
    #[cfg(target_os = "windows")]
    {
        Err("Hover tracking is not yet supported on Windows".to_string())
    }
}

/// Query element_at_point for hover tracking (wraps platform call).
fn element_at_point_for_hover(
    x: f64,
    y: f64,
    app_name: Option<&str>,
) -> Result<HoverElement, String> {
    #[cfg(target_os = "macos")]
    {
        let value = crate::macos::ax::element_at_point(x, y, app_name)?;
        Ok(parse_hover_element(&value))
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (x, y, app_name);
        Err("Hover tracking is not yet supported on Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hover_element_full() {
        let json = serde_json::json!({
            "name": "File",
            "role": "AXMenuBarItem",
            "label": "File menu",
            "value": null,
            "bounds": { "x": 100.0, "y": 200.0, "width": 40.0, "height": 22.0 },
            "app_name": "Finder",
            "pid": 1234
        });
        let el = parse_hover_element(&json);
        assert_eq!(el.name, Some("File".to_string()));
        assert_eq!(el.role, Some("AXMenuBarItem".to_string()));
        assert_eq!(el.label, Some("File menu".to_string()));
        assert_eq!(el.app_name, Some("Finder".to_string()));
        assert_eq!(el.pid, Some(1234));
        assert_eq!(
            el.bounds,
            Some(ElementBounds {
                x: 100.0,
                y: 200.0,
                width: 40.0,
                height: 22.0
            })
        );
    }

    #[test]
    fn test_parse_hover_element_empty() {
        let json = serde_json::json!({});
        let el = parse_hover_element(&json);
        assert_eq!(el.name, None);
        assert_eq!(el.role, None);
        assert_eq!(el.bounds, None);
    }

    #[test]
    fn test_elements_equal_same() {
        let a = HoverElement {
            name: Some("File".into()),
            role: Some("AXMenuBarItem".into()),
            label: Some("label".into()),
            bounds: Some(ElementBounds {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            }),
            app_name: Some("Finder".into()),
            pid: Some(1),
        };
        let b = HoverElement {
            name: Some("File".into()),
            role: Some("AXMenuBarItem".into()),
            label: Some("different label".into()),
            bounds: Some(ElementBounds {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            }),
            app_name: Some("Finder".into()),
            pid: Some(999),
        };
        assert!(elements_equal(&a, &b));
    }

    #[test]
    fn test_elements_equal_different_role() {
        let a = HoverElement {
            name: Some("File".into()),
            role: Some("AXMenuBarItem".into()),
            label: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        let b = HoverElement {
            name: Some("File".into()),
            role: Some("AXButton".into()),
            label: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        assert!(!elements_equal(&a, &b));
    }

    #[test]
    fn test_elements_equal_different_name() {
        let a = HoverElement {
            name: Some("File".into()),
            role: Some("AXMenuBarItem".into()),
            label: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        let b = HoverElement {
            name: Some("Edit".into()),
            role: Some("AXMenuBarItem".into()),
            label: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        assert!(!elements_equal(&a, &b));
    }

    #[test]
    fn test_drain_events_clears_buffer() {
        let ts = now_millis();
        let events = Arc::new(Mutex::new(vec![HoverEvent {
            timestamp_ms: ts,
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: Some("A".into()),
                role: None,
                label: None,

                bounds: None,
                app_name: None,
                pid: None,
            },
            dwell_ms: 50,
            timeout: false,
        }]));
        let cancel = CancellationToken::new();
        let tracker = HoverTracker::new(
            events.clone(),
            tokio::runtime::Runtime::new().unwrap().spawn(async {}),
            cancel,
        );

        let drained = tracker.drain_events();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].timestamp_ms, ts);

        // Second drain should be empty
        let drained2 = tracker.drain_events();
        assert!(drained2.is_empty());
    }

    #[test]
    fn test_hover_event_serialization_omits_timeout_when_false() {
        let event = HoverEvent {
            timestamp_ms: now_millis(),
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: Some("A".into()),
                role: None,
                label: None,

                bounds: None,
                app_name: None,
                pid: None,
            },
            dwell_ms: 50,
            timeout: false,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("timeout"));
    }

    #[tokio::test]
    async fn test_start_polling_cancellation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();

        let handle = start_polling(
            events.clone(),
            cancel.clone(),
            None, // no app_name
            50,   // 50ms poll interval
            1000, // 1s max duration
            0,    // no dwell threshold
        );

        // Cancel immediately
        cancel.cancel();
        // Task should finish promptly
        tokio::time::timeout(std::time::Duration::from_millis(500), handle)
            .await
            .expect("task should finish after cancel")
            .expect("task should not panic");
    }

    #[tokio::test]
    async fn test_start_polling_max_duration_timeout() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();

        let handle = start_polling(
            events.clone(),
            cancel.clone(),
            None,
            50,  // 50ms poll interval
            500, // 500ms max duration
            0,   // no dwell threshold
        );

        // AX queries can be slow; allow generous margin
        tokio::time::timeout(std::time::Duration::from_millis(3000), handle)
            .await
            .expect("task should auto-stop after max duration")
            .expect("task should not panic");

        // If any element was confirmed before timeout, the last event should be
        // a timeout sentinel. If no element was confirmed (e.g. AX was too slow),
        // there may be no events at all — both are valid outcomes.
        let evts = events.lock().unwrap();
        if let Some(last) = evts.last() {
            assert!(last.timeout, "last event should be a timeout sentinel");
        }
    }

    #[test]
    fn test_truncate_field_short_string() {
        assert_eq!(truncate_field("hello"), "hello");
    }

    #[test]
    fn test_truncate_field_exact_limit() {
        let s = "a".repeat(MAX_FIELD_LEN);
        assert_eq!(truncate_field(&s), s);
    }

    #[test]
    fn test_truncate_field_long_string() {
        let s = "a".repeat(MAX_FIELD_LEN + 50);
        let result = truncate_field(&s);
        assert!(result.len() <= MAX_FIELD_LEN + "…".len());
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_field_multibyte() {
        // Ensure we don't panic on multi-byte chars at the boundary
        let s = "é".repeat(MAX_FIELD_LEN); // each é is 2 bytes
        let result = truncate_field(&s);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_parse_hover_element_truncates_long_name() {
        let long_name = "x".repeat(500);
        let json = serde_json::json!({
            "role": "AXStaticText",
            "name": long_name,
        });
        let el = parse_hover_element(&json);
        let name = el.name.unwrap();
        assert!(name.len() <= MAX_FIELD_LEN + "…".len());
        assert!(name.ends_with('…'));
    }

    #[test]
    fn test_parse_hover_element_drops_value() {
        let json = serde_json::json!({
            "role": "AXTextArea",
            "value": "some text content",
        });
        let el = parse_hover_element(&json);
        // value field is not captured in HoverElement
        assert_eq!(el.role, Some("AXTextArea".to_string()));
    }

    #[test]
    fn test_hover_event_serialization_includes_timeout_when_true() {
        let event = HoverEvent {
            timestamp_ms: now_millis(),
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: None,
                role: None,
                label: None,

                bounds: None,
                app_name: None,
                pid: None,
            },
            dwell_ms: 500,
            timeout: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"timeout\":true"));
    }
}
