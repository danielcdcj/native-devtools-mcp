use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::Serialize;

use super::device::AndroidDevice;

#[derive(Debug, Clone, Serialize)]
pub struct UiElement {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub bounds: UiBounds,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

const DUMP_PATH: &str = "/sdcard/ui_dump.xml";

/// Result of a find_text search, including available element names on empty results.
pub struct FindTextResult {
    pub matches: Vec<UiElement>,
    /// Populated only when `matches` is empty — lists all visible element names.
    pub available_elements: Vec<String>,
}

/// Find UI elements matching `search` text (case-insensitive) via `uiautomator dump`.
///
/// Tries dumping to `/dev/tty` first (fast, avoids file I/O). If the device doesn't
/// return XML inline (e.g. Samsung), falls back to dumping to a temp file on the device.
///
/// When no matches are found, `available_elements` is populated with all visible
/// element names so the caller can suggest corrections.
pub fn find_text(device: &mut AndroidDevice, search: &str) -> Result<FindTextResult, String> {
    let xml = dump_ui_xml(device)?;
    let matches = search_xml(&xml, search);
    let available_elements = if matches.is_empty() {
        collect_element_names_xml(&xml)
    } else {
        Vec::new()
    };
    Ok(FindTextResult {
        matches,
        available_elements,
    })
}

/// Dump the UI hierarchy XML from the device.
fn dump_ui_xml(device: &mut AndroidDevice) -> Result<String, String> {
    // Try /dev/tty first — works on most AOSP-based devices.
    let output = device
        .shell("uiautomator dump /dev/tty")
        .map_err(|e| format!("uiautomator dump failed: {}", e))?;

    if let Some(xml_start) = output.find('<') {
        return Ok(output[xml_start..].to_string());
    }

    // Fallback: dump to file (required on Samsung and some other OEMs).
    let output = device
        .shell(&format!(
            "uiautomator dump {path} && cat {path} && rm -f {path}",
            path = DUMP_PATH
        ))
        .map_err(|e| format!("uiautomator dump (file fallback) failed: {}", e))?;

    let xml_start = output.find('<').ok_or_else(|| {
        format!(
            "UI dump failed — device may be locked or showing a system dialog. Raw output: {}",
            &output[..output.len().min(200)]
        )
    })?;

    Ok(output[xml_start..].to_string())
}

/// Parse a bounds string `[x1,y1][x2,y2]` into `(x1, y1, x2, y2)`.
fn parse_bounds(bounds_str: &str) -> Option<(f64, f64, f64, f64)> {
    let stripped = bounds_str.trim();
    if !stripped.starts_with('[') {
        return None;
    }

    let parts: Vec<&str> = stripped.split(']').collect();
    if parts.len() < 2 {
        return None;
    }

    let first = parts[0].trim_start_matches('[');
    let second = parts[1].trim_start_matches('[');

    let p1: Vec<&str> = first.split(',').collect();
    let p2: Vec<&str> = second.split(',').collect();

    if p1.len() != 2 || p2.len() != 2 {
        return None;
    }

    let x1 = p1[0].parse::<f64>().ok()?;
    let y1 = p1[1].parse::<f64>().ok()?;
    let x2 = p2[0].parse::<f64>().ok()?;
    let y2 = p2[1].parse::<f64>().ok()?;

    Some((x1, y1, x2, y2))
}

/// Collect all unique non-empty element names (text and content-desc) from the UI hierarchy XML.
fn collect_element_names_xml(xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key != b"text" && key != b"content-desc" {
                        continue;
                    }
                    let value = attr.unescape_value().unwrap_or_default();
                    let trimmed = value.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                        names.push(trimmed.to_string());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    names
}

/// Search UI hierarchy XML for elements whose `text` or `content-desc` contains `search` (case-insensitive).
fn search_xml(xml: &str, search: &str) -> Vec<UiElement> {
    let search_lower = search.to_lowercase();
    let mut results = Vec::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let mut text_attr = String::new();
                let mut content_desc_attr = String::new();
                let mut bounds_attr = String::new();

                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"text" => {
                            text_attr = attr.unescape_value().unwrap_or_default().to_string()
                        }
                        b"content-desc" => {
                            content_desc_attr =
                                attr.unescape_value().unwrap_or_default().to_string()
                        }
                        b"bounds" => {
                            bounds_attr = attr.unescape_value().unwrap_or_default().to_string()
                        }
                        _ => {}
                    }
                }

                let text_match =
                    !text_attr.is_empty() && text_attr.to_lowercase().contains(&search_lower);
                let desc_match = !content_desc_attr.is_empty()
                    && content_desc_attr.to_lowercase().contains(&search_lower);

                if (text_match || desc_match) && !bounds_attr.is_empty() {
                    if let Some((x1, y1, x2, y2)) = parse_bounds(&bounds_attr) {
                        let display_text = if text_match {
                            text_attr
                        } else {
                            content_desc_attr
                        };

                        results.push(UiElement {
                            text: display_text,
                            x: (x1 + x2) / 2.0,
                            y: (y1 + y2) / 2.0,
                            bounds: UiBounds {
                                x: x1,
                                y: y1,
                                width: x2 - x1,
                                height: y2 - y1,
                            },
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("Error parsing UI hierarchy XML: {}", e);
                break;
            }
            _ => {}
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<hierarchy rotation="0">
  <node index="0" text="Settings" resource-id="com.android.settings:id/title"
        class="android.widget.TextView" package="com.android.settings"
        content-desc="" checkable="false" checked="false" clickable="true"
        enabled="true" focusable="true" focused="false"
        bounds="[56,200][400,260]" />
  <node index="1" text="" resource-id=""
        class="android.widget.ImageView" package="com.android.settings"
        content-desc="Navigate up" checkable="false" checked="false"
        clickable="true" enabled="true" focusable="true" focused="false"
        bounds="[0,66][140,210]" />
  <node index="2" text="Wi-Fi" resource-id="com.android.settings:id/title"
        class="android.widget.TextView" package="com.android.settings"
        content-desc="" checkable="false" checked="false" clickable="true"
        enabled="true" focusable="true" focused="false"
        bounds="[56,300][400,360]" />
</hierarchy>"#;

    #[test]
    fn test_parse_bounds_valid() {
        let result = parse_bounds("[56,200][400,260]");
        assert!(result.is_some());
        let (x1, y1, x2, y2) = result.unwrap();
        assert_eq!(x1, 56.0);
        assert_eq!(y1, 200.0);
        assert_eq!(x2, 400.0);
        assert_eq!(y2, 260.0);
    }

    #[test]
    fn test_parse_bounds_invalid() {
        assert!(parse_bounds("").is_none());
        assert!(parse_bounds("invalid").is_none());
        assert!(parse_bounds("[56,200]").is_none());
        assert!(parse_bounds("[56,200][abc,260]").is_none());
    }

    #[test]
    fn test_search_xml_finds_text_match() {
        let results = search_xml(SAMPLE_XML, "Settings");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Settings");
        // Center of [56,200][400,260] = (228, 230)
        assert_eq!(results[0].x, 228.0);
        assert_eq!(results[0].y, 230.0);
        assert_eq!(results[0].bounds.x, 56.0);
        assert_eq!(results[0].bounds.y, 200.0);
        assert_eq!(results[0].bounds.width, 344.0);
        assert_eq!(results[0].bounds.height, 60.0);
    }

    #[test]
    fn test_search_xml_case_insensitive() {
        let results = search_xml(SAMPLE_XML, "settings");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Settings");
    }

    #[test]
    fn test_search_xml_matches_content_desc() {
        let results = search_xml(SAMPLE_XML, "Navigate up");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Navigate up");
        // Center of [0,66][140,210] = (70, 138)
        assert_eq!(results[0].x, 70.0);
        assert_eq!(results[0].y, 138.0);
    }

    #[test]
    fn test_search_xml_no_match() {
        let results = search_xml(SAMPLE_XML, "Bluetooth");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_xml_partial_match() {
        let results = search_xml(SAMPLE_XML, "Wi");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Wi-Fi");
    }
}
