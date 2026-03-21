//! CDP input tools: click, hover, fill, press_key.

use super::{resolve_element_center, resolve_node, resolve_to_object_id};
use crate::cdp::{cdp_error, CdpClient};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    MouseButton,
};
use chromiumoxide::cdp::js_protocol::runtime::{CallArgument, CallFunctionOnParams};
use rmcp::model::{CallToolResult, Content};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn cdp_click(
    uid: String,
    dbl_click: bool,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let (node_role, node_name, cx, cy) = match resolve_element_center(&uid, client, &page).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    drop(guard);

    let click_count = if dbl_click { 2_i64 } else { 1_i64 };

    let move_event = DispatchMouseEventParams::new(DispatchMouseEventType::MouseMoved, cx, cy);

    let mut press_event =
        DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, cx, cy);
    press_event.button = Some(MouseButton::Left);
    press_event.buttons = Some(1);
    press_event.click_count = Some(click_count);

    let mut release_event =
        DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, cx, cy);
    release_event.button = Some(MouseButton::Left);
    release_event.click_count = Some(click_count);

    for event in [move_event, press_event, release_event] {
        if let Err(e) = page.execute(event).await {
            return cdp_error(format!("Click failed on uid={}: {}", uid, e));
        }
    }

    let dbl_note = if dbl_click { " (double-click)" } else { "" };
    CallToolResult::success(vec![Content::text(format!(
        "Clicked uid={} '{}' ({}) at ({:.1}, {:.1}){}",
        uid, node_name, node_role, cx, cy, dbl_note
    ))])
}

pub async fn cdp_hover(uid: String, cdp_client: Arc<RwLock<Option<CdpClient>>>) -> CallToolResult {
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let (node_role, node_name, cx, cy) = match resolve_element_center(&uid, client, &page).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    drop(guard);

    let move_event = DispatchMouseEventParams::new(DispatchMouseEventType::MouseMoved, cx, cy);
    if let Err(e) = page.execute(move_event).await {
        return cdp_error(format!("Hover failed on uid={}: {}", uid, e));
    }

    CallToolResult::success(vec![Content::text(format!(
        "Hovered uid={} '{}' ({}) at ({:.1}, {:.1})",
        uid, node_name, node_role, cx, cy
    ))])
}

pub async fn cdp_fill(
    uid: String,
    value: String,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let (backend_node_id, node_role, node_name) = match resolve_node(&uid, client, &page).await {
        Ok(v) => v,
        Err(e) => return e,
    };

    drop(guard);

    let object_id = match resolve_to_object_id(&uid, backend_node_id, &page).await {
        Ok(id) => id,
        Err(e) => return e,
    };

    let fill_fn = r#"function(value) {
        if (this.tagName === 'SELECT') {
            const option = Array.from(this.options).find(o => o.value === value || o.textContent.trim() === value);
            if (!option) throw new Error('Option not found: ' + value);
            this.value = option.value;
            this.dispatchEvent(new Event('input', { bubbles: true }));
            this.dispatchEvent(new Event('change', { bubbles: true }));
            return;
        }
        this.focus();
        if (this.select) this.select();
        else document.execCommand('selectAll', false, null);
        document.execCommand('insertText', false, value);
    }"#;

    let call_params = match CallFunctionOnParams::builder()
        .function_declaration(fill_fn)
        .object_id(object_id)
        .arguments(vec![CallArgument::builder()
            .value(serde_json::Value::String(value.clone()))
            .build()])
        .await_promise(true)
        .build()
    {
        Ok(p) => p,
        Err(e) => return cdp_error(format!("Failed to build call params: {}", e)),
    };

    match page.execute(call_params).await {
        Ok(resp) => {
            if let Some(exc) = &resp.result.exception_details {
                return cdp_error(format!("Fill failed: {}", exc.text));
            }
            CallToolResult::success(vec![Content::text(format!(
                "Filled uid={} '{}' ({}) with '{}'",
                uid, node_name, node_role, value
            ))])
        }
        Err(e) => cdp_error(format!("Fill failed on uid={}: {}", uid, e)),
    }
}

/// Map a key name to its CDP key identifier, code, and Windows virtual key code.
fn key_definition(key: &str) -> Option<(&'static str, &'static str, i64)> {
    Some(match key {
        "Enter" => ("Enter", "Enter", 13),
        "Tab" => ("Tab", "Tab", 9),
        "Escape" => ("Escape", "Escape", 27),
        "Backspace" => ("Backspace", "Backspace", 8),
        "Delete" => ("Delete", "Delete", 46),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        "Home" => ("Home", "Home", 36),
        "End" => ("End", "End", 35),
        "PageUp" => ("PageUp", "PageUp", 33),
        "PageDown" => ("PageDown", "PageDown", 34),
        "Space" | " " => (" ", "Space", 32),
        "F1" => ("F1", "F1", 112),
        "F2" => ("F2", "F2", 113),
        "F3" => ("F3", "F3", 114),
        "F4" => ("F4", "F4", 115),
        "F5" => ("F5", "F5", 116),
        "F6" => ("F6", "F6", 117),
        "F7" => ("F7", "F7", 118),
        "F8" => ("F8", "F8", 119),
        "F9" => ("F9", "F9", 120),
        "F10" => ("F10", "F10", 121),
        "F11" => ("F11", "F11", 122),
        "F12" => ("F12", "F12", 123),
        _ if key.len() == 1 => return None,
        _ => return None,
    })
}

const MODIFIER_ALT: i64 = 1;
const MODIFIER_CONTROL: i64 = 2;
const MODIFIER_META: i64 = 4;
const MODIFIER_SHIFT: i64 = 8;

/// Map a modifier name to its CDP bitmask.
fn modifier_bit(name: &str) -> Option<i64> {
    match name {
        "Alt" => Some(MODIFIER_ALT),
        "Control" => Some(MODIFIER_CONTROL),
        "Meta" => Some(MODIFIER_META),
        "Shift" => Some(MODIFIER_SHIFT),
        _ => None,
    }
}

/// Parsed key combination: modifier bitmask + main key name.
#[derive(Debug)]
struct ParsedKeyCombo {
    modifiers: i64,
    modifier_names: Vec<String>,
    main_key: String,
}

/// Parse a key combo string like "Control+Shift+A" or "Control++".
/// Returns Err with the unknown modifier name on failure.
fn parse_key_combo(key: &str) -> Result<ParsedKeyCombo, String> {
    let parts: Vec<&str> = key.split('+').collect();

    let (modifier_parts, main_key) = if key.ends_with("++") {
        (&parts[..parts.len() - 2], "+")
    } else if parts.len() > 1 {
        (&parts[..parts.len() - 1], *parts.last().unwrap_or(&""))
    } else {
        (&[][..], parts[0])
    };

    let mut modifiers: i64 = 0;
    let mut modifier_names = Vec::new();
    for &m in modifier_parts {
        match modifier_bit(m) {
            Some(bit) => {
                modifiers |= bit;
                modifier_names.push(m.to_string());
            }
            None => return Err(m.to_string()),
        }
    }

    Ok(ParsedKeyCombo {
        modifiers,
        modifier_names,
        main_key: main_key.to_string(),
    })
}

pub async fn cdp_press_key(
    key: String,
    cdp_client: Arc<RwLock<Option<CdpClient>>>,
) -> CallToolResult {
    let guard = cdp_client.read().await;
    let client = match guard.as_ref() {
        Some(c) => c,
        None => return cdp_error("No CDP connection. Use cdp_connect first."),
    };

    let page = match client.require_page() {
        Ok(p) => p,
        Err(e) => return e,
    };

    drop(guard);

    let combo = match parse_key_combo(&key) {
        Ok(c) => c,
        Err(unknown) => {
            return cdp_error(format!(
                "Unknown modifier '{}'. Use Control, Shift, Alt, or Meta.",
                unknown
            ))
        }
    };
    let modifiers = combo.modifiers;
    let main_key = &combo.main_key;

    // Dispatch modifier key-downs.
    for m in &combo.modifier_names {
        let mut params = DispatchKeyEventParams::new(DispatchKeyEventType::KeyDown);
        params.key = Some(m.clone());
        params.modifiers = Some(modifiers);
        if let Err(e) = page.execute(params).await {
            return cdp_error(format!("Failed to press modifier {}: {}", m, e));
        }
    }

    // Dispatch main key.
    if let Some((key_val, code, vk)) = key_definition(main_key) {
        let mut down = DispatchKeyEventParams::new(DispatchKeyEventType::RawKeyDown);
        down.key = Some(key_val.to_string());
        down.code = Some(code.to_string());
        down.windows_virtual_key_code = Some(vk);
        down.modifiers = Some(modifiers);
        if let Err(e) = page.execute(down).await {
            return cdp_error(format!("Failed to press key {}: {}", main_key, e));
        }

        let mut up = DispatchKeyEventParams::new(DispatchKeyEventType::KeyUp);
        up.key = Some(key_val.to_string());
        up.code = Some(code.to_string());
        up.windows_virtual_key_code = Some(vk);
        up.modifiers = Some(modifiers);
        if let Err(e) = page.execute(up).await {
            return cdp_error(format!("Failed to release key {}: {}", main_key, e));
        }
    } else if main_key.len() == 1 {
        let ch = main_key.chars().next().unwrap_or(' ');
        let upper = ch.to_ascii_uppercase();
        let vk = upper as i64;

        let mut down = DispatchKeyEventParams::new(DispatchKeyEventType::RawKeyDown);
        down.key = Some(main_key.to_string());
        down.code = Some(format!("Key{}", upper));
        down.windows_virtual_key_code = Some(vk);
        down.modifiers = Some(modifiers);
        if let Err(e) = page.execute(down).await {
            return cdp_error(format!("Failed to press key {}: {}", main_key, e));
        }

        // Only send Char event if no modifiers (otherwise it's a shortcut).
        if modifiers == 0 || modifiers == MODIFIER_SHIFT {
            let text = if modifiers & MODIFIER_SHIFT != 0 {
                upper.to_string()
            } else {
                ch.to_string()
            };
            let mut char_event = DispatchKeyEventParams::new(DispatchKeyEventType::Char);
            char_event.text = Some(text);
            char_event.modifiers = Some(modifiers);
            let _ = page.execute(char_event).await;
        }

        let mut up = DispatchKeyEventParams::new(DispatchKeyEventType::KeyUp);
        up.key = Some(main_key.to_string());
        up.code = Some(format!("Key{}", upper));
        up.windows_virtual_key_code = Some(vk);
        up.modifiers = Some(modifiers);
        if let Err(e) = page.execute(up).await {
            return cdp_error(format!("Failed to release key {}: {}", main_key, e));
        }
    } else {
        return cdp_error(format!(
            "Unknown key '{}'. Use key names like Enter, Tab, ArrowUp, or single characters.",
            main_key
        ));
    }

    // Release modifier keys in reverse order.
    for m in combo.modifier_names.iter().rev() {
        let mut params = DispatchKeyEventParams::new(DispatchKeyEventType::KeyUp);
        params.key = Some(m.clone());
        let _ = page.execute(params).await;
    }

    CallToolResult::success(vec![Content::text(format!("Pressed key: {}", key))])
}

#[cfg(test)]
mod tests {
    use super::*;

    // MARK: - key_definition tests

    #[test]
    fn key_definition_returns_enter() {
        let (key, code, vk) = key_definition("Enter").unwrap();
        assert_eq!(key, "Enter");
        assert_eq!(code, "Enter");
        assert_eq!(vk, 13);
    }

    #[test]
    fn key_definition_returns_tab() {
        let (_, _, vk) = key_definition("Tab").unwrap();
        assert_eq!(vk, 9);
    }

    #[test]
    fn key_definition_returns_arrow_keys() {
        assert_eq!(key_definition("ArrowUp").unwrap().2, 38);
        assert_eq!(key_definition("ArrowDown").unwrap().2, 40);
        assert_eq!(key_definition("ArrowLeft").unwrap().2, 37);
        assert_eq!(key_definition("ArrowRight").unwrap().2, 39);
    }

    #[test]
    fn key_definition_returns_space() {
        let (key, code, vk) = key_definition("Space").unwrap();
        assert_eq!(key, " ");
        assert_eq!(code, "Space");
        assert_eq!(vk, 32);
        // Also works with literal space.
        assert!(key_definition(" ").is_some());
    }

    #[test]
    fn key_definition_returns_f_keys() {
        assert_eq!(key_definition("F1").unwrap().2, 112);
        assert_eq!(key_definition("F12").unwrap().2, 123);
    }

    #[test]
    fn key_definition_returns_none_for_single_char() {
        assert!(key_definition("a").is_none());
        assert!(key_definition("Z").is_none());
        assert!(key_definition("1").is_none());
    }

    #[test]
    fn key_definition_returns_none_for_unknown() {
        assert!(key_definition("FooBar").is_none());
        assert!(key_definition("").is_none());
    }

    // MARK: - modifier_bit tests

    #[test]
    fn modifier_bit_returns_correct_values() {
        assert_eq!(modifier_bit("Alt"), Some(1));
        assert_eq!(modifier_bit("Control"), Some(2));
        assert_eq!(modifier_bit("Meta"), Some(4));
        assert_eq!(modifier_bit("Shift"), Some(8));
    }

    #[test]
    fn modifier_bit_returns_none_for_unknown() {
        assert_eq!(modifier_bit("Ctrl"), None);
        assert_eq!(modifier_bit("alt"), None);
        assert_eq!(modifier_bit(""), None);
    }

    // MARK: - parse_key_combo tests

    #[test]
    fn parse_single_key() {
        let combo = parse_key_combo("Enter").unwrap();
        assert_eq!(combo.main_key, "Enter");
        assert_eq!(combo.modifiers, 0);
        assert!(combo.modifier_names.is_empty());
    }

    #[test]
    fn parse_single_character() {
        let combo = parse_key_combo("a").unwrap();
        assert_eq!(combo.main_key, "a");
        assert_eq!(combo.modifiers, 0);
    }

    #[test]
    fn parse_control_a() {
        let combo = parse_key_combo("Control+A").unwrap();
        assert_eq!(combo.main_key, "A");
        assert_eq!(combo.modifiers, MODIFIER_CONTROL);
        assert_eq!(combo.modifier_names, vec!["Control"]);
    }

    #[test]
    fn parse_control_shift_r() {
        let combo = parse_key_combo("Control+Shift+R").unwrap();
        assert_eq!(combo.main_key, "R");
        assert_eq!(combo.modifiers, MODIFIER_CONTROL | MODIFIER_SHIFT);
        assert_eq!(combo.modifier_names, vec!["Control", "Shift"]);
    }

    #[test]
    fn parse_control_plus_key() {
        // "Control++" means Control + the '+' key.
        let combo = parse_key_combo("Control++").unwrap();
        assert_eq!(combo.main_key, "+");
        assert_eq!(combo.modifiers, MODIFIER_CONTROL);
    }

    #[test]
    fn parse_all_modifiers() {
        let combo = parse_key_combo("Alt+Control+Meta+Shift+x").unwrap();
        assert_eq!(combo.main_key, "x");
        assert_eq!(
            combo.modifiers,
            MODIFIER_ALT | MODIFIER_CONTROL | MODIFIER_META | MODIFIER_SHIFT
        );
        assert_eq!(combo.modifier_names.len(), 4);
    }

    #[test]
    fn parse_unknown_modifier_returns_error() {
        let err = parse_key_combo("Ctrl+A").unwrap_err();
        assert_eq!(err, "Ctrl");
    }

    #[test]
    fn parse_meta_enter() {
        let combo = parse_key_combo("Meta+Enter").unwrap();
        assert_eq!(combo.main_key, "Enter");
        assert_eq!(combo.modifiers, MODIFIER_META);
    }
}
