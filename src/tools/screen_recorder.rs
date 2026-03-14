//! Screen recording: captures the frontmost app's window at a configurable
//! frame rate, writing timestamped JPEG frames to an output directory.
//!
//! Follows the same lifecycle pattern as [`super::hover_tracker::HoverTracker`]:
//! a background task captures frames, a shared buffer stores metadata, and the
//! caller drains or stops the session.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// JPEG compression quality for recorded frames (0–100).
const JPEG_QUALITY: u8 = 80;

/// Current time as Unix milliseconds.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Metadata for a single recorded frame.
#[derive(Debug, Clone, Serialize)]
pub struct RecordedFrame {
    /// Absolute Unix timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Path to the JPEG file on disk
    pub path: String,
    /// App that owned the captured window
    pub app_name: String,
    /// Window ID that was captured
    pub window_id: u32,
    /// Screen-space origin of the window (points)
    pub origin_x: f64,
    pub origin_y: f64,
    /// Backing scale factor (pixels per point)
    pub scale: f64,
    /// Pixel dimensions
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// Active screen recording session.
pub struct ScreenRecorder {
    frames: Arc<Mutex<Vec<RecordedFrame>>>,
    task_handle: JoinHandle<()>,
    cancel: CancellationToken,
}

impl ScreenRecorder {
    pub fn new(
        frames: Arc<Mutex<Vec<RecordedFrame>>>,
        task_handle: JoinHandle<()>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            frames,
            task_handle,
            cancel,
        }
    }

    /// Drain all buffered frame metadata, returning them and clearing the buffer.
    pub fn drain_frames(&self) -> Vec<RecordedFrame> {
        let mut frames = self.frames.lock().unwrap();
        frames.drain(..).collect()
    }

    /// Cancel recording, await task shutdown, then drain remaining frames.
    pub async fn cancel_and_drain(self) -> Vec<RecordedFrame> {
        self.cancel.cancel();
        let Self {
            frames,
            mut task_handle,
            ..
        } = self;
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task_handle)
            .await
            .is_err()
        {
            task_handle.abort();
        }
        let mut buf = frames.lock().unwrap();
        buf.drain(..).collect()
    }
}

/// Start the screen recording background task.
///
/// Each tick captures the frontmost app's window via `CGWindowListCreateImage`,
/// encodes to JPEG on the blocking pool, writes to disk, and pushes a
/// `RecordedFrame` to the shared buffer.
///
/// Stops when `cancel` is triggered or `max_duration_ms` elapses.
pub fn start_recording(
    frames: Arc<Mutex<Vec<RecordedFrame>>>,
    cancel: CancellationToken,
    output_dir: PathBuf,
    fps: u32,
    max_duration_ms: u32,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        let max_duration = std::time::Duration::from_millis(max_duration_ms as u64);
        let tick_interval = std::time::Duration::from_millis(1000 / fps.max(1) as u64);

        let mut interval = tokio::time::interval(tick_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Cache PID→(app_name, window_id) to avoid repeated window list lookups
        // when the frontmost app hasn't changed.
        let mut pid_cache: HashMap<i32, (String, u32)> = HashMap::new();

        loop {
            interval.tick().await;

            if cancel.is_cancelled() || start.elapsed() >= max_duration {
                return;
            }

            // Get the frontmost app and capture its window on the blocking pool.
            let output_dir = output_dir.clone();
            let cache = pid_cache.clone();

            let result = tokio::task::spawn_blocking(move || {
                capture_frontmost_frame(&output_dir, &cache)
            })
            .await;

            match result {
                Ok(Ok((frame, pid))) => {
                    pid_cache.insert(pid, (frame.app_name.clone(), frame.window_id));
                    frames.lock().unwrap().push(frame);
                }
                Ok(Err(e)) => {
                    tracing::debug!("Frame capture failed: {e}");
                }
                Err(e) => {
                    tracing::debug!("Frame capture task panicked: {e}");
                }
            }
        }
    })
}

/// Capture a single frame from the frontmost app's window.
///
/// Returns the recorded frame metadata and the frontmost PID (for cache update).
fn capture_frontmost_frame(
    output_dir: &std::path::Path,
    pid_cache: &HashMap<i32, (String, u32)>,
) -> Result<(RecordedFrame, i32), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::macos;

        let pid = macos::ax::frontmost_pid()?;

        // Try cache first, fall back to window list lookup.
        let (app_name, window_id) = if let Some(cached) = pid_cache.get(&pid) {
            cached.clone()
        } else {
            let windows = macos::window::list_windows()
                .map_err(|e| format!("Failed to list windows: {e}"))?;
            let win = windows
                .iter()
                .find(|w| w.owner_pid == pid as i64 && w.is_on_screen)
                .ok_or_else(|| format!("No on-screen window for PID {pid}"))?;
            (win.owner_name.clone(), win.id)
        };

        let timestamp_ms = now_millis();
        let (jpeg_data, meta) = macos::screenshot::capture_window_cg_jpeg(window_id, JPEG_QUALITY)
            .map_err(|e| format!("Capture failed: {e}"))?;

        let filename = format!("frame_{timestamp_ms}.jpg");
        let path = output_dir.join(&filename);
        std::fs::write(&path, &jpeg_data)
            .map_err(|e| format!("Failed to write frame: {e}"))?;

        Ok((
            RecordedFrame {
                timestamp_ms,
                path: path.to_string_lossy().to_string(),
                app_name,
                window_id,
                origin_x: meta.origin_x,
                origin_y: meta.origin_y,
                scale: meta.scale,
                pixel_width: meta.pixel_width,
                pixel_height: meta.pixel_height,
            },
            pid,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (output_dir, pid_cache);
        Err("Screen recording is only supported on macOS".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recorded_frame_serialization() {
        let frame = RecordedFrame {
            timestamp_ms: 1710400000000,
            path: "/tmp/frame_1710400000000.jpg".to_string(),
            app_name: "Finder".to_string(),
            window_id: 42,
            origin_x: 100.0,
            origin_y: 200.0,
            scale: 2.0,
            pixel_width: 1920,
            pixel_height: 1080,
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"timestamp_ms\":1710400000000"));
        assert!(json.contains("\"app_name\":\"Finder\""));
        assert!(json.contains("\"window_id\":42"));
    }

    #[test]
    fn test_drain_frames_clears_buffer() {
        let frames = Arc::new(Mutex::new(vec![RecordedFrame {
            timestamp_ms: 1000,
            path: "/tmp/f.jpg".to_string(),
            app_name: "Test".to_string(),
            window_id: 1,
            origin_x: 0.0,
            origin_y: 0.0,
            scale: 2.0,
            pixel_width: 100,
            pixel_height: 100,
        }]));
        let cancel = CancellationToken::new();
        let recorder = ScreenRecorder::new(
            frames.clone(),
            tokio::runtime::Runtime::new().unwrap().spawn(async {}),
            cancel,
        );

        let drained = recorder.drain_frames();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].timestamp_ms, 1000);

        let drained2 = recorder.drain_frames();
        assert!(drained2.is_empty());
    }

    #[tokio::test]
    async fn test_start_recording_cancellation() {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();
        let dir = tempfile::tempdir().unwrap();

        let handle = start_recording(
            frames.clone(),
            cancel.clone(),
            dir.path().to_path_buf(),
            3,
            10000,
        );

        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_millis(500), handle)
            .await
            .expect("task should finish after cancel")
            .expect("task should not panic");
    }
}
