//! In-memory [`MailStores`] registry adapter (Phase 6, Slice 39 / 41a).
//!
//! Wraps a [`HashMap`] from [`MessageBaseRef`] to an
//! `Arc<tokio::sync::Mutex<Box<dyn MailStore + Send>>>`. The
//! composition root opens one store per known message base at startup
//! (Slice 41a) and registers it here. Sessions look up their current
//! visit's msgbase through this registry to dispatch `R` / `M` / `N`
//! commands.
//!
//! The mutex / arc wrapping is the registry's concern; menu use cases
//! work against the [`MailStoreGuard`] returned by [`MailStores::lock`]
//! and never see the underlying types.
//!
//! "In-memory" refers to the *registry*, not to the underlying
//! [`MailStore`] implementations: in production the registry holds
//! [`crate::adapters::file_mail_store::FileMailStore`] handles; in
//! tests it can hold purely in-process stores.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::app::mail_stores::{
    MailStoreGuard, MailStoreLockFut, MailStorePairLockFut, MailStorePairLockOutcome, MailStores,
};
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail_store::MailStore;

/// Mutex slot held in the registry. Internal: callers receive a
/// [`MailStoreGuard`] through the trait, never this raw handle.
type MailStoreSlot = Arc<Mutex<Box<dyn MailStore + Send>>>;

/// In-process [`MailStores`] registry keyed by [`MessageBaseRef`].
///
/// Construction is two-step: build an empty registry, then `register`
/// one entry per available message base.
#[derive(Default)]
pub struct InMemoryMailStores {
    by_msgbase: HashMap<MessageBaseRef, MailStoreSlot>,
}

impl InMemoryMailStores {
    /// Returns an empty registry. Callers register message-base
    /// handles via [`Self::register`] before the registry is read.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a handle for `msgbase`. The registry wraps the store in
    /// its own `Arc<tokio::sync::Mutex<…>>`; the caller hands over
    /// ownership of the box. Overwrites any previous handle registered
    /// for the same coordinate — the registry owns the "one store per
    /// base" invariant.
    pub fn register(&mut self, msgbase: MessageBaseRef, store: Box<dyn MailStore + Send>) {
        self.by_msgbase.insert(msgbase, Arc::new(Mutex::new(store)));
    }

    /// Returns the number of registered handles. Useful for sysop
    /// logging at startup ("opened N message bases").
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_msgbase.len()
    }

    /// Returns whether the registry contains no handles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_msgbase.is_empty()
    }
}

impl MailStores for InMemoryMailStores {
    fn lock(&self, msgbase: MessageBaseRef) -> MailStoreLockFut<'_> {
        // Clone the Arc so the future owns its handle. `lock_owned`
        // returns a guard with a `'static` lifetime that keeps the
        // underlying mutex alive for as long as the caller holds the
        // guard, which is what menu use cases need to move the guard
        // across `.await`.
        let slot = self.by_msgbase.get(&msgbase).cloned();
        Box::pin(async move {
            let arc = slot?;
            Some(MailStoreGuard::new(arc.lock_owned().await))
        })
    }

    fn lock_pair(
        &self,
        source: MessageBaseRef,
        target: MessageBaseRef,
    ) -> MailStorePairLockFut<'_> {
        let source_slot = self.by_msgbase.get(&source).cloned();
        let target_slot = self.by_msgbase.get(&target).cloned();
        Box::pin(async move {
            let Some(source_slot) = source_slot else {
                return MailStorePairLockOutcome::MissingSource;
            };
            let Some(target_slot) = target_slot else {
                return MailStorePairLockOutcome::MissingTarget;
            };
            // Catch same-msgbase requests *before* acquiring the
            // second lock so a self-deadlock is impossible. The
            // registry enforces "one store per coordinate", so
            // `source == target` keys imply the same `Arc<Mutex<_>>`
            // and pointer equality would say the same thing — keying
            // on `MessageBaseRef` is simpler and matches the domain
            // rule (`messaging.allium:MoveMail` rejects on equal
            // msgbase, not on pointer identity).
            if source == target {
                return MailStorePairLockOutcome::SameStore;
            }
            // Distinct coordinates resolve to distinct `Arc<Mutex<_>>`
            // values so acquiring them in any order can't deadlock
            // with another move that picked the opposite order, as
            // long as we always honour the same global ordering.
            // Sorting on `MessageBaseRef` gives a total order and is
            // cheap. (Any total order works; cargo-mutants flags
            // `< → >` and `< → <=` as surviving — both are equivalent
            // mutants: `>` is the reverse ordering, also a valid total
            // order; `<=` only differs at `a == b`, which is short-
            // circuited above.)
            let (first_is_source, first_slot, second_slot) = if source < target {
                (true, source_slot, target_slot)
            } else {
                (false, target_slot, source_slot)
            };
            let first = first_slot.lock_owned().await;
            let second = second_slot.lock_owned().await;
            let (source_guard, target_guard) = if first_is_source {
                (first, second)
            } else {
                (second, first)
            };
            MailStorePairLockOutcome::Locked {
                source: MailStoreGuard::new(source_guard),
                target: MailStoreGuard::new(target_guard),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::mail::{Mail, MailDraft};
    use crate::domain::messaging::mail_store::{MailStore, MailStoreError};

    /// Minimal in-process [`MailStore`] stub used by the registry's
    /// unit tests. Not a public API.
    struct StubStore {
        msgbase: MessageBaseRef,
    }

    impl MailStore for StubStore {
        fn highest_message(&self) -> u32 {
            0
        }
        fn msgbase(&self) -> MessageBaseRef {
            self.msgbase
        }
        fn insert(&mut self, _draft: MailDraft) -> Result<Mail, MailStoreError> {
            unimplemented!("not used in registry tests")
        }
        fn load(&self, _number: u32) -> Result<Option<Mail>, MailStoreError> {
            Ok(None)
        }
        fn save(&mut self, _mail: &Mail) -> Result<(), MailStoreError> {
            Ok(())
        }
    }

    fn stub(msgbase: MessageBaseRef) -> Box<dyn MailStore + Send> {
        Box::new(StubStore { msgbase })
    }

    #[test]
    fn new_registry_is_empty() {
        let registry = InMemoryMailStores::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[tokio::test]
    async fn lock_returns_none_for_unregistered_coordinate() {
        let registry = InMemoryMailStores::new();
        assert!(registry.lock(MessageBaseRef::new(2, 1)).await.is_none());
    }

    #[test]
    fn registry_with_an_entry_is_not_empty() {
        // Pin the boundary so a mutation that hard-codes `is_empty`
        // to `true` (registry incorrectly reporting "no stores") is
        // detected.
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, stub(r));
        assert!(!registry.is_empty());
    }

    #[tokio::test]
    async fn register_then_lock_returns_the_registered_handle() {
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, stub(r));
        assert_eq!(registry.len(), 1);
        let guard = registry.lock(r).await.expect("present");
        assert_eq!(guard.msgbase(), r);
    }

    #[tokio::test]
    async fn lock_for_unregistered_coordinate_returns_none() {
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, stub(r));
        assert!(registry.lock(MessageBaseRef::new(2, 2)).await.is_none());
        assert!(registry.lock(MessageBaseRef::new(3, 1)).await.is_none());
    }

    #[test]
    fn register_overwrites_existing_entry_for_same_coordinate() {
        // The "one store per base" invariant means a re-register must
        // not produce two entries for the same coordinate.
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, stub(r));
        registry.register(r, stub(r));
        assert_eq!(registry.len(), 1);
    }

    #[tokio::test]
    async fn lock_pair_returns_missing_source_when_source_is_unknown() {
        let mut registry = InMemoryMailStores::new();
        let target = MessageBaseRef::new(2, 1);
        registry.register(target, stub(target));
        let outcome = registry.lock_pair(MessageBaseRef::new(9, 9), target).await;
        assert!(matches!(outcome, MailStorePairLockOutcome::MissingSource));
    }

    #[tokio::test]
    async fn lock_pair_returns_missing_target_when_target_is_unknown() {
        let mut registry = InMemoryMailStores::new();
        let source = MessageBaseRef::new(2, 1);
        registry.register(source, stub(source));
        let outcome = registry.lock_pair(source, MessageBaseRef::new(9, 9)).await;
        assert!(matches!(outcome, MailStorePairLockOutcome::MissingTarget));
    }

    #[tokio::test]
    async fn lock_pair_returns_same_store_when_coordinates_match() {
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, stub(r));
        let outcome = registry.lock_pair(r, r).await;
        assert!(matches!(outcome, MailStorePairLockOutcome::SameStore));
    }

    #[tokio::test]
    async fn lock_pair_locks_distinct_stores_in_a_deadlock_safe_order() {
        // Requesting the same pair from both orientations must both
        // succeed under concurrent contention: the registry sorts the
        // underlying mutex acquisitions on `MessageBaseRef` so two
        // concurrent moves can't deadlock by picking opposite orders.
        let mut registry = InMemoryMailStores::new();
        let a = MessageBaseRef::new(1, 1);
        let b = MessageBaseRef::new(2, 1);
        registry.register(a, stub(a));
        registry.register(b, stub(b));

        let outcome_ab = registry.lock_pair(a, b).await;
        match outcome_ab {
            MailStorePairLockOutcome::Locked { source, target } => {
                assert_eq!(source.msgbase(), a);
                assert_eq!(target.msgbase(), b);
            }
            _ => panic!("expected Locked"),
        }

        let outcome_ba = registry.lock_pair(b, a).await;
        match outcome_ba {
            MailStorePairLockOutcome::Locked { source, target } => {
                assert_eq!(source.msgbase(), b);
                assert_eq!(target.msgbase(), a);
            }
            _ => panic!("expected Locked"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_lock_pair_in_opposite_orientations_does_not_deadlock() {
        // The registry's lock ordering invariant prevents the classic
        // "thread A locks X then Y, thread B locks Y then X" deadlock
        // that a naive lock-source-then-lock-target path would hit
        // when two concurrent moves pick opposite orientations.
        //
        // Spawn many task pairs that hold each `lock_pair` guard for a
        // short busy interval and request the pair in opposite orders;
        // if the global ordering breaks, the deadlock would hang the
        // whole test. A short `tokio::time::timeout` wrapper turns
        // that hang into an explicit failure instead of a stalled CI
        // run.
        use std::sync::Arc as StdArc;
        use std::time::Duration;
        use tokio::time::timeout;

        let mut registry = InMemoryMailStores::new();
        let a = MessageBaseRef::new(1, 1);
        let b = MessageBaseRef::new(2, 1);
        registry.register(a, stub(a));
        registry.register(b, stub(b));
        let registry = StdArc::new(registry);

        let mut handles = Vec::with_capacity(40);
        for i in 0..20 {
            let reg = registry.clone();
            handles.push(tokio::spawn(async move {
                // Alternate the orientation so half the tasks ask
                // for (a, b) and half ask for (b, a).
                let (source, target) = if i % 2 == 0 { (a, b) } else { (b, a) };
                let outcome = reg.lock_pair(source, target).await;
                match outcome {
                    MailStorePairLockOutcome::Locked {
                        source: src,
                        target: tgt,
                    } => {
                        assert_eq!(src.msgbase(), source);
                        assert_eq!(tgt.msgbase(), target);
                        // Hold both guards across an `.await` so the
                        // other tasks pile up on the registry's
                        // mutexes — exactly the contention shape that
                        // would deadlock without a consistent global
                        // lock order.
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        drop(tgt);
                        drop(src);
                    }
                    other => panic!("expected Locked, got {:?}", std::mem::discriminant(&other)),
                }
            }));
        }
        let join_all = async move {
            for h in handles {
                h.await.expect("task completed");
            }
        };
        timeout(Duration::from_secs(5), join_all)
            .await
            .expect("lock_pair contention completed without deadlock");
    }
}
