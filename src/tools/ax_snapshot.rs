use serde::Deserialize;

#[derive(Deserialize)]
pub struct TakeAxSnapshotParams {
    pub app_name: Option<String>,
}

/// Axis-aligned rectangle in screen points. Used for snapshot bboxes and
/// for the bbox carried on `ax_click` / `ax_set_value` responses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Collect the accessibility tree and format as snapshot text.
///
/// On macOS the server handler in `server.rs` bypasses this function
/// entirely (it drives `AxSession` directly so uids carry a generation tag
/// and stale-uid rejection works by construction). The macOS arm here
/// remains as a transitional compat path between Task 2 and Task 9; once
/// Task 9 lands the server wiring, the macOS arm is deleted.
pub fn take_ax_snapshot(params: TakeAxSnapshotParams) -> Result<String, String> {
    let nodes = {
        #[cfg(target_os = "macos")]
        {
            let (nodes, _refs) =
                crate::macos::ax::collect_ax_tree_indexed(params.app_name.as_deref())?;
            nodes
        }
        #[cfg(target_os = "windows")]
        {
            crate::windows::uia::collect_uia_tree(params.app_name.as_deref())?
        }
    };
    Ok(format_snapshot(&nodes, None))
}

/// A single node in a serialized accessibility tree snapshot.
/// Used by both AX/UIA snapshots (Phase 1) and CDP snapshots (Phase 2).
pub struct AXSnapshotNode {
    pub uid: u32,
    pub role: String,
    pub name: Option<String>,
    pub value: Option<String>,
    pub focused: bool,
    pub disabled: bool,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub depth: u32,
    /// Screen-point bbox. Populated by the macOS AX collector when the
    /// element exposes `kAXPositionAttribute` and `kAXSizeAttribute`.
    /// Always `None` from Windows UIA and CDP collectors.
    pub bbox: Option<Rect>,
}

/// Format snapshot nodes into the text representation.
///
/// When `generation` is `Some(g)`, each uid renders as `a<N>g<g>` (macOS AX,
/// post-dispatch-branch format). When `None`, each uid renders as bare `a<N>`
/// (preserves exact CDP output byte-for-byte).
///
/// When a node has `bbox: Some`, a trailing `bbox=(x,y,w,h)` attribute is
/// appended. Coordinates are formatted as plain integers via truncating
/// cast — decimal values from AX are not expected in practice (AX returns
/// i32-backed CGPoint/CGSize), and integer output keeps the line stable for
/// human readers and downstream parsers.
pub fn format_snapshot(nodes: &[AXSnapshotNode], generation: Option<u64>) -> String {
    let mut lines = Vec::with_capacity(nodes.len());

    for node in nodes {
        let indent = "  ".repeat(node.depth as usize);

        let uid_tag = match generation {
            Some(g) => format!("uid=a{}g{}", node.uid, g),
            None => format!("uid=a{}", node.uid),
        };
        let mut parts = vec![format!("{} {}", uid_tag, node.role)];

        if let Some(name) = &node.name {
            parts.push(format!("\"{}\"", name));
        }
        if let Some(value) = &node.value {
            parts.push(format!("value=\"{}\"", value));
        }
        if node.focused {
            parts.push("focused".to_string());
        }
        if node.disabled {
            parts.push("disabled".to_string());
        }
        if node.expanded == Some(true) {
            parts.push("expanded".to_string());
        }
        if node.selected == Some(true) {
            parts.push("selected".to_string());
        }
        if let Some(bbox) = &node.bbox {
            parts.push(format!(
                "bbox=({},{},{},{})",
                bbox.x as i64, bbox.y as i64, bbox.w as i64, bbox.h as i64
            ));
        }

        lines.push(format!("{}{}", indent, parts.join(" ")));
    }

    lines.join("\n")
}

/// Maps a macOS AXRole string to a short CDP-style role name.
///
/// Known roles are mapped to their CDP equivalents. Unknown roles have the
/// "AX" prefix stripped and the remainder lowercased.
pub fn map_ax_role(ax_role: &str) -> String {
    match ax_role {
        "AXButton" => "button",
        "AXStaticText" => "text",
        "AXTextField" | "AXTextArea" => "textbox",
        "AXCheckBox" => "checkbox",
        "AXWebArea" => "RootWebArea",
        "AXGroup" => "generic",
        "AXLink" => "link",
        "AXImage" => "img",
        "AXList" => "list",
        "AXHeading" => "heading",
        "AXMenuItem" => "menuitem",
        "AXTable" => "table",
        "AXRow" => "row",
        "AXCell" => "cell",
        "AXTabGroup" => "tablist",
        "AXComboBox" | "AXPopUpButton" => "combobox",
        "AXScrollArea" => "scrollbar",
        "AXToolbar" => "toolbar",
        "AXRadioButton" => "radio",
        "AXSlider" => "slider",
        "AXProgressIndicator" => "progressbar",
        unknown => {
            let stripped = unknown.strip_prefix("AX").unwrap_or(unknown);
            return stripped.to_lowercase();
        }
    }
    .to_string()
}

/// Maps a Windows UIA ControlType ID to a short CDP-style role name.
///
/// Unknown IDs are formatted as `unknown_<id>`.
#[cfg(target_os = "windows")]
pub fn map_uia_control_type(control_type_id: i32) -> String {
    match control_type_id {
        50000 => "button",
        50001 => "calendar",
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "textbox",
        50005 => "link",
        50006 => "img",
        50007 => "listitem",
        50008 => "list",
        50009 => "menu",
        50010 => "menubar",
        50011 => "menuitem",
        50012 => "progressbar",
        50013 => "radio",
        50014 => "scrollbar",
        50015 => "slider",
        50016 => "spinner",
        50017 => "statusbar",
        50018 => "tab",
        50019 => "tablist",
        50020 => "text",
        50021 => "toolbar",
        50022 => "tooltip",
        50023 => "tree",
        50024 => "treeitem",
        50025 => "custom",
        50026 => "generic",
        50027 => "thumb",
        50028 => "datagrid",
        50029 => "dataitem",
        50030 => "document",
        50031 => "splitbutton",
        50032 => "window",
        50033 => "pane",
        50034 => "header",
        50035 => "headeritem",
        50036 => "table",
        50037 => "titlebar",
        50038 => "separator",
        50039 => "semanticzoom",
        50040 => "appbar",
        _ => return format!("unknown_{}", control_type_id),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_snapshot_basic_preserves_cdp_output_with_none_generation() {
        let nodes = vec![
            AXSnapshotNode {
                uid: 1,
                role: "RootWebArea".to_string(),
                name: Some("Page Title".to_string()),
                value: None,
                focused: false,
                disabled: false,
                expanded: None,
                selected: None,
                depth: 0,
                bbox: None,
            },
            AXSnapshotNode {
                uid: 2,
                role: "button".to_string(),
                name: Some("Submit".to_string()),
                value: None,
                focused: false,
                disabled: false,
                expanded: None,
                selected: None,
                depth: 1,
                bbox: None,
            },
            AXSnapshotNode {
                uid: 3,
                role: "textbox".to_string(),
                name: None,
                value: Some("hello".to_string()),
                focused: true,
                disabled: false,
                expanded: None,
                selected: None,
                depth: 1,
                bbox: None,
            },
        ];

        let result = format_snapshot(&nodes, None);
        assert_eq!(
            result,
            "uid=a1 RootWebArea \"Page Title\"\n  uid=a2 button \"Submit\"\n  uid=a3 textbox value=\"hello\" focused"
        );
    }

    #[test]
    fn test_format_snapshot_with_attributes_none_generation() {
        let nodes = vec![AXSnapshotNode {
            uid: 1,
            role: "checkbox".to_string(),
            name: Some("Remember me".to_string()),
            value: None,
            focused: false,
            disabled: true,
            expanded: Some(false),
            selected: Some(true),
            depth: 0,
            bbox: None,
        }];

        let result = format_snapshot(&nodes, None);
        assert_eq!(result, "uid=a1 checkbox \"Remember me\" disabled selected");
    }

    #[test]
    fn test_format_snapshot_empty_name_omitted_none_generation() {
        let nodes = vec![AXSnapshotNode {
            uid: 1,
            role: "generic".to_string(),
            name: None,
            value: None,
            focused: false,
            disabled: false,
            expanded: None,
            selected: None,
            depth: 0,
            bbox: None,
        }];

        let result = format_snapshot(&nodes, None);
        assert_eq!(result, "uid=a1 generic");
    }

    #[test]
    fn test_format_snapshot_emits_generation_tag_when_some() {
        let nodes = vec![AXSnapshotNode {
            uid: 42,
            role: "button".to_string(),
            name: Some("5".to_string()),
            value: None,
            focused: false,
            disabled: false,
            expanded: None,
            selected: None,
            depth: 0,
            bbox: None,
        }];
        let result = format_snapshot(&nodes, Some(3));
        assert_eq!(result, "uid=a42g3 button \"5\"");
    }

    #[test]
    fn test_format_snapshot_emits_bbox_when_some() {
        let nodes = vec![AXSnapshotNode {
            uid: 1,
            role: "button".to_string(),
            name: Some("5".to_string()),
            value: None,
            focused: false,
            disabled: false,
            expanded: None,
            selected: None,
            depth: 0,
            bbox: Some(Rect {
                x: 412.0,
                y: 285.0,
                w: 64.0,
                h: 32.0,
            }),
        }];
        let result = format_snapshot(&nodes, Some(3));
        assert_eq!(result, "uid=a1g3 button \"5\" bbox=(412,285,64,32)");
    }

    #[test]
    fn test_format_snapshot_omits_bbox_when_none_even_with_generation() {
        let nodes = vec![AXSnapshotNode {
            uid: 1,
            role: "generic".to_string(),
            name: None,
            value: None,
            focused: false,
            disabled: false,
            expanded: None,
            selected: None,
            depth: 0,
            bbox: None,
        }];
        let result = format_snapshot(&nodes, Some(3));
        assert_eq!(result, "uid=a1g3 generic");
    }

    #[test]
    fn test_rect_renders_integer_coords_without_trailing_decimals() {
        // Load-bearing: CDP output and existing tests assume plain integers.
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 1440.0,
            h: 900.0,
        };
        assert_eq!(
            format!(
                "bbox=({},{},{},{})",
                rect.x as i64, rect.y as i64, rect.w as i64, rect.h as i64
            ),
            "bbox=(0,0,1440,900)"
        );
    }

    #[test]
    fn test_map_macos_role() {
        assert_eq!(map_ax_role("AXButton"), "button");
        assert_eq!(map_ax_role("AXStaticText"), "text");
        assert_eq!(map_ax_role("AXTextField"), "textbox");
        assert_eq!(map_ax_role("AXTextArea"), "textbox");
        assert_eq!(map_ax_role("AXCheckBox"), "checkbox");
        assert_eq!(map_ax_role("AXWebArea"), "RootWebArea");
        assert_eq!(map_ax_role("AXGroup"), "generic");
        assert_eq!(map_ax_role("AXLink"), "link");
        assert_eq!(map_ax_role("AXImage"), "img");
        assert_eq!(map_ax_role("AXList"), "list");
        assert_eq!(map_ax_role("AXHeading"), "heading");
        assert_eq!(map_ax_role("AXMenuItem"), "menuitem");
        assert_eq!(map_ax_role("AXTable"), "table");
        assert_eq!(map_ax_role("AXRow"), "row");
        assert_eq!(map_ax_role("AXCell"), "cell");
        assert_eq!(map_ax_role("AXTabGroup"), "tablist");
        assert_eq!(map_ax_role("AXComboBox"), "combobox");
        assert_eq!(map_ax_role("AXPopUpButton"), "combobox");
        assert_eq!(map_ax_role("AXScrollArea"), "scrollbar");
        assert_eq!(map_ax_role("AXToolbar"), "toolbar");
        assert_eq!(map_ax_role("AXRadioButton"), "radio");
        assert_eq!(map_ax_role("AXSlider"), "slider");
        assert_eq!(map_ax_role("AXProgressIndicator"), "progressbar");
    }

    #[test]
    fn test_map_macos_role_unknown_passthrough() {
        assert_eq!(map_ax_role("AXSplitGroup"), "splitgroup");
        assert_eq!(map_ax_role("AXOutline"), "outline");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_map_windows_control_type() {
        assert_eq!(map_uia_control_type(50000), "button");
        assert_eq!(map_uia_control_type(50001), "calendar");
        assert_eq!(map_uia_control_type(50002), "checkbox");
        assert_eq!(map_uia_control_type(50003), "combobox");
        assert_eq!(map_uia_control_type(50004), "textbox");
        assert_eq!(map_uia_control_type(50005), "link");
        assert_eq!(map_uia_control_type(50006), "img");
        assert_eq!(map_uia_control_type(50007), "listitem");
        assert_eq!(map_uia_control_type(50008), "list");
        assert_eq!(map_uia_control_type(50009), "menu");
        assert_eq!(map_uia_control_type(50010), "menubar");
        assert_eq!(map_uia_control_type(50011), "menuitem");
        assert_eq!(map_uia_control_type(50012), "progressbar");
        assert_eq!(map_uia_control_type(50013), "radio");
        assert_eq!(map_uia_control_type(50014), "scrollbar");
        assert_eq!(map_uia_control_type(50015), "slider");
        assert_eq!(map_uia_control_type(50016), "spinner");
        assert_eq!(map_uia_control_type(50017), "statusbar");
        assert_eq!(map_uia_control_type(50018), "tab");
        assert_eq!(map_uia_control_type(50019), "tablist");
        assert_eq!(map_uia_control_type(50020), "text");
        assert_eq!(map_uia_control_type(50021), "toolbar");
        assert_eq!(map_uia_control_type(50022), "tooltip");
        assert_eq!(map_uia_control_type(50023), "tree");
        assert_eq!(map_uia_control_type(50024), "treeitem");
        assert_eq!(map_uia_control_type(50025), "custom");
        assert_eq!(map_uia_control_type(50026), "generic");
        assert_eq!(map_uia_control_type(50027), "thumb");
        assert_eq!(map_uia_control_type(50028), "datagrid");
        assert_eq!(map_uia_control_type(50029), "dataitem");
        assert_eq!(map_uia_control_type(50030), "document");
        assert_eq!(map_uia_control_type(50031), "splitbutton");
        assert_eq!(map_uia_control_type(50032), "window");
        assert_eq!(map_uia_control_type(50033), "pane");
        assert_eq!(map_uia_control_type(50034), "header");
        assert_eq!(map_uia_control_type(50035), "headeritem");
        assert_eq!(map_uia_control_type(50036), "table");
        assert_eq!(map_uia_control_type(50037), "titlebar");
        assert_eq!(map_uia_control_type(50038), "separator");
        assert_eq!(map_uia_control_type(50039), "semanticzoom");
        assert_eq!(map_uia_control_type(50040), "appbar");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_map_windows_control_type_unknown() {
        assert_eq!(map_uia_control_type(99999), "unknown_99999");
    }
}
