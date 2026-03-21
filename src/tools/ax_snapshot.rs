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
}
