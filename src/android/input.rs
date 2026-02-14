use super::device::AndroidDevice;

/// Tap at the given screen coordinates.
pub fn click(device: &mut AndroidDevice, x: f64, y: f64) -> Result<(), String> {
    let x_str = x.to_string();
    let y_str = y.to_string();
    device.shell_args(&["input", "tap", &x_str, &y_str])?;
    Ok(())
}

/// Swipe from one point to another, optionally over a given duration.
pub fn swipe(
    device: &mut AndroidDevice,
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
    duration_ms: Option<u32>,
) -> Result<(), String> {
    let sx = start_x.to_string();
    let sy = start_y.to_string();
    let ex = end_x.to_string();
    let ey = end_y.to_string();

    match duration_ms {
        Some(ms) => {
            let ms_str = ms.to_string();
            device.shell_args(&["input", "swipe", &sx, &sy, &ex, &ey, &ms_str])?;
        }
        None => {
            device.shell_args(&["input", "swipe", &sx, &sy, &ex, &ey])?;
        }
    }
    Ok(())
}

/// Type text on the device. Special shell characters are escaped.
pub fn type_text(device: &mut AndroidDevice, text: &str) -> Result<(), String> {
    let escaped = escape_for_input(text);
    device.shell_args(&["input", "text", &escaped])?;
    Ok(())
}

/// Press a key by its keycode name (e.g. "KEYCODE_HOME") or numeric code.
pub fn press_key(device: &mut AndroidDevice, key: &str) -> Result<(), String> {
    device.shell_args(&["input", "keyevent", key])?;
    Ok(())
}

/// Escape text for use with `adb shell input text`.
///
/// The `input text` command runs through the shell, so certain characters
/// need escaping. Spaces become `%s`, and shell metacharacters are
/// backslash-escaped.
fn escape_for_input(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        match c {
            ' ' => result.push_str("%s"),
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\'' => result.push_str("\\'"),
            '&' => result.push_str("\\&"),
            '|' => result.push_str("\\|"),
            ';' => result.push_str("\\;"),
            '(' => result.push_str("\\("),
            ')' => result.push_str("\\)"),
            '<' => result.push_str("\\<"),
            '>' => result.push_str("\\>"),
            '`' => result.push_str("\\`"),
            '$' => result.push_str("\\$"),
            '!' => result.push_str("\\!"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_spaces() {
        assert_eq!(escape_for_input("hello world"), "hello%sworld");
    }

    #[test]
    fn test_escape_special_chars() {
        assert_eq!(escape_for_input("a&b"), "a\\&b");
        assert_eq!(escape_for_input("it's"), "it\\'s");
        assert_eq!(escape_for_input("a\"b"), "a\\\"b");
    }

    #[test]
    fn test_escape_no_special() {
        assert_eq!(escape_for_input("hello123"), "hello123");
    }
}
