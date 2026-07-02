//! SQLite-backed [`FlaggedStore`] (slice D5-persist).
//!
//! Opens its own connection to the same `users.db` the user's chosen
//! `config.user_storage` names, owning a single `flagged_files` table.
//! Separate from [`crate::adapters::sqlite_user_repository`] so the user
//! adapter stays focused; two connections to one WAL file are safe at
//! BBS write concurrency. The persisted row is `(slot, conference,
//! name)` — exactly the domain [`FlaggedKey`] identity.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
use crate::domain::files::flagged_store::{FlaggedStore, FlaggedStoreError};

/// Errors returned when opening a [`SqliteFlaggedStore`].
#[derive(Debug, thiserror::Error)]
pub enum SqliteFlaggedStoreError {
    /// `rusqlite` could not open the database file.
    #[error("couldn't open flagged-file database {}: {error}", path.display())]
    Open {
        /// The path that was attempted.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: rusqlite::Error,
    },
    /// Pragma or schema setup failed.
    #[error("couldn't initialise flagged-file schema: {0}")]
    Schema(#[source] rusqlite::Error),
}

impl From<rusqlite::Error> for FlaggedStoreError {
    fn from(error: rusqlite::Error) -> Self {
        FlaggedStoreError::Backend(error.to_string())
    }
}

/// `rusqlite`-backed [`FlaggedStore`]; owns one connection behind a
/// [`Mutex`], WAL + `busy_timeout` like the user repository.
pub struct SqliteFlaggedStore {
    conn: Mutex<Connection>,
}

impl SqliteFlaggedStore {
    /// Opens (creating if needed) the database at `path` and ensures the
    /// `flagged_files` table exists.
    ///
    /// # Errors
    /// Returns [`SqliteFlaggedStoreError`] when the connection or schema
    /// setup fails.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, SqliteFlaggedStoreError> {
        let path = path.as_ref();
        let conn = Connection::open(path).map_err(|error| SqliteFlaggedStoreError::Open {
            path: path.to_path_buf(),
            error,
        })?;
        Self::init(&conn).map_err(SqliteFlaggedStoreError::Schema)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Opens an in-memory database (the adapter's own tests).
    ///
    /// # Errors
    /// Returns [`SqliteFlaggedStoreError::Schema`] if setup fails.
    pub fn in_memory() -> Result<Self, SqliteFlaggedStoreError> {
        let conn = Connection::open_in_memory().map_err(SqliteFlaggedStoreError::Schema)?;
        Self::init(&conn).map_err(SqliteFlaggedStoreError::Schema)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS flagged_files (
                 slot_number INTEGER NOT NULL,
                 conference  INTEGER NOT NULL,
                 name        TEXT    NOT NULL,
                 PRIMARY KEY (slot_number, conference, name)
             );",
        )?;
        Ok(())
    }
}

impl FlaggedStore for SqliteFlaggedStore {
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError> {
        let conn = self.conn.lock().expect("flagged store mutex");
        let mut stmt =
            conn.prepare("SELECT conference, name FROM flagged_files WHERE slot_number = ?1")?;
        let rows = stmt.query_map(params![slot], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut flags = FlaggedFiles::default();
        for row in rows {
            let (conference, name) = row?;
            flags.flag(FlaggedKey::new(conference, &name));
        }
        Ok(flags)
    }

    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError> {
        let mut conn = self.conn.lock().expect("flagged store mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM flagged_files WHERE slot_number = ?1",
            params![slot],
        )?;
        for (conference, name) in flags.entries() {
            tx.execute(
                "INSERT INTO flagged_files (slot_number, conference, name) VALUES (?1, ?2, ?3)",
                params![slot, conference, name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::sqlite_flagged_store::SqliteFlaggedStore;
    use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
    use crate::domain::files::flagged_store::FlaggedStore;

    fn two_flag_set() -> FlaggedFiles {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, "termv48.lha"));
        flags.flag(FlaggedKey::new(2, "mydemo.dms"));
        flags
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(7, &two_flag_set()).expect("save");
        let loaded = store.load(7).expect("load");
        assert!(loaded.contains(&FlaggedKey::new(2, "MYDEMO.DMS")));
        assert!(loaded.contains(&FlaggedKey::new(2, "TERMV48.LHA")));
        assert!(!loaded.contains(&FlaggedKey::new(3, "MYDEMO.DMS")));
    }

    #[test]
    fn unknown_slot_loads_empty() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        assert!(store.load(99).expect("load").is_empty());
    }

    #[test]
    fn save_survives_the_same_file_flagged_from_listing_and_prompt() {
        // Regression (July 2026 review, item 14): the same file flagged
        // from an F listing (real area) and via the A prompt / a restore
        // (area 0) used to occupy two set entries, so `entries()` emitted
        // two identical (conference, name) rows and the second INSERT
        // violated the primary key — rolling back the WHOLE save and
        // silently losing the session's flags on logoff.
        let store = SqliteFlaggedStore::in_memory().expect("open");
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, "termv48.lha"));
        flags.flag(FlaggedKey::new(2, "termv48.lha"));
        store.save(7, &flags).expect("save must not roll back");
        let loaded = store.load(7).expect("load");
        assert!(loaded.contains(&FlaggedKey::new(2, "TERMV48.LHA")));
    }

    #[test]
    fn re_saving_replaces_the_slot_and_empty_save_clears_it() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(1, &two_flag_set()).expect("save");
        // A smaller set replaces, not merges.
        let mut one = FlaggedFiles::default();
        one.flag(FlaggedKey::new(2, "mydemo.dms"));
        store.save(1, &one).expect("resave");
        let loaded = store.load(1).expect("load");
        assert!(loaded.contains(&FlaggedKey::new(2, "MYDEMO.DMS")));
        assert!(
            !loaded.contains(&FlaggedKey::new(2, "TERMV48.LHA")),
            "replaced, not merged"
        );
        // Empty save clears.
        store.save(1, &FlaggedFiles::default()).expect("clear");
        assert!(store.load(1).expect("load").is_empty());
    }

    #[test]
    fn save_is_keyed_per_slot() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(1, &two_flag_set()).expect("save");
        assert!(store.load(2).expect("load").is_empty());
        // A save to one slot must not disturb another slot's rows — the
        // `DELETE ... WHERE slot_number = ?1` is per-slot. Pins the WHERE
        // clause directly (a future `save` refactor that widened the
        // delete would survive the assertion above but fail here).
        store.save(2, &two_flag_set()).expect("save slot 2");
        assert!(
            !store.load(1).expect("load").is_empty(),
            "slot 1 survives a slot-2 save"
        );
    }
}
