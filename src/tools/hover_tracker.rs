//! Hover tracking state and event types.
//!
//! Manages a background polling task that tracks cursor position and
//! accessibility element changes, emitting events on transitions.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// A single hover transition event.
#[derive(Debug, Clone, Serialize)]
pub struct HoverEvent {
    /// Milliseconds since tracking started
    pub timestamp_ms: u64,
    /// Cursor position at time of transition
    pub cursor: CursorPosition,
    /// The accessibility element now under the cursor
    pub element: HoverElement,
    /// How long the cursor stayed on the previous element (ms)
    pub previous_dwell_ms: u64,
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
    pub value: Option<String>,
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

/// Parse a `serde_json::Value` (from `element_at_point`) into a `HoverElement`.
pub fn parse_hover_element(value: &serde_json::Value) -> HoverElement {
    HoverElement {
        name: value.get("name").and_then(|v| v.as_str()).map(String::from),
        role: value.get("role").and_then(|v| v.as_str()).map(String::from),
        label: value
            .get("label")
            .and_then(|v| v.as_str())
            .map(String::from),
        value: value
            .get("value")
            .and_then(|v| v.as_str())
            .map(String::from),
        bounds: value.get("bounds").map(|b| ElementBounds {
            x: b.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            y: b.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
            width: b.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0),
            height: b.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0),
        }),
        app_name: value
            .get("app_name")
            .and_then(|v| v.as_str())
            .map(String::from),
        pid: value.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32),
    }
}

/// Check if two elements are the same (by role + name + bounds).
pub fn elements_equal(a: &HoverElement, b: &HoverElement) -> bool {
    a.role == b.role && a.name == b.name && a.bounds == b.bounds
}

/// Start the hover polling background task.
///
/// Polls cursor position + element_at_point every `poll_interval_ms`,
/// pushing a `HoverEvent` whenever the element under the cursor changes.
/// Stops when `cancel` is triggered or `max_duration_ms` elapses.
pub fn start_polling(
    events: Arc<Mutex<Vec<HoverEvent>>>,
    cancel: CancellationToken,
    app_name: Option<String>,
    poll_interval_ms: u32,
    max_duration_ms: u32,
) -> JoinHandle<()> {
    // Use Arc<str> to avoid cloning the string on every poll tick
    let app_name: Option<Arc<str>> = app_name.map(|s| Arc::from(s.as_str()));

    tokio::spawn(async move {
        let start = Instant::now();
        let max_duration = std::time::Duration::from_millis(max_duration_ms as u64);
        let poll_interval = std::time::Duration::from_millis(poll_interval_ms as u64);
        let mut previous_element: Option<HoverElement> = None;
        let mut last_change = Instant::now();

        loop {
            // Check cancellation
            if cancel.is_cancelled() {
                return;
            }

            // Check max duration
            if start.elapsed() >= max_duration {
                let elapsed = start.elapsed().as_millis() as u64;
                let cursor = get_cursor_position_sync().unwrap_or((0.0, 0.0));
                let previous_dwell = last_change.elapsed().as_millis() as u64;
                let event = HoverEvent {
                    timestamp_ms: elapsed,
                    cursor: CursorPosition {
                        x: cursor.0,
                        y: cursor.1,
                    },
                    element: previous_element.unwrap_or_default(),
                    previous_dwell_ms: previous_dwell,
                    timeout: true,
                };
                events.lock().unwrap().push(event);
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

            // Check if element changed
            let changed = match &previous_element {
                Some(prev) => !elements_equal(prev, &current_element),
                None => true, // First element is always a "change"
            };

            if changed {
                let elapsed = start.elapsed().as_millis() as u64;
                let previous_dwell = last_change.elapsed().as_millis() as u64;
                let event = HoverEvent {
                    timestamp_ms: elapsed,
                    cursor: CursorPosition {
                        x: cursor.0,
                        y: cursor.1,
                    },
                    element: current_element.clone(),
                    previous_dwell_ms: previous_dwell,
                    timeout: false,
                };
                events.lock().unwrap().push(event);
                previous_element = Some(current_element);
                last_change = Instant::now();
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
        assert_eq!(el.value, None);
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
            value: None,
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
            value: Some("val".into()),
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
            value: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        let b = HoverElement {
            name: Some("File".into()),
            role: Some("AXButton".into()),
            label: None,
            value: None,
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
            value: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        let b = HoverElement {
            name: Some("Edit".into()),
            role: Some("AXMenuBarItem".into()),
            label: None,
            value: None,
            bounds: None,
            app_name: None,
            pid: None,
        };
        assert!(!elements_equal(&a, &b));
    }

    #[test]
    fn test_drain_events_clears_buffer() {
        let events = Arc::new(Mutex::new(vec![HoverEvent {
            timestamp_ms: 100,
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: Some("A".into()),
                role: None,
                label: None,
                value: None,
                bounds: None,
                app_name: None,
                pid: None,
            },
            previous_dwell_ms: 50,
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
        assert_eq!(drained[0].timestamp_ms, 100);

        // Second drain should be empty
        let drained2 = tracker.drain_events();
        assert!(drained2.is_empty());
    }

    #[test]
    fn test_hover_event_serialization_omits_timeout_when_false() {
        let event = HoverEvent {
            timestamp_ms: 100,
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: Some("A".into()),
                role: None,
                label: None,
                value: None,
                bounds: None,
                app_name: None,
                pid: None,
            },
            previous_dwell_ms: 50,
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
        );

        // AX queries can be slow; allow generous margin
        tokio::time::timeout(std::time::Duration::from_millis(3000), handle)
            .await
            .expect("task should auto-stop after max duration")
            .expect("task should not panic");

        // Should have a timeout sentinel event
        let evts = events.lock().unwrap();
        assert!(
            evts.last().map_or(false, |e| e.timeout),
            "last event should be a timeout sentinel"
        );
    }

    #[test]
    fn test_hover_event_serialization_includes_timeout_when_true() {
        let event = HoverEvent {
            timestamp_ms: 60000,
            cursor: CursorPosition { x: 1.0, y: 2.0 },
            element: HoverElement {
                name: None,
                role: None,
                label: None,
                value: None,
                bounds: None,
                app_name: None,
                pid: None,
            },
            previous_dwell_ms: 500,
            timeout: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"timeout\":true"));
    }
}
