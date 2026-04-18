//! macOS AX element session cache. See task comment — `AXRef` binding lands in Task 3.

use std::sync::atomic::AtomicU64;
use tokio::sync::RwLock;

/// Reason a uid could not be resolved to a live element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    SnapshotExpired { reason: String },
    UidNotFound,
}

/// Parse a uid of the form `"a<u32>g<u64>"`. Any other shape returns `None`.
pub fn parse_uid(s: &str) -> Option<(u32, u64)> {
    let rest = s.strip_prefix('a')?;
    let g_pos = rest.find('g')?;
    let (n_str, gen_str) = rest.split_at(g_pos);
    let gen_str = &gen_str[1..];
    if n_str.is_empty() || gen_str.is_empty() {
        return None;
    }
    let n: u32 = n_str.parse().ok()?;
    let generation: u64 = gen_str.parse().ok()?;
    Some((n, generation))
}

pub struct AxSnapshot {
    pub generation: u64,
    // refs: HashMap<u32, AXRef> — added in Task 3 after AXRef lands in macos::ax
}

pub struct AxSession {
    current: RwLock<Option<AxSnapshot>>,
    next_generation: AtomicU64,
}

impl Default for AxSession {
    fn default() -> Self {
        Self::new()
    }
}

impl AxSession {
    pub fn new() -> Self {
        Self {
            current: RwLock::new(None),
            next_generation: AtomicU64::new(1),
        }
    }

    pub async fn current_generation(&self) -> Option<u64> {
        self.current.read().await.as_ref().map(|s| s.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uid_accepts_well_formed() {
        assert_eq!(parse_uid("a42g3"), Some((42, 3)));
        assert_eq!(parse_uid("a0g0"), Some((0, 0)));
        assert_eq!(parse_uid("a1g18446744073709551615"), Some((1, u64::MAX)));
    }

    #[test]
    fn parse_uid_rejects_bare_n() {
        assert_eq!(parse_uid("a42"), None);
    }

    #[test]
    fn parse_uid_rejects_missing_gen_number() {
        assert_eq!(parse_uid("a42g"), None);
    }

    #[test]
    fn parse_uid_rejects_missing_n() {
        assert_eq!(parse_uid("ag3"), None);
    }

    #[test]
    fn parse_uid_rejects_non_numeric_gen() {
        assert_eq!(parse_uid("a42gX"), None);
    }

    #[test]
    fn parse_uid_rejects_non_numeric_n() {
        assert_eq!(parse_uid("aXg3"), None);
    }

    #[test]
    fn parse_uid_rejects_empty() {
        assert_eq!(parse_uid(""), None);
    }

    #[test]
    fn parse_uid_rejects_missing_prefix() {
        assert_eq!(parse_uid("42g3"), None);
    }

    #[test]
    fn parse_uid_rejects_uppercase() {
        assert_eq!(parse_uid("A42G3"), None);
        assert_eq!(parse_uid("a42G3"), None);
    }
}
