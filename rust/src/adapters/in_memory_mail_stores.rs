//! In-memory [`MailStores`] registry adapter (Phase 6, Slice 39 / 41a).
//!
//! Wraps a [`HashMap`] from [`MessageBaseRef`] to [`SharedMailStore`].
//! The composition root opens one store per known message base at
//! startup (Slice 41a) and registers it here. Sessions look up their
//! current visit's msgbase through this registry to dispatch `R` /
//! `M` / `N` commands.
//!
//! "In-memory" refers to the *registry*, not to the underlying
//! [`MailStore`] implementations: in production the registry holds
//! [`crate::adapters::file_mail_store::FileMailStore`] handles; in
//! tests it can hold purely in-process stores.

use std::collections::HashMap;

use crate::app::mail_stores::{MailStores, SharedMailStore};
use crate::domain::conference::MessageBaseRef;

/// In-process [`MailStores`] registry keyed by [`MessageBaseRef`].
///
/// Construction is two-step: build an empty registry, then `register`
/// one entry per available message base.
#[derive(Default)]
pub struct InMemoryMailStores {
    by_msgbase: HashMap<MessageBaseRef, SharedMailStore>,
}

impl InMemoryMailStores {
    /// Returns an empty registry. Callers register message-base
    /// handles via [`Self::register`] before the registry is read.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a handle for `msgbase`. Overwrites any previous handle
    /// registered for the same coordinate — the registry owns the
    /// "one store per base" invariant.
    pub fn register(&mut self, msgbase: MessageBaseRef, store: SharedMailStore) {
        self.by_msgbase.insert(msgbase, store);
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
    fn for_msgbase(&self, msgbase: MessageBaseRef) -> Option<SharedMailStore> {
        self.by_msgbase.get(&msgbase).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Mutex;

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

    fn shared(msgbase: MessageBaseRef) -> SharedMailStore {
        Arc::new(Mutex::new(
            Box::new(StubStore { msgbase }) as Box<dyn MailStore + Send>
        ))
    }

    #[test]
    fn new_registry_is_empty() {
        let registry = InMemoryMailStores::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.for_msgbase(MessageBaseRef::new(2, 1)).is_none());
    }

    #[test]
    fn registry_with_an_entry_is_not_empty() {
        // Pin the boundary so a mutation that hard-codes `is_empty`
        // to `true` (registry incorrectly reporting "no stores") is
        // detected.
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, shared(r));
        assert!(!registry.is_empty());
    }

    #[test]
    fn register_then_lookup_returns_the_registered_handle() {
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, shared(r));
        assert_eq!(registry.len(), 1);
        let got = registry.for_msgbase(r).expect("present");
        // Identity is preserved: cloning the Arc gives the same store.
        let store = got.try_lock().expect("uncontended");
        assert_eq!(store.msgbase(), r);
    }

    #[test]
    fn lookup_for_unregistered_coordinate_returns_none() {
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, shared(r));
        assert!(registry.for_msgbase(MessageBaseRef::new(2, 2)).is_none());
        assert!(registry.for_msgbase(MessageBaseRef::new(3, 1)).is_none());
    }

    #[test]
    fn register_overwrites_existing_entry_for_same_coordinate() {
        // The "one store per base" invariant means a re-register must
        // not produce two entries for the same coordinate.
        let mut registry = InMemoryMailStores::new();
        let r = MessageBaseRef::new(2, 1);
        registry.register(r, shared(r));
        registry.register(r, shared(r));
        assert_eq!(registry.len(), 1);
    }
}
