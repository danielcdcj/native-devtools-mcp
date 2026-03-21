//! Convert CDP accessibility tree nodes to AXSnapshotNode format.

use super::{SnapshotMap, SnapshotNode};
use crate::tools::ax_snapshot::AXSnapshotNode;
use std::collections::HashMap;

/// Convert a CDP `Accessibility.getFullAXTree` response into our snapshot format.
///
/// Returns a flat list of [`AXSnapshotNode`]s in DFS order and a [`SnapshotMap`]
/// that maps UIDs to backend node identifiers needed for click/eval resolution.
pub fn convert_cdp_ax_tree(
    nodes: &[serde_json::Value],
    page_url: &str,
) -> (Vec<AXSnapshotNode>, SnapshotMap) {
    if nodes.is_empty() {
        return (
            Vec::new(),
            SnapshotMap {
                uid_to_node: HashMap::new(),
                page_url: page_url.to_string(),
                navigation_id: None,
            },
        );
    }

    // Step 1: build nodeId → array-index map.
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        if let Some(node_id) = node["nodeId"].as_str() {
            id_to_index.insert(node_id.to_string(), i);
        }
    }

    // Step 2: build nodeId → children-indices map from childIds.
    let mut children_map: HashMap<String, Vec<usize>> = HashMap::new();
    for node in nodes.iter() {
        let node_id = match node["nodeId"].as_str() {
            Some(id) => id.to_string(),
            None => continue,
        };
        let child_ids: Vec<usize> = node["childIds"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|cid| id_to_index.get(cid).copied())
                    .collect()
            })
            .unwrap_or_default();
        children_map.insert(node_id, child_ids);
    }

    // Step 3: DFS from root (first node), assigning sequential UIDs.
    let mut snapshot_nodes: Vec<AXSnapshotNode> = Vec::new();
    let mut uid_to_node: HashMap<String, SnapshotNode> = HashMap::new();
    let mut uid_counter: u32 = 1;

    // Stack entries: (node_index, depth)
    let mut stack: Vec<(usize, u32)> = vec![(0, 0)];

    while let Some((idx, depth)) = stack.pop() {
        let node = &nodes[idx];
        let uid = uid_counter;
        uid_counter += 1;

        // Extract role.
        let role = node["role"]["value"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        // Extract name (omit if empty).
        let name = node["name"]["value"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Extract properties.
        let mut value: Option<String> = None;
        let mut focused = false;
        let mut disabled = false;
        let mut expanded: Option<bool> = None;
        let mut selected: Option<bool> = None;

        if let Some(props) = node["properties"].as_array() {
            for prop in props {
                let prop_name = prop["name"].as_str().unwrap_or("");
                match prop_name {
                    "value" => {
                        value = prop["value"]["value"].as_str().map(|s| s.to_string());
                    }
                    "focused" => {
                        focused = prop["value"]["value"].as_bool().unwrap_or(false);
                    }
                    "disabled" => {
                        disabled = prop["value"]["value"].as_bool().unwrap_or(false);
                    }
                    "expanded" => {
                        expanded = prop["value"]["value"].as_bool();
                    }
                    "selected" => {
                        selected = prop["value"]["value"].as_bool();
                    }
                    _ => {}
                }
            }
        }

        // Record snapshot map entry.
        let backend_node_id = node["backendDOMNodeId"].as_i64().unwrap_or(0);
        uid_to_node.insert(
            uid.to_string(),
            SnapshotNode {
                backend_node_id,
                role: role.clone(),
                name: name.clone().unwrap_or_default(),
            },
        );

        snapshot_nodes.push(AXSnapshotNode {
            uid,
            role,
            name,
            value,
            focused,
            disabled,
            expanded,
            selected,
            depth,
        });

        // Push children in reverse order so the first child is processed first.
        let node_id = node["nodeId"].as_str().unwrap_or("").to_string();
        if let Some(child_indices) = children_map.get(&node_id) {
            for &child_idx in child_indices.iter().rev() {
                stack.push((child_idx, depth + 1));
            }
        }
    }

    let snapshot_map = SnapshotMap {
        uid_to_node,
        page_url: page_url.to_string(),
        navigation_id: None,
    };

    (snapshot_nodes, snapshot_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_cdp_ax_tree_basic() {
        let nodes = vec![
            json!({
                "nodeId": "1",
                "role": {"type": "role", "value": "RootWebArea"},
                "name": {"type": "computedString", "value": "Test Page"},
                "backendDOMNodeId": 1,
                "childIds": ["2", "3"],
                "properties": []
            }),
            json!({
                "nodeId": "2",
                "role": {"type": "role", "value": "button"},
                "name": {"type": "computedString", "value": "Click me"},
                "backendDOMNodeId": 5,
                "childIds": [],
                "properties": []
            }),
            json!({
                "nodeId": "3",
                "role": {"type": "role", "value": "textbox"},
                "name": {"type": "computedString", "value": ""},
                "backendDOMNodeId": 8,
                "childIds": [],
                "properties": [
                    {"name": "value", "value": {"type": "string", "value": "hello"}},
                    {"name": "focused", "value": {"type": "boolean", "value": true}}
                ]
            }),
        ];

        let (snapshot_nodes, snapshot_map) = convert_cdp_ax_tree(&nodes, "https://example.com");

        // Verify 3 nodes produced.
        assert_eq!(snapshot_nodes.len(), 3);

        // Verify UIDs, roles, names, depths.
        assert_eq!(snapshot_nodes[0].uid, 1);
        assert_eq!(snapshot_nodes[0].role, "RootWebArea");
        assert_eq!(snapshot_nodes[0].name, Some("Test Page".to_string()));
        assert_eq!(snapshot_nodes[0].depth, 0);

        assert_eq!(snapshot_nodes[1].uid, 2);
        assert_eq!(snapshot_nodes[1].role, "button");
        assert_eq!(snapshot_nodes[1].name, Some("Click me".to_string()));
        assert_eq!(snapshot_nodes[1].depth, 1);

        assert_eq!(snapshot_nodes[2].uid, 3);
        assert_eq!(snapshot_nodes[2].role, "textbox");
        assert_eq!(snapshot_nodes[2].name, None); // empty name omitted
        assert_eq!(snapshot_nodes[2].depth, 1);
        assert_eq!(snapshot_nodes[2].value, Some("hello".to_string()));
        assert!(snapshot_nodes[2].focused);

        // Verify SnapshotMap has 3 entries with correct backend_node_ids.
        assert_eq!(snapshot_map.uid_to_node.len(), 3);
        assert_eq!(snapshot_map.uid_to_node["1"].backend_node_id, 1);
        assert_eq!(snapshot_map.uid_to_node["2"].backend_node_id, 5);
        assert_eq!(snapshot_map.uid_to_node["3"].backend_node_id, 8);

        // Verify page_url.
        assert_eq!(snapshot_map.page_url, "https://example.com");
    }
}
