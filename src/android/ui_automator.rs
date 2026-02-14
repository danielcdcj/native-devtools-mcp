use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::Serialize;

use super::device::AndroidDevice;

/// A UI element found on the Android device screen.
#[derive(Debug, Clone, Serialize)]
pub struct UiElement {
    /// The text content of the element.
    pub text: String,
    /// Center X coordinate of the element bounds.
    pub x: f64,
    /// Center Y coordinate of the element bounds.
    pub y: f64,
    /// Bounding rectangle of the element.
    pub bounds: UiBounds,
}

/// Bounding rectangle for a UI element in screen coordinates.
#[derive(Debug, Clone, Serialize)]
pub struct UiBounds {
    /// Left edge X coordinate.
    pub x: f64,
    /// Top edge Y coordinate.
    pub y: f64,
    /// Width of the bounding rectangle.
    pub width: f64,
    /// Height of the bounding rectangle.
    pub height: f64,
}

/// Find UI elements on the device that match the given search text.
///
/// Runs `uiautomator dump` to capture the view hierarchy, then searches
/// the XML for elements whose `text` or `content-desc` attributes contain
/// the search string (case-insensitive).
pub fn find_text(device: &mut AndroidDevice, search: &str) -> Result<Vec<UiElement>, String> {
    let xml = device
        .shell("uiautomator dump /dev/tty")
        .map_err(|e| format!("uiautomator dump failed: {}", e))?;

    Ok(search_xml(&xml, search))
}

/// Parse a bounds string of the form `[x1,y1][x2,y2]` into `(x1, y1, x2, y2)`.
fn parse_bounds(bounds_str: &str) -> Option<(f64, f64, f64, f64)> {
    // Expected format: [x1,y1][x2,y2]
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

/// Search parsed UI hierarchy XML for elements matching the search text.
///
/// Matches against both `text` and `content-desc` attributes (case-insensitive,
/// substring match).
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
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = attr.unescape_value().unwrap_or_default().to_string();

                    match key.as_str() {
                        "text" => text_attr = value,
                        "content-desc" => content_desc_attr = value,
                        "bounds" => bounds_attr = value,
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
