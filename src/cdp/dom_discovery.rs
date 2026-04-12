//! DOM-native discovery for CDP-connected pages.
//!
//! Walks the live DOM via Runtime.evaluate to find interactive elements,
//! extract semantic labels, and assign d<N> prefixed UIDs.

use super::{SnapshotMap, SnapshotNode};
use std::collections::HashMap;

/// A candidate element extracted from the live DOM.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DomCandidate {
    #[serde(rename = "backendNodeId")]
    pub backend_node_id: i64,
    pub role: String,
    pub label: String,
    pub tag: String,
    pub disabled: bool,
    #[serde(rename = "parentRole")]
    pub parent_role: String,
    #[serde(rename = "parentName")]
    pub parent_name: String,
}

/// Build a SnapshotMap from DOM candidates, assigning d<N> prefixed UIDs.
///
/// `generation` is stamped onto the resulting [`SnapshotMap`] so stale
/// snapshots (e.g. after a reload or SPA navigation) are detected at
/// lookup time.
pub fn build_dom_snapshot(candidates: &[DomCandidate], generation: u64) -> SnapshotMap {
    let mut uid_to_node = HashMap::new();
    let mut backend_to_uids: HashMap<i64, Vec<String>> = HashMap::new();

    for (i, candidate) in candidates.iter().enumerate() {
        let uid_key = format!("d{}", i + 1);

        uid_to_node.insert(
            uid_key.clone(),
            SnapshotNode {
                backend_node_id: candidate.backend_node_id,
                role: candidate.role.clone(),
                name: candidate.label.clone(),
            },
        );

        if candidate.backend_node_id != 0 {
            backend_to_uids
                .entry(candidate.backend_node_id)
                .or_default()
                .push(uid_key);
        }
    }

    SnapshotMap {
        uid_to_node,
        backend_to_uids,
        generation,
    }
}

/// Build the JavaScript expression that walks the DOM and returns interactive candidates.
///
/// The JS walker:
/// 1. Collects interactive elements (buttons, links, inputs, textareas, selects,
///    contenteditable, elements with interactive ARIA roles, tabindex >= 0)
/// 2. Filters invisible elements (display:none, visibility:hidden, aria-hidden, zero-size, inert)
/// 3. Walks open shadow roots and same-origin iframes
/// 4. Returns matched elements and a parallel metadata array,
///    plus an `inventory` summarizing all interactive elements by role
///
/// Returns `{ elements: Element[], metadata: [...], inventory: [...] }` as a non-by-value object.
/// The caller must use `Runtime.callFunctionOn` to extract `metadata` (a JSON array of
/// `DomCandidate` objects) in one bulk call, then `DOM.describeNode` per element to
/// resolve `backendNodeId`.
pub fn dom_walker_js(query: &str, role_filter: Option<&str>, max_results: u32) -> String {
    // Use serde_json::to_string for proper JS string encoding (handles all edge cases)
    let query_json = serde_json::to_string(query).unwrap();
    let role_json = role_filter
        .map(|r| serde_json::to_string(r).unwrap())
        .unwrap_or_else(|| "null".to_string());

    format!(
        r##"(() => {{
const QUERY = {query_json};
const ROLE_FILTER = {role_json};
const MAX = {max_results};

const INTERACTIVE_TAGS = new Set(["BUTTON", "A", "INPUT", "TEXTAREA", "SELECT", "SUMMARY"]);
const INTERACTIVE_ROLES = new Set([
    "button", "checkbox", "combobox", "link", "menuitem", "menuitemcheckbox",
    "menuitemradio", "option", "radio", "searchbox", "slider", "spinbutton",
    "switch", "tab", "textbox", "treeitem"
]);

function isVisible(el) {{
    if (el.closest("[aria-hidden='true']") || el.closest("[inert]")) return false;
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    if (el.offsetWidth === 0 && el.offsetHeight === 0) return false;
    return true;
}}

function getLabel(el) {{
    const ariaLabel = el.getAttribute("aria-label");
    if (ariaLabel) return ariaLabel.trim();

    const labelledBy = el.getAttribute("aria-labelledby");
    if (labelledBy) {{
        const parts = labelledBy.split(/\s+/).map(id => {{
            const ref_ = el.getRootNode().getElementById(id);
            return ref_ ? ref_.textContent.trim() : "";
        }}).filter(Boolean);
        if (parts.length) return parts.join(" ");
    }}

    if (el.labels && el.labels.length) {{
        const txt = Array.from(el.labels).map(l => l.textContent.trim()).join(" ");
        if (txt) return txt;
    }}

    const ph = el.getAttribute("placeholder") || el.getAttribute("data-placeholder");
    if (ph) return ph.trim();

    if (el.tagName === "INPUT" && ["submit", "button", "reset"].includes(el.type)) {{
        if (el.value) return el.value.trim();
    }}

    const title = el.getAttribute("title");
    if (title) return title.trim();

    const alt = el.getAttribute("alt");
    if (alt) return alt.trim();

    const text = el.textContent || "";
    const trimmed = text.trim().substring(0, 200);
    return trimmed;
}}

function getRole(el) {{
    const ariaRole = el.getAttribute("role");
    if (ariaRole && INTERACTIVE_ROLES.has(ariaRole)) return ariaRole;

    const tag = el.tagName;
    if (tag === "BUTTON" || (tag === "INPUT" && ["submit", "button", "reset"].includes(el.type))) return "button";
    if (tag === "A" && el.hasAttribute("href")) return "link";
    if (tag === "INPUT") {{
        const t = el.type || "text";
        if (t === "checkbox") return "checkbox";
        if (t === "radio") return "radio";
        if (t === "search") return "searchbox";
        if (t === "range") return "slider";
        if (t === "number") return "spinbutton";
        return "textbox";
    }}
    if (tag === "TEXTAREA") return "textbox";
    if (tag === "SELECT") return "combobox";
    if (tag === "SUMMARY") return "button";
    if (el.isContentEditable) return "textbox";
    if (ariaRole) return ariaRole;
    return "generic";
}}

function getParentContext(el) {{
    let parent = el.parentElement;
    while (parent) {{
        const role = parent.getAttribute("role");
        if (role) {{
            const name = parent.getAttribute("aria-label") || parent.textContent?.trim().substring(0, 50) || "";
            return {{ role, name }};
        }}
        const tag = parent.tagName;
        if (["NAV", "MAIN", "ASIDE", "HEADER", "FOOTER", "SECTION", "FORM", "DIALOG"].includes(tag)) {{
            const name = parent.getAttribute("aria-label") || "";
            return {{ role: tag.toLowerCase(), name }};
        }}
        parent = parent.parentElement;
    }}
    return {{ role: "", name: "" }};
}}

function isInteractive(el) {{
    if (INTERACTIVE_TAGS.has(el.tagName)) return true;
    if (el.isContentEditable && (!el.parentElement || !el.parentElement.isContentEditable)) return true;
    const role = el.getAttribute("role");
    if (role && INTERACTIVE_ROLES.has(role)) return true;
    const tabindex = el.getAttribute("tabindex");
    if (tabindex !== null && parseInt(tabindex, 10) >= 0) return true;
    return false;
}}

function walk(root, results) {{
    const elements = root.querySelectorAll("*");
    for (const el of elements) {{
        if (isInteractive(el)) {{
            results.push(el);
        }}
        if (el.shadowRoot) {{
            walk(el.shadowRoot, results);
        }}
    }}
    // Same-origin iframes
    if (root === document) {{
        for (const iframe of document.querySelectorAll("iframe")) {{
            try {{
                if (iframe.contentDocument) walk(iframe.contentDocument, results);
            }} catch(e) {{}}
        }}
    }}
}}

const allElements = [];
walk(document, allElements);

const queryLower = QUERY.toLowerCase();
const matchedElements = [];
const metadataArray = [];
const roleCounts = {{}};

for (const el of allElements) {{
    const label = getLabel(el);
    const role = getRole(el);

    // Inventory counts all interactive elements regardless of visibility or text match
    if (!roleCounts[role]) roleCounts[role] = {{ count: 0, labels: [] }};
    roleCounts[role].count++;
    if (roleCounts[role].labels.length < 3 && label) {{
        roleCounts[role].labels.push(label.substring(0, 80));
    }}

    // Match filter: cheap checks first, expensive isVisible last
    if (ROLE_FILTER && role !== ROLE_FILTER) continue;
    if (!label.toLowerCase().includes(queryLower)) continue;
    if (matchedElements.length >= MAX) continue;
    if (!isVisible(el)) continue;

    const tag = el.tagName.toLowerCase();
    const disabled = el.disabled === true || el.getAttribute("aria-disabled") === "true";
    const parent = getParentContext(el);

    // Store metadata in a parallel array (avoids mutating live DOM elements)
    metadataArray.push({{
        backendNodeId: 0,
        role,
        label: label.substring(0, 200),
        tag,
        disabled,
        parentRole: parent.role,
        parentName: parent.name.substring(0, 100),
    }});
    matchedElements.push(el);
}}

const inventory = Object.entries(roleCounts).map(([role, data]) => ({{
    role,
    count: data.count,
    sample_labels: data.labels,
}}));

return {{ elements: matchedElements, metadata: metadataArray, inventory }};
}})()"##
    )
}

/// Format DOM candidates as indented text (similar to AX snapshot format).
pub fn format_dom_snapshot(candidates: &[DomCandidate]) -> String {
    let mut lines = Vec::with_capacity(candidates.len());
    for (i, node) in candidates.iter().enumerate() {
        let mut parts = vec![format!("uid=d{} {}", i + 1, node.role)];
        if !node.label.is_empty() {
            parts.push(format!("\"{}\"", node.label));
        }
        parts.push(format!("tag={}", node.tag));
        if node.disabled {
            parts.push("disabled".to_string());
        }
        lines.push(parts.join(" "));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_dom_snapshot_map_basic() {
        let candidates = vec![
            DomCandidate {
                backend_node_id: 10,
                role: "button".to_string(),
                label: "Submit".to_string(),
                tag: "button".to_string(),
                disabled: false,
                parent_role: "form".to_string(),
                parent_name: "Login".to_string(),
            },
            DomCandidate {
                backend_node_id: 20,
                role: "textbox".to_string(),
                label: "Email".to_string(),
                tag: "input".to_string(),
                disabled: false,
                parent_role: "form".to_string(),
                parent_name: "Login".to_string(),
            },
        ];

        let map = build_dom_snapshot(&candidates, 0);
        assert_eq!(map.uid_to_node.len(), 2);
        assert!(map.uid_to_node.contains_key("d1"));
        assert!(map.uid_to_node.contains_key("d2"));
        assert_eq!(map.uid_to_node["d1"].backend_node_id, 10);
        assert_eq!(map.uid_to_node["d1"].role, "button");
        assert_eq!(map.uid_to_node["d1"].name, "Submit");
    }

    #[test]
    fn build_dom_snapshot_reverse_map() {
        let candidates = vec![DomCandidate {
            backend_node_id: 42,
            role: "button".to_string(),
            label: "Ok".to_string(),
            tag: "button".to_string(),
            disabled: false,
            parent_role: "dialog".to_string(),
            parent_name: "Confirm".to_string(),
        }];

        let map = build_dom_snapshot(&candidates, 0);
        assert_eq!(map.backend_to_uids[&42], vec!["d1"]);
    }

    #[test]
    fn dom_walker_js_is_valid_javascript() {
        let js = dom_walker_js("Search", None, 10);
        // Basic sanity: contains the query and returns elements + inventory
        assert!(js.contains("Search"));
        assert!(js.contains("elements"));
        assert!(js.contains("inventory"));
    }

    #[test]
    fn dom_walker_js_encodes_special_chars() {
        // Verify serde_json encoding handles edge cases
        let js = dom_walker_js("test\"with\\quotes", Some("button"), 5);
        assert!(js.contains(r#"test\"with\\quotes"#));
        assert!(js.contains(r#""button""#));
    }

    #[test]
    fn format_dom_snapshot_basic() {
        let candidates = vec![
            DomCandidate {
                backend_node_id: 10,
                role: "button".to_string(),
                label: "Submit".to_string(),
                tag: "button".to_string(),
                disabled: false,
                parent_role: "form".to_string(),
                parent_name: "Login".to_string(),
            },
            DomCandidate {
                backend_node_id: 20,
                role: "textbox".to_string(),
                label: "".to_string(),
                tag: "input".to_string(),
                disabled: true,
                parent_role: "".to_string(),
                parent_name: "".to_string(),
            },
        ];

        let result = format_dom_snapshot(&candidates);
        assert_eq!(
            result,
            "uid=d1 button \"Submit\" tag=button\nuid=d2 textbox tag=input disabled"
        );
    }
}
