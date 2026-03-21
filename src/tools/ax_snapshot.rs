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
}

/// Formats a slice of [`AXSnapshotNode`]s into an indented text representation.
///
/// Each line has the form:
/// `<indent>uid=<N> <role> ["<name>"] [value="<val>"] [focused] [disabled] [expanded] [selected]`
///
/// - Indent is 2 spaces per depth level.
/// - `name` is quoted and only shown when `Some`.
/// - `value` is shown as `value="..."` only when `Some`.
/// - Boolean flags (`focused`, `disabled`) are shown only when `true`.
/// - `expanded` is shown only when `Some(true)`.
/// - `selected` is shown only when `Some(true)`.
pub fn format_snapshot(nodes: &[AXSnapshotNode]) -> String {
    let mut lines = Vec::with_capacity(nodes.len());

    for node in nodes {
        let indent = "  ".repeat(node.depth as usize);
        let mut parts = vec![format!("uid={} {}", node.uid, node.role)];

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
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "textbox",
        50005 => "link",
        50006 => "img",
        50008 => "list",
        50011 => "menuitem",
        50014 => "scrollbar",
        50019 => "tablist",
        50020 => "text",
        50021 => "toolbar",
        50026 => "generic",
        50036 => "table",
        _ => return format!("unknown_{}", control_type_id),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_snapshot_basic() {
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
            },
        ];

        let result = format_snapshot(&nodes);
        assert_eq!(
            result,
            "uid=1 RootWebArea \"Page Title\"\n  uid=2 button \"Submit\"\n  uid=3 textbox value=\"hello\" focused"
        );
    }

    #[test]
    fn test_format_snapshot_with_attributes() {
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
        }];

        let result = format_snapshot(&nodes);
        assert_eq!(result, "uid=1 checkbox \"Remember me\" disabled selected");
    }

    #[test]
    fn test_format_snapshot_empty_name_omitted() {
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
        }];

        let result = format_snapshot(&nodes);
        assert_eq!(result, "uid=1 generic");
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
        assert_eq!(map_uia_control_type(50002), "checkbox");
        assert_eq!(map_uia_control_type(50003), "combobox");
        assert_eq!(map_uia_control_type(50004), "textbox");
        assert_eq!(map_uia_control_type(50005), "link");
        assert_eq!(map_uia_control_type(50006), "img");
        assert_eq!(map_uia_control_type(50008), "list");
        assert_eq!(map_uia_control_type(50011), "menuitem");
        assert_eq!(map_uia_control_type(50014), "scrollbar");
        assert_eq!(map_uia_control_type(50019), "tablist");
        assert_eq!(map_uia_control_type(50020), "text");
        assert_eq!(map_uia_control_type(50021), "toolbar");
        assert_eq!(map_uia_control_type(50026), "generic");
        assert_eq!(map_uia_control_type(50036), "table");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_map_windows_control_type_unknown() {
        assert_eq!(map_uia_control_type(99999), "unknown_99999");
    }
}
