//! Pure row-resolution logic for `ax_select`.
//!
//! Callers hand this module a uid-targeted starting element and an
//! ancestor-walk. The algorithm:
//!
//! 1. Walk up from the start element (inclusive) looking for the first
//!    ancestor with `AXRole == "AXRow"`. That is the row we will select.
//! 2. Walk up from the row (exclusive) looking for the first ancestor whose
//!    `AXRole` is `"AXOutline"` or `"AXTable"`. That is the container whose
//!    `AXSelectedRows` attribute we will write.
//!
//! Failure modes:
//! - `NoRow` — the start element and every walk-limited ancestor failed to
//!   expose an `AXRow`. `ax_select` was aimed at an element that is not
//!   inside a row.
//! - `NoContainer` — a row was found but none of its ancestors within the
//!   walk limit is an outline or table. Unusual — typically means the host
//!   app nests rows inside a non-standard container.
//!
//! # Why a pure function
//!
//! The production walk calls into `AXParent` / `AXRole` FFI which requires
//! live `AXUIElementRef` handles. We cannot construct those in a unit test
//! without going through the full app snapshot path. This module expresses
//! the decision logic as `resolve_row_and_container(roles)` where `roles`
//! is a leaf-to-root slice of role strings the integration layer has
//! already read. That lets us unit-test every branch (cell→row, deeper
//! descendant→row, non-row-descendant fails, row-without-outline fails)
//! without the FFI.

/// Role strings we consider candidate rows and containers.
const ROW_ROLE: &str = "AXRow";
const OUTLINE_ROLE: &str = "AXOutline";
const TABLE_ROLE: &str = "AXTable";

/// Outcome of the row-resolution walk over a precomputed ancestor role chain.
///
/// Returned indices refer into the input slice: `ancestor_roles[0]` is the
/// starting element, `ancestor_roles[1]` its parent, and so on. `row_idx`
/// is always strictly less than `container_idx` because the walk for the
/// container starts one step above the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowResolution {
    Resolved {
        row_idx: usize,
        container_idx: usize,
    },
    NoRow,
    NoContainer {
        row_idx: usize,
    },
}

/// Resolve `(row, container)` over a leaf-to-root role chain.
///
/// `ancestor_roles[0]` is the starting element, each subsequent entry is
/// one parent up. `None` represents an ancestor whose `AXRole` was
/// unreadable — common near the app root, and not fatal unless we run out
/// of ancestors before finding a row or container.
pub(crate) fn resolve_row_and_container(ancestor_roles: &[Option<&str>]) -> RowResolution {
    let Some(row_idx) = ancestor_roles
        .iter()
        .position(|r| matches!(r, Some(role) if *role == ROW_ROLE))
    else {
        return RowResolution::NoRow;
    };

    let Some(offset) = ancestor_roles[row_idx + 1..]
        .iter()
        .position(|r| matches!(r, Some(role) if *role == OUTLINE_ROLE || *role == TABLE_ROLE))
    else {
        return RowResolution::NoContainer { row_idx };
    };

    RowResolution::Resolved {
        row_idx,
        container_idx: row_idx + 1 + offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(s: &str) -> Option<&str> {
        Some(s)
    }

    // Helper — build a role chain from string slices so tests read cleanly.
    fn chain<'a>(roles: &[&'a str]) -> Vec<Option<&'a str>> {
        roles.iter().map(|r| Some(*r)).collect()
    }

    #[test]
    fn row_as_start_resolves_to_itself() {
        // uid pointed directly at the row.
        let roles = chain(&["AXRow", "AXOutline", "AXScrollArea", "AXWindow"]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::Resolved {
                row_idx: 0,
                container_idx: 1,
            }
        );
    }

    #[test]
    fn cell_start_resolves_to_parent_row() {
        // uid pointed at the row's cell.
        let roles = chain(&["AXCell", "AXRow", "AXOutline", "AXScrollArea", "AXWindow"]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::Resolved {
                row_idx: 1,
                container_idx: 2,
            }
        );
    }

    #[test]
    fn deeper_descendant_resolves_to_enclosing_row() {
        // uid pointed at a static-text/image inside a cell inside a row.
        let roles = chain(&[
            "AXStaticText",
            "AXCell",
            "AXRow",
            "AXOutline",
            "AXScrollArea",
            "AXWindow",
        ]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::Resolved {
                row_idx: 2,
                container_idx: 3,
            }
        );
    }

    #[test]
    fn table_instead_of_outline_also_resolves() {
        // NSTableView-backed sidebars report `AXTable`.
        let roles = chain(&["AXCell", "AXRow", "AXTable", "AXScrollArea", "AXWindow"]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::Resolved {
                row_idx: 1,
                container_idx: 2,
            }
        );
    }

    #[test]
    fn non_row_descendant_chain_reports_no_row() {
        // uid pointed at a bare button — no row in any ancestor.
        let roles = chain(&["AXButton", "AXGroup", "AXWindow", "AXApplication"]);
        assert_eq!(resolve_row_and_container(&roles), RowResolution::NoRow);
    }

    #[test]
    fn row_without_outline_container_reports_no_container() {
        // Pathological: row nested in a group that is not outline/table.
        let roles = chain(&["AXCell", "AXRow", "AXGroup", "AXWindow", "AXApplication"]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::NoContainer { row_idx: 1 }
        );
    }

    #[test]
    fn unreadable_ancestor_roles_do_not_abort_walk() {
        // An unreadable `AXRole` on an intermediate element must not block
        // the walk — we keep going until we find the row or run out of
        // ancestors.
        let roles = vec![
            some("AXStaticText"),
            None, // unreadable group
            some("AXRow"),
            None, // unreadable scroll area
            some("AXOutline"),
            some("AXWindow"),
        ];
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::Resolved {
                row_idx: 2,
                container_idx: 4,
            }
        );
    }

    #[test]
    fn empty_chain_reports_no_row() {
        let roles: Vec<Option<&str>> = Vec::new();
        assert_eq!(resolve_row_and_container(&roles), RowResolution::NoRow);
    }

    #[test]
    fn row_with_no_further_ancestors_reports_no_container() {
        // Row at the very top of the walked chain — no outline above.
        let roles = chain(&["AXRow"]);
        assert_eq!(
            resolve_row_and_container(&roles),
            RowResolution::NoContainer { row_idx: 0 }
        );
    }
}
