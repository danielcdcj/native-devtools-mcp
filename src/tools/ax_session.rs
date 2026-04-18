//! macOS AX element session cache.
//!
//! Owns the most recent `take_ax_snapshot` result on macOS. Each snapshot
//! carries a monotonic generation number and a map of retained `AXUIElement`
//! handles keyed by the numeric uid index. Uids are strings of the form
//! `"a<N>g<gen>"` — `ax_click` and `ax_set_value` parse them, check the
//! generation against the current snapshot, and reject stale uids by
//! construction.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use crate::macos::ax::AXRef;

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
    pub refs: HashMap<u32, AXRef>,
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

    /// Peek the current generation without taking a read lock on the snapshot.
    /// Returns `None` until the first snapshot has been created. Currently
    /// exercised only by tests; kept public for observability/debugging.
    #[allow(dead_code)]
    pub async fn current_generation(&self) -> Option<u64> {
        self.current.read().await.as_ref().map(|s| s.generation)
    }

    /// Install a fresh snapshot with the given refs map. Returns the assigned
    /// generation. Drops the prior snapshot (releasing every AXRef in it).
    pub async fn create_snapshot(&self, refs: HashMap<u32, AXRef>) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let mut guard = self.current.write().await;
        *guard = Some(AxSnapshot { generation, refs });
        generation
    }

    /// Resolve a uid string to an `AXRef`.
    ///
    /// * Unparseable uids → `SnapshotExpired` (including legacy bare `a<N>`).
    /// * No current snapshot → `SnapshotExpired`.
    /// * Generation mismatch → `SnapshotExpired`.
    /// * Index missing from current refs → `UidNotFound`.
    /// * Otherwise → `Ok(AXRef)` (Arc-cloned handle; cheap).
    pub async fn lookup(&self, uid: &str) -> Result<AXRef, LookupError> {
        let Some((n, gen)) = parse_uid(uid) else {
            return Err(LookupError::SnapshotExpired {
                reason: format!("uid must match a<N>g<gen>; got: {}", uid),
            });
        };
        let guard = self.current.read().await;
        let Some(snapshot) = guard.as_ref() else {
            return Err(LookupError::SnapshotExpired {
                reason: "no take_ax_snapshot has been called on this server".to_string(),
            });
        };
        if snapshot.generation != gen {
            return Err(LookupError::SnapshotExpired {
                reason: format!(
                    "uid generation g{} does not match current g{}",
                    gen, snapshot.generation
                ),
            });
        }
        snapshot
            .refs
            .get(&n)
            .cloned()
            .ok_or(LookupError::UidNotFound)
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

    // Constructing a dummy `AxSnapshot` without the FFI is not possible because
    // `AXRef` has no safe constructor. Tests here drive the public API that
    // does not require `AXRef` construction (generation bumping, parse failures,
    // empty-snapshot tombstone behavior). The populated-map + concurrency tests
    // use `AXRef::from_create` against a heap-allocated CFData.

    #[tokio::test]
    async fn empty_session_has_no_generation() {
        let s = AxSession::new();
        assert_eq!(s.current_generation().await, None);
    }

    #[tokio::test]
    async fn lookup_before_any_snapshot_returns_snapshot_expired() {
        let s = AxSession::new();
        let r = s.lookup("a1g1").await;
        assert!(matches!(r, Err(LookupError::SnapshotExpired { .. })));
    }

    #[tokio::test]
    async fn lookup_malformed_uid_returns_snapshot_expired_with_format_message() {
        let s = AxSession::new();
        for bad in ["a42", "a42gX", "a42g", "", "A42G3"] {
            let r = s.lookup(bad).await;
            match r {
                Err(LookupError::SnapshotExpired { reason }) => {
                    assert!(
                        reason.contains("a<N>g<gen>"),
                        "reason should name expected format, got {reason:?}"
                    );
                    assert!(
                        reason.contains(bad),
                        "reason should include the received input, got {reason:?}"
                    );
                }
                other => panic!("expected SnapshotExpired for {bad:?}, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn create_snapshot_increments_generation_starting_at_one() {
        let s = AxSession::new();
        let g1 = s.create_snapshot(HashMap::new()).await;
        assert_eq!(g1, 1);
        assert_eq!(s.current_generation().await, Some(1));

        let g2 = s.create_snapshot(HashMap::new()).await;
        assert_eq!(g2, 2);
        assert_eq!(s.current_generation().await, Some(2));
    }

    #[tokio::test]
    async fn lookup_returns_uid_not_found_when_gen_matches_but_index_missing() {
        let s = AxSession::new();
        let g = s.create_snapshot(HashMap::new()).await;
        assert_eq!(g, 1);

        let r = s.lookup("a99g1").await;
        assert!(matches!(r, Err(LookupError::UidNotFound)), "got {:?}", r);
    }

    #[tokio::test]
    async fn lookup_stale_gen_returns_snapshot_expired_not_uid_not_found() {
        let s = AxSession::new();
        s.create_snapshot(HashMap::new()).await; // gen 1
        s.create_snapshot(HashMap::new()).await; // gen 2

        let r = s.lookup("a1g1").await;
        match r {
            Err(LookupError::SnapshotExpired { .. }) => (),
            other => panic!("expected SnapshotExpired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn lookup_returns_ok_for_fresh_uid_in_populated_refs() {
        use core_foundation::base::{CFRetain, CFTypeRef, TCFType};
        use core_foundation::data::CFData;

        // Build a synthetic ref using a CFData (its retain/release are CF-level
        // and work with AXRef::from_create). CFData is heap-allocated; avoids
        // the tagged-pointer pitfall of short CFStrings.
        let d = CFData::from_buffer(&[1u8, 2, 3, 4, 5, 6, 7, 8]);
        let raw: CFTypeRef = d.as_concrete_TypeRef() as CFTypeRef;
        unsafe {
            CFRetain(raw);
        }
        let aref = unsafe { AXRef::from_create(raw as *mut _) };

        let mut refs = HashMap::new();
        refs.insert(42u32, aref);

        let session = AxSession::new();
        let gen = session.create_snapshot(refs).await;

        let uid = format!("a42g{}", gen);
        let looked_up = session.lookup(&uid).await;
        assert!(
            looked_up.is_ok(),
            "populated uid should resolve (got {:?})",
            looked_up
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_lookups_are_safe() {
        use core_foundation::base::{CFRetain, CFTypeRef, TCFType};
        use core_foundation::data::CFData;
        use std::sync::Arc;

        let d = CFData::from_buffer(&[9u8, 8, 7, 6, 5, 4, 3, 2, 1]);
        let raw: CFTypeRef = d.as_concrete_TypeRef() as CFTypeRef;
        unsafe {
            CFRetain(raw);
        }
        let aref = unsafe { AXRef::from_create(raw as *mut _) };

        let mut refs = HashMap::new();
        refs.insert(1u32, aref);

        let session = Arc::new(AxSession::new());
        let gen = session.create_snapshot(refs).await;
        let uid = format!("a1g{}", gen);

        let mut handles = Vec::new();
        for _ in 0..64 {
            let s = session.clone();
            let u = uid.clone();
            handles.push(tokio::spawn(async move { s.lookup(&u).await }));
        }
        for h in handles {
            let r = h.await.expect("task should not panic");
            assert!(r.is_ok(), "concurrent lookup should succeed: {:?}", r);
        }
    }
}
