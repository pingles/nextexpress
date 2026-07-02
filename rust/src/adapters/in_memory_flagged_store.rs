//! In-memory [`FlaggedStore`] — the default adapter (slice D5-persist).
//!
//! Retains flag sets for the process lifetime behind a [`Mutex`], the
//! analogue of [`crate::adapters::in_memory_user_repository`]. A logoff
//! → logon round-trip in one running server restores the set; a process
//! restart clears it. Durable cross-restart storage is
//! [`crate::adapters::sqlite_flagged_store::SqliteFlaggedStore`].

use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::files::flagged::FlaggedFiles;
use crate::domain::files::flagged_store::{FlaggedStore, FlaggedStoreError};

/// In-memory [`FlaggedStore`] keyed by user slot; retains for the
/// process lifetime.
#[derive(Debug, Default)]
pub struct InMemoryFlaggedStore {
    sets: Mutex<HashMap<u32, FlaggedFiles>>,
}

impl InMemoryFlaggedStore {
    /// Constructs an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl FlaggedStore for InMemoryFlaggedStore {
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError> {
        let sets = self.sets.lock().expect("flagged store mutex");
        Ok(sets.get(&slot).cloned().unwrap_or_default())
    }

    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError> {
        // The set's key IS the persisted (conference, name) projection,
        // so the clone round-trips identically to the SQLite adapter.
        let mut sets = self.sets.lock().expect("flagged store mutex");
        sets.insert(slot, flags.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore;
    use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
    use crate::domain::files::flagged_store::FlaggedStore;

    fn one_flag_set() -> FlaggedFiles {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, "mydemo.dms"));
        flags
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = InMemoryFlaggedStore::new();
        store.save(7, &one_flag_set()).expect("save");
        let loaded = store.load(7).expect("load");
        assert!(
            loaded.contains(&FlaggedKey::new(2, "MYDEMO.DMS")),
            "the (conference, name) flag round-trips"
        );
        assert!(
            !loaded.contains(&FlaggedKey::new(3, "MYDEMO.DMS")),
            "another conference's key does not match"
        );
    }

    #[test]
    fn unknown_slot_loads_empty() {
        let store = InMemoryFlaggedStore::new();
        assert!(store.load(99).expect("load").is_empty());
    }

    #[test]
    fn save_is_keyed_per_slot() {
        let store = InMemoryFlaggedStore::new();
        store.save(1, &one_flag_set()).expect("save");
        assert!(
            store.load(2).expect("load").is_empty(),
            "slot 2 is untouched"
        );
        assert!(!store.load(1).expect("load").is_empty(), "slot 1 retained");
    }

    #[test]
    fn empty_save_clears_a_previously_saved_slot() {
        let store = InMemoryFlaggedStore::new();
        store.save(1, &one_flag_set()).expect("save");
        store.save(1, &FlaggedFiles::default()).expect("clear");
        assert!(store.load(1).expect("load").is_empty());
    }
}
