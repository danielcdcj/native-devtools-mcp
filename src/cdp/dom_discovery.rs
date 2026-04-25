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
/// `page_url` and `generation` are stamped onto the resulting
/// [`SnapshotMap`] so stale snapshots are detected at lookup time.
pub fn build_dom_snapshot(
    candidates: &[DomCandidate],
    page_url: String,
    generation: u64,
) -> SnapshotMap {
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
        page_url,
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

    // Fallback: collect the element's own text. Prefer direct text nodes
    // (what this element itself says) over any descendant text. Only walk
    // into children — with pruning — if the element has no direct text of
    // its own. This prevents a header button wrapping avatar + name +
    // badges from collapsing all descendant textContent into a composite
    // label like "Note to Self 1 week Verified".
    const own = ownTextNodes(el);
    if (own) return own.substring(0, 200);

    const nested = directOwnText(el).trim().substring(0, 200);
    if (nested) return nested;

    // Last resort: tag name, so we never concatenate descendant text.
    return el.tagName.toLowerCase();
}}

function hasOwnLabel(el) {{
    return el.hasAttribute("aria-label")
        || el.hasAttribute("aria-labelledby")
        || el.hasAttribute("title")
        || el.hasAttribute("alt")
        || el.hasAttribute("role")
        || el.hasAttribute("data-testid");
}}

// Returns the concatenation of this element's *direct* child text nodes
// (nodeType === 3), normalised. Ignores descendant elements entirely. This
// is the preferred label source: it captures what the element itself says
// without picking up styled-span badges or avatar text.
function ownTextNodes(el) {{
    let out = "";
    for (const child of el.childNodes) {{
        if (child.nodeType === Node.TEXT_NODE) {{
            out += child.nodeValue;
        }}
    }}
    return out.replace(/\s+/g, " ").trim();
}}

function directOwnText(el) {{
    let out = "";
    for (const child of el.childNodes) {{
        if (child.nodeType === Node.TEXT_NODE) {{
            out += child.nodeValue;
        }} else if (child.nodeType === Node.ELEMENT_NODE) {{
            // Prune subtrees that own their own label or are interactive on
            // their own — their text belongs to them, not the ancestor.
            if (hasOwnLabel(child)) continue;
            if (isInteractive(child)) continue;
            out += " " + directOwnText(child);
        }}
    }}
    return out.replace(/\s+/g, " ").trim();
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
///
/// Includes parent context (`role` / optional `name`) when the walker
/// captured one, so an LLM can disambiguate, e.g., a sidebar list row from
/// a chat-header button that happen to carry the same label text.
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
        if !node.parent_role.is_empty() {
            if node.parent_name.is_empty() {
                parts.push(format!("(in {})", node.parent_role));
            } else {
                parts.push(format!(
                    "(in {} \"{}\")",
                    node.parent_role, node.parent_name
                ));
            }
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

        let map = build_dom_snapshot(&candidates, "about:blank".to_string(), 0);
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

        let map = build_dom_snapshot(&candidates, "about:blank".to_string(), 0);
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
            "uid=d1 button \"Submit\" tag=button (in form \"Login\")\nuid=d2 textbox tag=input disabled"
        );
    }

    #[test]
    fn format_dom_snapshot_parent_role_only() {
        // parent_role set, parent_name empty → emit only the role.
        let candidates = vec![DomCandidate {
            backend_node_id: 1,
            role: "button".to_string(),
            label: "Send".to_string(),
            tag: "button".to_string(),
            disabled: false,
            parent_role: "nav".to_string(),
            parent_name: "".to_string(),
        }];

        let result = format_dom_snapshot(&candidates);
        assert_eq!(result, "uid=d1 button \"Send\" tag=button (in nav)");
    }

    #[test]
    fn format_dom_snapshot_disambiguates_sidebar_vs_header() {
        // Regression: the clickweave agent once confused a sidebar row and a
        // chat-header button that both surfaced the label "Note to Self" in
        // the snapshot. With parent context rendered, the two lines are
        // visibly distinct.
        let candidates = vec![
            DomCandidate {
                backend_node_id: 100,
                role: "button".to_string(),
                label: "Note to Self".to_string(),
                tag: "li".to_string(),
                disabled: false,
                parent_role: "list".to_string(),
                parent_name: "Chats".to_string(),
            },
            DomCandidate {
                backend_node_id: 200,
                role: "button".to_string(),
                label: "Note to Self".to_string(),
                tag: "button".to_string(),
                disabled: false,
                parent_role: "header".to_string(),
                parent_name: "".to_string(),
            },
        ];

        let result = format_dom_snapshot(&candidates);
        assert_eq!(
            result,
            "uid=d1 button \"Note to Self\" tag=li (in list \"Chats\")\n\
             uid=d2 button \"Note to Self\" tag=button (in header)"
        );
    }

    #[test]
    fn dom_walker_js_uses_direct_text_fallback() {
        // Sanity: the emitted JS includes the direct-text helpers and no
        // longer relies on bare `el.textContent` for the label fallback.
        let js = dom_walker_js("anything", None, 1);
        assert!(
            js.contains("directOwnText"),
            "expected directOwnText helper in walker JS"
        );
        assert!(
            js.contains("hasOwnLabel"),
            "expected hasOwnLabel helper in walker JS"
        );
        // The old fallback concatenated all descendants via `el.textContent`.
        // Guard against regressing to that.
        assert!(
            !js.contains("el.textContent || \"\""),
            "walker JS must not fall back to raw el.textContent for label"
        );
    }

    #[test]
    fn dom_walker_js_prefers_element_own_text_over_descendants() {
        // Primary defence: the walker should look at an element's direct
        // text nodes first (ownTextNodes) and only descend into children
        // when the element has no direct text of its own. Without this,
        // header buttons like Signal's "Note to Self" chat header collapse
        // their sibling badge spans ("1 week", "Verified") into a composite
        // label.
        let js = dom_walker_js("anything", None, 1);
        assert!(
            js.contains("ownTextNodes"),
            "expected ownTextNodes helper (direct text nodes) in walker JS"
        );
        // The helper must filter for TEXT_NODE only — no ELEMENT_NODE branch.
        let helper_start = js
            .find("function ownTextNodes")
            .expect("ownTextNodes should be defined");
        // The next function definition marks the end of ownTextNodes.
        let helper_rest = &js[helper_start..];
        let after_header = helper_rest.find('{').expect("ownTextNodes has no body") + 1;
        let helper_end = helper_rest[after_header..]
            .find("function ")
            .expect("ownTextNodes body not followed by another function")
            + after_header;
        let helper_body = &helper_rest[..helper_end];
        assert!(
            helper_body.contains("Node.TEXT_NODE"),
            "ownTextNodes must inspect TEXT_NODE children"
        );
        assert!(
            !helper_body.contains("ELEMENT_NODE"),
            "ownTextNodes must NOT descend into element children"
        );
        // getLabel must try ownTextNodes before directOwnText so the direct
        // path wins whenever the element has any direct text at all.
        let get_label_start = js.find("function getLabel").expect("getLabel must exist");
        let get_label_body = &js[get_label_start..];
        let own_idx = get_label_body
            .find("ownTextNodes(el)")
            .expect("getLabel must call ownTextNodes");
        let nested_idx = get_label_body
            .find("directOwnText(el)")
            .expect("getLabel must call directOwnText as fallback");
        assert!(
            own_idx < nested_idx,
            "getLabel must call ownTextNodes before directOwnText"
        );
    }

    #[test]
    fn dom_walker_js_hasownlabel_covers_role_and_testid() {
        // Belt-and-braces: role= and data-testid on a descendant are strong
        // signals it's a semantic unit of its own; pruning them makes the
        // recursive fallback safer if we ever fall back to it (e.g. buttons
        // whose text actually lives inside a wrapper span).
        let js = dom_walker_js("anything", None, 1);
        let start = js
            .find("function hasOwnLabel")
            .expect("hasOwnLabel must exist");
        let rest = &js[start..];
        let after_header = rest.find('{').expect("hasOwnLabel has no body") + 1;
        let end = rest[after_header..]
            .find("function ")
            .expect("hasOwnLabel body not followed by another function")
            + after_header;
        let body = &rest[..end];
        assert!(body.contains("\"role\""), "hasOwnLabel should prune role=");
        assert!(
            body.contains("\"data-testid\""),
            "hasOwnLabel should prune data-testid="
        );
    }

    #[test]
    fn format_dom_snapshot_header_button_wraps_badges() {
        // Structural regression: in Signal's web-based chat header, the
        // title button wraps "Note to Self" plus unlabelled badge spans
        // ("1 week", "Verified"). With the new direct-text-first logic,
        // the DOM walker reports the button's direct text only, so
        // format_dom_snapshot emits a clean label — not a composite.
        //
        // This test validates the downstream rendering given candidates
        // that already reflect the new walker output: label is just
        // "Note to Self", and the header context is preserved for
        // disambiguation against sidebar rows.
        let candidates = vec![DomCandidate {
            backend_node_id: 321,
            role: "button".to_string(),
            label: "Note to Self".to_string(),
            tag: "button".to_string(),
            disabled: false,
            parent_role: "header".to_string(),
            parent_name: "".to_string(),
        }];

        let result = format_dom_snapshot(&candidates);
        assert_eq!(
            result,
            "uid=d1 button \"Note to Self\" tag=button (in header)"
        );
        assert!(
            !result.contains("1 week"),
            "badge text must not leak into the header button label"
        );
        assert!(
            !result.contains("Verified"),
            "verified badge must not leak into the header button label"
        );
    }
}
