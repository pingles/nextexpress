//! In-memory [`FlaggedStore`] — the default adapter (slice D5-persist).
//!
//! Retains flag sets for the process lifetime behind a [`Mutex`], the
//! analogue of [`crate::adapters::in_memory_user_repository`]. A logoff
//! → logon round-trip in one running server restores the set; a process
//! restart clears it. Durable cross-restart storage is
//! [`crate::adapters::sqlite_flagged_store::SqliteFlaggedStore`].

use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
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
        // Persist the (conference, name) projection: rebuild at area 0 so
        // the in-memory round-trip matches the SQLite one exactly.
        let mut normalised = FlaggedFiles::default();
        for (conference, name) in flags.entries() {
            normalised.flag(FlaggedKey::new(conference, 0, name));
        }
        let mut sets = self.sets.lock().expect("flagged store mutex");
        sets.insert(slot, normalised);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore;
    use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
    use crate::domain::files::flagged_store::FlaggedStore;

    fn set_with(area: u32) -> FlaggedFiles {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, area, "mydemo.dms"));
        flags
    }

    #[test]
    fn save_then_load_round_trips_and_normalises_area_to_zero() {
        let store = InMemoryFlaggedStore::new();
        store.save(7, &set_with(3)).expect("save");
        let loaded = store.load(7).expect("load");
        assert!(
            loaded.contains(&FlaggedKey::new(2, 0, "MYDEMO.DMS")),
            "restored flag is keyed at area 0"
        );
        assert!(
            !loaded.contains(&FlaggedKey::new(2, 3, "MYDEMO.DMS")),
            "the original area is not preserved"
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
        store.save(1, &set_with(1)).expect("save");
        assert!(
            store.load(2).expect("load").is_empty(),
            "slot 2 is untouched"
        );
        assert!(!store.load(1).expect("load").is_empty(), "slot 1 retained");
    }

    #[test]
    fn empty_save_clears_a_previously_saved_slot() {
        let store = InMemoryFlaggedStore::new();
        store.save(1, &set_with(1)).expect("save");
        store.save(1, &FlaggedFiles::default()).expect("clear");
        assert!(store.load(1).expect("load").is_empty());
    }
}
