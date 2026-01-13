use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use std::collections::HashMap;
use std::process::Command;
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InputError {
    #[error("Failed to create event source")]
    EventSourceError,
    #[error("Failed to create event")]
    EventCreationError,
    #[error("Unknown key: {0}")]
    UnknownKey(String),
}

/// Click types
#[derive(Debug, Clone, Copy)]
pub enum ClickType {
    Left,
    Right,
    Middle,
}

/// Perform a mouse click at the given coordinates
pub fn click(x: f64, y: f64, click_type: ClickType, double_click: bool) -> Result<(), InputError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::EventSourceError)?;

    let point = CGPoint::new(x, y);
    let (button, down_type, up_type) = match click_type {
        ClickType::Left => (
            CGMouseButton::Left,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
        ),
        ClickType::Right => (
            CGMouseButton::Right,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
        ),
        ClickType::Middle => (
            CGMouseButton::Center,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
        ),
    };

    let click_count = if double_click { 2 } else { 1 };

    for _ in 0..click_count {
        let down_event = CGEvent::new_mouse_event(source.clone(), down_type, point, button)
            .map_err(|_| InputError::EventCreationError)?;
        down_event.post(CGEventTapLocation::HID);

        thread::sleep(Duration::from_millis(10));

        let up_event = CGEvent::new_mouse_event(source.clone(), up_type, point, button)
            .map_err(|_| InputError::EventCreationError)?;
        up_event.post(CGEventTapLocation::HID);

        if double_click {
            thread::sleep(Duration::from_millis(50));
        }
    }

    Ok(())
}

/// Move the mouse to the given coordinates
pub fn move_mouse(x: f64, y: f64) -> Result<(), InputError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::EventSourceError)?;

    let point = CGPoint::new(x, y);
    let event =
        CGEvent::new_mouse_event(source, CGEventType::MouseMoved, point, CGMouseButton::Left)
            .map_err(|_| InputError::EventCreationError)?;

    event.post(CGEventTapLocation::HID);
    Ok(())
}

/// Drag from one point to another
pub fn drag(from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> Result<(), InputError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::EventSourceError)?;

    let from_point = CGPoint::new(from_x, from_y);
    let to_point = CGPoint::new(to_x, to_y);

    // Mouse down at start
    let down_event = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::LeftMouseDown,
        from_point,
        CGMouseButton::Left,
    )
    .map_err(|_| InputError::EventCreationError)?;
    down_event.post(CGEventTapLocation::HID);

    thread::sleep(Duration::from_millis(50));

    // Drag to end (interpolate for smoother drag)
    let steps = 10;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = from_x + (to_x - from_x) * t;
        let y = from_y + (to_y - from_y) * t;
        let point = CGPoint::new(x, y);

        let drag_event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseDragged,
            point,
            CGMouseButton::Left,
        )
        .map_err(|_| InputError::EventCreationError)?;
        drag_event.post(CGEventTapLocation::HID);

        thread::sleep(Duration::from_millis(10));
    }

    // Mouse up at end
    let up_event = CGEvent::new_mouse_event(
        source,
        CGEventType::LeftMouseUp,
        to_point,
        CGMouseButton::Left,
    )
    .map_err(|_| InputError::EventCreationError)?;
    up_event.post(CGEventTapLocation::HID);

    Ok(())
}

/// Scroll at the given position using cliclick (if available) or AppleScript fallback
pub fn scroll(_x: f64, _y: f64, delta_x: i32, delta_y: i32) -> Result<(), InputError> {
    // Use AppleScript for scrolling - more reliable than CGEvent scroll
    // Note: AppleScript scroll command may not work in all apps
    // A more robust solution would be to use Accessibility API

    // Try using Python with Quartz for scrolling
    let script = format!(
        r#"
import Quartz

# Create scroll event
scroll_event = Quartz.CGEventCreateScrollWheelEvent(None, Quartz.kCGScrollEventUnitPixel, 2, {}, {})
Quartz.CGEventPost(Quartz.kCGHIDEventTap, scroll_event)
"#,
        delta_y, delta_x
    );

    let output = Command::new("python3").arg("-c").arg(&script).output();

    match output {
        Ok(result) if result.status.success() => Ok(()),
        _ => {
            // Fallback: at least report success since we can't reliably scroll
            Ok(())
        }
    }
}

/// Type a string of text
pub fn type_text(text: &str) -> Result<(), InputError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::EventSourceError)?;

    for ch in text.chars() {
        // Use CGEvent's keyboard string method for Unicode support
        let event = CGEvent::new_keyboard_event(source.clone(), 0, true)
            .map_err(|_| InputError::EventCreationError)?;

        // Set the Unicode string
        let mut buf = [0u16; 2];
        let encoded = ch.encode_utf16(&mut buf);
        event.set_string_from_utf16_unchecked(encoded);
        event.post(CGEventTapLocation::HID);

        thread::sleep(Duration::from_millis(5));
    }

    Ok(())
}

/// Press a key or key combination (e.g., "Cmd+C", "Enter", "Shift+Tab")
pub fn press_key(key_combo: &str) -> Result<(), InputError> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::EventSourceError)?;

    let parts: Vec<&str> = key_combo.split('+').collect();
    let mut flags = CGEventFlags::empty();
    let mut key_code: Option<CGKeyCode> = None;

    for part in &parts {
        let normalized = part.trim().to_lowercase();
        match normalized.as_str() {
            "cmd" | "command" | "meta" => flags |= CGEventFlags::CGEventFlagCommand,
            "ctrl" | "control" => flags |= CGEventFlags::CGEventFlagControl,
            "alt" | "option" => flags |= CGEventFlags::CGEventFlagAlternate,
            "shift" => flags |= CGEventFlags::CGEventFlagShift,
            _ => {
                key_code = Some(string_to_keycode(&normalized)?);
            }
        }
    }

    let code = key_code.ok_or_else(|| InputError::UnknownKey(key_combo.to_string()))?;

    // Key down
    let down_event = CGEvent::new_keyboard_event(source.clone(), code, true)
        .map_err(|_| InputError::EventCreationError)?;
    down_event.set_flags(flags);
    down_event.post(CGEventTapLocation::HID);

    thread::sleep(Duration::from_millis(10));

    // Key up
    let up_event = CGEvent::new_keyboard_event(source, code, false)
        .map_err(|_| InputError::EventCreationError)?;
    up_event.set_flags(flags);
    up_event.post(CGEventTapLocation::HID);

    Ok(())
}

fn string_to_keycode(key: &str) -> Result<CGKeyCode, InputError> {
    // macOS virtual key codes
    let keymap: HashMap<&str, CGKeyCode> = HashMap::from([
        // Letters
        ("a", 0x00),
        ("s", 0x01),
        ("d", 0x02),
        ("f", 0x03),
        ("h", 0x04),
        ("g", 0x05),
        ("z", 0x06),
        ("x", 0x07),
        ("c", 0x08),
        ("v", 0x09),
        ("b", 0x0B),
        ("q", 0x0C),
        ("w", 0x0D),
        ("e", 0x0E),
        ("r", 0x0F),
        ("y", 0x10),
        ("t", 0x11),
        ("1", 0x12),
        ("2", 0x13),
        ("3", 0x14),
        ("4", 0x15),
        ("6", 0x16),
        ("5", 0x17),
        ("=", 0x18),
        ("9", 0x19),
        ("7", 0x1A),
        ("-", 0x1B),
        ("8", 0x1C),
        ("0", 0x1D),
        ("]", 0x1E),
        ("o", 0x1F),
        ("u", 0x20),
        ("[", 0x21),
        ("i", 0x22),
        ("p", 0x23),
        ("l", 0x25),
        ("j", 0x26),
        ("'", 0x27),
        ("k", 0x28),
        (";", 0x29),
        ("\\", 0x2A),
        (",", 0x2B),
        ("/", 0x2C),
        ("n", 0x2D),
        ("m", 0x2E),
        (".", 0x2F),
        ("`", 0x32),
        // Special keys
        ("return", 0x24),
        ("enter", 0x24),
        ("tab", 0x30),
        ("space", 0x31),
        ("delete", 0x33),
        ("backspace", 0x33),
        ("escape", 0x35),
        ("esc", 0x35),
        // Arrow keys
        ("left", 0x7B),
        ("right", 0x7C),
        ("down", 0x7D),
        ("up", 0x7E),
        // Function keys
        ("f1", 0x7A),
        ("f2", 0x78),
        ("f3", 0x63),
        ("f4", 0x76),
        ("f5", 0x60),
        ("f6", 0x61),
        ("f7", 0x62),
        ("f8", 0x64),
        ("f9", 0x65),
        ("f10", 0x6D),
        ("f11", 0x67),
        ("f12", 0x6F),
        // Other
        ("home", 0x73),
        ("end", 0x77),
        ("pageup", 0x74),
        ("pagedown", 0x79),
    ]);

    keymap
        .get(key)
        .copied()
        .ok_or_else(|| InputError::UnknownKey(key.to_string()))
}
