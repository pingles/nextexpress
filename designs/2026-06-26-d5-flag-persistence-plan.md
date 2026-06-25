# D5-persist (flagged-file persistence + logon banner) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist the session flagged-file set across logons (durable under SQLite, process-lifetime in memory) and emit the legacy `** Flagged File(s) Exist **` banner on logon when a non-empty set is restored.

**Architecture:** A synchronous `FlaggedStore` port (`domain/files/`) with two adapters — `InMemoryFlaggedStore` (default) and `SqliteFlaggedStore` (own connection to `users.db`) — selected by the existing `config.user_storage` switch. The session lifecycle gains a logon load + banner hook and a logoff save hook. Keying is `(conference, name)`; `area` is normalised to `0` on the round-trip in both adapters.

**Tech Stack:** Rust, `rusqlite` (already a dependency), `thiserror`, `tokio` (e2e), `cargo nextest`, `cargo mutants`.

Design: `designs/2026-06-25-d5-flag-persistence-design.md`.

## Global Constraints

- TDD: every change starts with a failing test; run it red, implement minimally, run green, commit. (`AGENTS.md` §"Key Workflow".)
- Wire is always valid UTF-8 (`AGENTS.md` §"Wire encoding"). The banner const is ASCII + BEL.
- Hexagonal boundary: port in `domain/`, adapters in `adapters/`, wired only in `bootstrap.rs`. `tests/architecture.rs` forbids `crate::adapters` imports outside the composition root — do not import an adapter from `domain/` or `app/`.
- Synchronous port methods (match `UserRepository`): `Result<_, FlaggedStoreError>`, no `async`.
- Persisted identity is `(conference, name)`; `area` is never stored; load returns `area = 0`. Both adapters behave identically.
- `fmt`/`clippy` run via Claude Code hooks; keep `cargo clippy -- -D warnings` clean. Public items need doc comments (`AGENTS.md` §"Style 4").
- Commit messages end with: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Run the suite with `cargo nextest run` from `rust/`. Single tests: `cargo nextest run -p nextexpress <substring>`.

---

### Task 1: `FlaggedFiles` persistence accessors + public visibility

Add the `(conference, name)` iterator the store consumes and widen the two flag types so they can appear in the public port signature.

**Files:**
- Modify: `rust/src/domain/files/flagged.rs`
- Test: `rust/src/domain/files/flagged.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub struct FlaggedFiles`, `pub struct FlaggedKey`; `FlaggedKey::conference(&self) -> u32`; `FlaggedFiles::entries(&self) -> impl Iterator<Item = (u32, &str)>` yielding `(conference, name)` in catalogue-key order. Existing `flag`, `contains`, `is_empty`, `clear`, `names` unchanged.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `rust/src/domain/files/flagged.rs`:

```rust
#[test]
fn entries_yield_conference_and_name_pairs_in_key_order() {
    let mut flags = FlaggedFiles::default();
    flags.flag(FlaggedKey::new(2, 1, "termv48.lha"));
    flags.flag(FlaggedKey::new(1, 3, "mydemo.dms"));
    let entries: Vec<(u32, &str)> = flags.entries().collect();
    assert_eq!(
        entries,
        vec![(1, "MYDEMO.DMS"), (2, "TERMV48.LHA")],
        "entries are (conference, upper-name) in BTreeSet key order"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p nextexpress entries_yield_conference_and_name_pairs_in_key_order`
Expected: FAIL — `no method named entries found for struct FlaggedFiles` (compile error).

- [ ] **Step 3: Widen visibility and add the accessors**

In `rust/src/domain/files/flagged.rs`, change the two type declarations from `pub(crate) struct` to `pub struct`:

```rust
pub struct FlaggedKey {
    conference: u32,
    area: u32,
    name: String,
}
```

```rust
pub struct FlaggedFiles {
    set: BTreeSet<FlaggedKey>,
}
```

Add to `impl FlaggedKey` (next to `name`):

```rust
    /// The conference this flag belongs to — the legacy `confNum`
    /// half of the persisted `(confNum, fileName)` key (slice D5-persist).
    pub(crate) fn conference(&self) -> u32 {
        self.conference
    }
```

Add to `impl FlaggedFiles` (next to `names`):

```rust
    /// The flags as `(conference, name)` pairs in catalogue-key order —
    /// the projection `FlaggedStore::save` persists (slice D5-persist).
    /// `area` is deliberately omitted: it is a session-local concern and
    /// the legacy `flagged` file stores only conf + name
    /// (`amiexpress/express.e:2822`).
    pub(crate) fn entries(&self) -> impl Iterator<Item = (u32, &str)> {
        self.set.iter().map(|k| (k.conference(), k.name()))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p nextexpress entries_yield_conference_and_name_pairs_in_key_order`
Expected: PASS.

- [ ] **Step 5: Build to confirm no visibility regressions**

Run: `cargo build`
Expected: clean (the `pub` widening is additive; `dead_code`/`unreachable_pub` lints are not enabled here).

- [ ] **Step 6: Commit**

```bash
git add rust/src/domain/files/flagged.rs
git commit -m "$(printf 'D5-persist: FlaggedFiles::entries + public flag types\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 2: `FlaggedStore` port + `InMemoryFlaggedStore` adapter

Define the port (its first concrete impl proves the trait shape) and the default in-memory adapter that retains for the process lifetime and normalises `area` to `0`.

**Files:**
- Create: `rust/src/domain/files/flagged_store.rs`
- Modify: `rust/src/domain/files/mod.rs` (register module)
- Create: `rust/src/adapters/in_memory_flagged_store.rs`
- Modify: `rust/src/adapters/mod.rs` (register module)
- Test: `rust/src/adapters/in_memory_flagged_store.rs` (inline tests)

**Interfaces:**
- Consumes: `FlaggedFiles`, `FlaggedKey` (Task 1).
- Produces:
  - `pub trait FlaggedStore { fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError>; fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError>; }`
  - `pub enum FlaggedStoreError { Backend(String) }` (impls `std::error::Error` via `thiserror`).
  - `pub struct InMemoryFlaggedStore` with `pub fn new() -> Self`.

- [ ] **Step 1: Create the port**

Create `rust/src/domain/files/flagged_store.rs`:

```rust
//! Port over a user's durable flagged-file set (slice D5-persist).
//!
//! The session's [`FlaggedFiles`] set lives and dies with the session
//! in memory; this port persists it per user slot so the next logon can
//! restore it (the legacy `saveFlagged`/`loadFlagged`,
//! `amiexpress/express.e:2806`/`:2757`). The persisted identity is
//! `(conference, name)` — `area` is normalised to `0` on load, uniformly
//! across adapters (see the design's Decision ①).

use crate::domain::files::flagged::FlaggedFiles;

/// Error from a [`FlaggedStore`] backend.
#[derive(Debug, thiserror::Error)]
pub enum FlaggedStoreError {
    /// The backing store could not be read or written.
    #[error("flagged-file store backend error: {0}")]
    Backend(String),
}

/// Durable home of a user's flagged-file set, keyed by user slot.
pub trait FlaggedStore {
    /// Loads the flag set saved for `slot`, or an empty set when none is
    /// stored. Restored keys carry `area = 0`.
    ///
    /// # Errors
    /// Returns [`FlaggedStoreError`] when the backing store cannot be read.
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError>;

    /// Replaces the saved flag set for `slot` with `flags` (an empty set
    /// clears it), persisting the `(conference, name)` projection.
    ///
    /// # Errors
    /// Returns [`FlaggedStoreError`] when the backing store cannot be written.
    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError>;
}
```

- [ ] **Step 2: Register the port module**

In `rust/src/domain/files/mod.rs`, add under the existing `pub mod flagged;`:

```rust
pub mod flagged_store;
```

- [ ] **Step 3: Write the failing in-memory adapter test**

Create `rust/src/adapters/in_memory_flagged_store.rs` with only the tests first:

```rust
//! In-memory [`FlaggedStore`] — the default adapter (slice D5-persist).
//!
//! Retains flag sets for the process lifetime behind a [`Mutex`], the
//! analogue of [`crate::adapters::in_memory_user_repository`]. A logoff
//! → logon round-trip in one running server restores the set; a process
//! restart clears it. Durable cross-restart storage is
//! [`crate::adapters::sqlite_flagged_store::SqliteFlaggedStore`].

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
        assert!(store.load(2).expect("load").is_empty(), "slot 2 is untouched");
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
```

- [ ] **Step 4: Register the adapter module and run the test (expect fail)**

In `rust/src/adapters/mod.rs`, add (keep alphabetical with the other `in_memory_*` entries):

```rust
pub mod in_memory_flagged_store;
```

Run: `cargo nextest run -p nextexpress in_memory_flagged_store`
Expected: FAIL — `cannot find struct InMemoryFlaggedStore` (not yet defined).

- [ ] **Step 5: Implement the adapter**

Prepend to `rust/src/adapters/in_memory_flagged_store.rs` (above the `#[cfg(test)]` module), keeping the module doc comment at the very top:

```rust
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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo nextest run -p nextexpress in_memory_flagged_store`
Expected: PASS (4 tests).

- [ ] **Step 7: Commit**

```bash
git add rust/src/domain/files/flagged_store.rs rust/src/domain/files/mod.rs rust/src/adapters/in_memory_flagged_store.rs rust/src/adapters/mod.rs
git commit -m "$(printf 'D5-persist: FlaggedStore port + in-memory adapter\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 3: `SqliteFlaggedStore` adapter

Durable backend: its own `Mutex<Connection>` to the `users.db` path, owning a `flagged_files` table.

**Files:**
- Create: `rust/src/adapters/sqlite_flagged_store.rs`
- Modify: `rust/src/adapters/mod.rs` (register module)
- Test: `rust/src/adapters/sqlite_flagged_store.rs` (inline tests via `Connection::open_in_memory`)

**Interfaces:**
- Consumes: `FlaggedStore`, `FlaggedStoreError`, `FlaggedFiles`, `FlaggedKey`.
- Produces: `pub struct SqliteFlaggedStore` with `pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, SqliteFlaggedStoreError>` and `pub fn in_memory() -> Result<Self, SqliteFlaggedStoreError>`.

- [ ] **Step 1: Write the failing tests**

Create `rust/src/adapters/sqlite_flagged_store.rs` with the module doc + tests first:

```rust
//! SQLite-backed [`FlaggedStore`] (slice D5-persist).
//!
//! Opens its own connection to the same `users.db` the user's chosen
//! `config.user_storage` names, owning a single `flagged_files` table.
//! Separate from [`crate::adapters::sqlite_user_repository`] so the user
//! adapter stays focused; two connections to one WAL file are safe at
//! BBS write concurrency. The persisted row is `(slot, conference,
//! name)` — `area` is not stored and load returns `area = 0`.

#[cfg(test)]
mod tests {
    use crate::adapters::sqlite_flagged_store::SqliteFlaggedStore;
    use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
    use crate::domain::files::flagged_store::FlaggedStore;

    fn set_with(area: u32) -> FlaggedFiles {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, area, "termv48.lha"));
        flags.flag(FlaggedKey::new(2, area, "mydemo.dms"));
        flags
    }

    #[test]
    fn save_then_load_round_trips_and_normalises_area_to_zero() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(7, &set_with(3)).expect("save");
        let loaded = store.load(7).expect("load");
        assert!(loaded.contains(&FlaggedKey::new(2, 0, "MYDEMO.DMS")));
        assert!(loaded.contains(&FlaggedKey::new(2, 0, "TERMV48.LHA")));
        assert!(!loaded.contains(&FlaggedKey::new(2, 3, "MYDEMO.DMS")));
    }

    #[test]
    fn unknown_slot_loads_empty() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        assert!(store.load(99).expect("load").is_empty());
    }

    #[test]
    fn re_saving_replaces_the_slot_and_empty_save_clears_it() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(1, &set_with(1)).expect("save");
        // A smaller set replaces, not merges.
        let mut one = FlaggedFiles::default();
        one.flag(FlaggedKey::new(2, 0, "mydemo.dms"));
        store.save(1, &one).expect("resave");
        let loaded = store.load(1).expect("load");
        assert!(loaded.contains(&FlaggedKey::new(2, 0, "MYDEMO.DMS")));
        assert!(!loaded.contains(&FlaggedKey::new(2, 0, "TERMV48.LHA")), "replaced, not merged");
        // Empty save clears.
        store.save(1, &FlaggedFiles::default()).expect("clear");
        assert!(store.load(1).expect("load").is_empty());
    }

    #[test]
    fn save_is_keyed_per_slot() {
        let store = SqliteFlaggedStore::in_memory().expect("open");
        store.save(1, &set_with(1)).expect("save");
        assert!(store.load(2).expect("load").is_empty());
    }
}
```

- [ ] **Step 2: Register and run (expect fail)**

In `rust/src/adapters/mod.rs`, add (keep next to `sqlite_user_repository`):

```rust
pub mod sqlite_flagged_store;
```

Run: `cargo nextest run -p nextexpress sqlite_flagged_store`
Expected: FAIL — `cannot find struct SqliteFlaggedStore`.

- [ ] **Step 3: Implement the adapter**

Prepend to `rust/src/adapters/sqlite_flagged_store.rs` (above the test module, below the doc comment):

```rust
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
use crate::domain::files::flagged_store::{FlaggedStore, FlaggedStoreError};

/// Errors from opening a [`SqliteFlaggedStore`].
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
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Opens an in-memory database (the adapter's own tests).
    ///
    /// # Errors
    /// Returns [`SqliteFlaggedStoreError::Schema`] if setup fails.
    pub fn in_memory() -> Result<Self, SqliteFlaggedStoreError> {
        let conn = Connection::open_in_memory().map_err(SqliteFlaggedStoreError::Schema)?;
        Self::init(&conn).map_err(SqliteFlaggedStoreError::Schema)?;
        Ok(Self { conn: Mutex::new(conn) })
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
        let mut stmt = conn
            .prepare("SELECT conference, name FROM flagged_files WHERE slot_number = ?1")
            .map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        let rows = stmt
            .query_map(params![slot], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        let mut flags = FlaggedFiles::default();
        for row in rows {
            let (conference, name) = row.map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
            flags.flag(FlaggedKey::new(conference, 0, &name));
        }
        Ok(flags)
    }

    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError> {
        let mut conn = self.conn.lock().expect("flagged store mutex");
        let tx = conn
            .transaction()
            .map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        tx.execute("DELETE FROM flagged_files WHERE slot_number = ?1", params![slot])
            .map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        for (conference, name) in flags.entries() {
            tx.execute(
                "INSERT INTO flagged_files (slot_number, conference, name) VALUES (?1, ?2, ?3)",
                params![slot, conference, name],
            )
            .map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        }
        tx.commit().map_err(|e| FlaggedStoreError::Backend(e.to_string()))?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p nextexpress sqlite_flagged_store`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add rust/src/adapters/sqlite_flagged_store.rs rust/src/adapters/mod.rs
git commit -m "$(printf 'D5-persist: SQLite flagged-file adapter\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 4: Wire `flagged_store` into `AppServices` + `bootstrap`

Add the port to the service bundle and select the adapter from `config.user_storage` (the same switch as the user repo). This task makes the project compile with the new field everywhere.

**Files:**
- Modify: `rust/src/app/services.rs` (alias + field)
- Modify: `rust/src/app/runtime.rs:86` (production `AppServices` literal; `RuntimePorts` if it carries the ports)
- Modify: `rust/src/bootstrap.rs` (`open_flagged_store` helper; thread into `Runtime::new`/`RuntimePorts`; add to `RuntimeAdapters` + `build_runtime`)
- Modify: every test `AppServices { .. }` literal — `session_driver.rs` (×7: lines ~626, 686, 811, 905, 982, 1083, 1162, 1329), `menu_flow/tests.rs:201`, `menu_flow/reply_forward.rs:423`, `menu_flow/sysop_admin.rs:540`, `menu_flow/read_subprompt.rs:442`, `menu_flow/pager.rs:152`, `menu_flow/file_list/tests.rs:165`, `menu_flow/join/tests.rs:136`, and any others the compiler flags.
- Test: `rust/src/bootstrap.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `InMemoryFlaggedStore`, `SqliteFlaggedStore`, `FlaggedStore` (Tasks 2–3).
- Produces: `pub type SharedFlaggedStore = Arc<dyn FlaggedStore + Send + Sync + 'static>;` and `AppServices.flagged_store: SharedFlaggedStore`.

- [ ] **Step 1: Write the failing bootstrap test**

Add to the `tests` module in `rust/src/bootstrap.rs`:

```rust
    #[test]
    fn open_flagged_store_is_in_memory_without_user_storage() {
        let config = Config::default(); // user_storage = None
        let store = open_flagged_store(&config).expect("store");
        // A fresh in-memory store loads empty for any slot and round-trips.
        use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
        assert!(store.load(1).expect("load").is_empty());
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(1, 0, "ansipack.lha"));
        store.save(1, &flags).expect("save");
        assert!(!store.load(1).expect("load").is_empty());
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p nextexpress open_flagged_store_is_in_memory_without_user_storage`
Expected: FAIL — `cannot find function open_flagged_store`.

- [ ] **Step 3: Add the alias and field to `AppServices`**

In `rust/src/app/services.rs`, add the import and alias near the other `Shared*` aliases:

```rust
use crate::domain::files::flagged_store::FlaggedStore;
```

```rust
/// Shared flagged-file store handle (slice D5-persist).
pub type SharedFlaggedStore = Arc<dyn FlaggedStore + Send + Sync + 'static>;
```

Add the field to the `AppServices` struct (after `file_repo`):

```rust
    /// Flagged-file store (slice D5-persist): per-slot persistence of the
    /// session flag set.
    pub flagged_store: SharedFlaggedStore,
```

- [ ] **Step 4: Add the `open_flagged_store` helper to bootstrap**

In `rust/src/bootstrap.rs`, add imports (near the other adapter imports):

```rust
use crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore;
use crate::adapters::sqlite_flagged_store::SqliteFlaggedStore;
use crate::app::services::SharedFlaggedStore;
```

Add the helper (next to `open_user_repository`):

```rust
/// Constructs the configured [`crate::domain::files::flagged_store::FlaggedStore`]
/// adapter. `None` for `config.user_storage` selects the in-memory
/// adapter (process-lifetime, cleared on restart); `Some(path)` opens a
/// [`SqliteFlaggedStore`] at the same database the user repository uses.
fn open_flagged_store(
    config: &Config,
) -> Result<SharedFlaggedStore, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(path) = config.user_storage.as_deref() {
        return Ok(Arc::new(SqliteFlaggedStore::open(path)?));
    }
    Ok(Arc::new(InMemoryFlaggedStore::new()))
}
```

- [ ] **Step 5: Thread it through `Runtime::new` / `RuntimePorts` and `build_runtime`**

In `rust/src/bootstrap.rs`:
- Add `flagged_store` to the `RuntimePorts { .. }` literal in `run` (call `open_flagged_store(&config)?`).
- Add `pub flagged_store: SharedFlaggedStore,` to the `RuntimeAdapters` struct, and forward it in `build_runtime`'s `RuntimePorts { .. }`.

In `rust/src/app/runtime.rs`:
- Add `pub flagged_store: SharedFlaggedStore,` to `RuntimePorts` (the struct `Runtime::new` consumes).
- In the `AppServices { .. }` literal at `runtime.rs:86`, add `flagged_store: ports.flagged_store,`.

(Match the exact field-passing style already used for `file_repo`.)

- [ ] **Step 6: Fix every other `AppServices` / `RuntimeAdapters` construction site**

Build, and for each `missing field flagged_store` error add the field. In **test** `AppServices { .. }` literals add:

```rust
            flagged_store: Arc::new(crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new()),
```

In any test that builds `RuntimeAdapters { .. }`, add the same `flagged_store:` line. (Use the fully-qualified path so no new `use` is needed; collapse to a local `use` if the file already imports the adapter.)

Run: `cargo build --tests`
Expected: eventually clean — iterate until no `missing field flagged_store` remains.

- [ ] **Step 7: Run the bootstrap test (expect pass) + full suite**

Run: `cargo nextest run -p nextexpress open_flagged_store_is_in_memory_without_user_storage`
Expected: PASS.

Run: `cargo nextest run`
Expected: all pass (behaviour unchanged so far — the store is wired but not yet read or written).

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(printf 'D5-persist: wire FlaggedStore into AppServices and bootstrap\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 5: Save the flag set on logoff

Call `flagged_store.save` in `handle_logoff`, right after the autosave banner and before the logoff transition consumes the session. Storage errors are logged, never fatal.

**Files:**
- Modify: `rust/src/app/menu_flow/mod.rs` (`handle_logoff`, after the `AUTOSAVING_FILE_FLAGS` emit ~`:538`)
- Test: `rust/src/app/menu_flow/tests.rs` (a spy `FlaggedStore`)

**Interfaces:**
- Consumes: `FlaggedStore` (via `self.services.flagged_store`), `MenuSession::user().slot_number()`, `MenuSession::flagged_files()`.

- [ ] **Step 1: Add a spy `FlaggedStore` to the menu-flow tests**

In `rust/src/app/menu_flow/tests.rs`, add near the top of the test module (after imports):

```rust
use std::sync::Mutex as StdMutex;

use crate::domain::files::flagged::FlaggedFiles;
use crate::domain::files::flagged_store::{FlaggedStore, FlaggedStoreError};

#[derive(Default)]
struct SpyFlaggedStore {
    /// (slot, sorted names) recorded on each save.
    saved: StdMutex<Vec<(u32, Vec<String>)>>,
    /// Pre-seeded sets returned by `load`, keyed by slot.
    seeded: StdMutex<std::collections::HashMap<u32, FlaggedFiles>>,
    /// When true, `save`/`load` return an error.
    fail: bool,
}

impl FlaggedStore for SpyFlaggedStore {
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError> {
        if self.fail {
            return Err(FlaggedStoreError::Backend("boom".into()));
        }
        Ok(self.seeded.lock().unwrap().get(&slot).cloned().unwrap_or_default())
    }
    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError> {
        if self.fail {
            return Err(FlaggedStoreError::Backend("boom".into()));
        }
        let names: Vec<String> = flags.names().map(str::to_owned).collect();
        self.saved.lock().unwrap().push((slot, names));
        Ok(())
    }
}
```

Add a helper to build `test_services()` with a chosen flagged store (refactor the existing `test_services()` to delegate, or add a sibling). Minimal approach — add:

```rust
fn services_with_flagged_store(store: Arc<dyn FlaggedStore + Send + Sync>) -> AppServices {
    let mut services = test_services();
    services.flagged_store = store;
    services
}
```

- [ ] **Step 2: Write the failing test**

Add to `rust/src/app/menu_flow/tests.rs`:

```rust
#[tokio::test]
async fn logoff_saves_the_flag_set_for_the_user_slot() {
    // Slice D5-persist: saveFlagged (express.e:2806) writes the session
    // set to the durable store on the `G Y` logoff path, after the
    // autosave banner. The sysop fixture is slot 2 (test_user()).
    let spy = Arc::new(SpyFlaggedStore::default());
    let services = services_with_flagged_store(spy.clone());
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome =
        dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    let saved = spy.saved.lock().unwrap();
    assert_eq!(saved.len(), 1, "save called exactly once on logoff");
    assert_eq!(saved[0], (2, vec!["MYDEMO.DMS".to_string()]),
        "saved (slot, names) for the logged-on user");
}
```

(Confirm the fixture's slot number: `test_user()` builds slot 2; `session_with_flagged_file()` flags `MYDEMO.DMS`. If `test_user`'s slot differs, use that number.)

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo nextest run -p nextexpress logoff_saves_the_flag_set_for_the_user_slot`
Expected: FAIL — `saved.len()` is 0 (no save call yet).

- [ ] **Step 4: Implement the save hook**

In `rust/src/app/menu_flow/mod.rs`, in `handle_logoff`, immediately after:

```rust
        self.write_and_flush(AUTOSAVING_FILE_FLAGS).await?;
```

insert:

```rust
        // D5-persist: saveFlagged writes the set to the durable store
        // (express.e:2806). Read slot + set before `user_requests_logoff`
        // consumes the session. A store error is logged, never fatal.
        let slot = session.user().slot_number();
        if let Err(error) = self.services.flagged_store.save(slot, session.flagged_files()) {
            eprintln!("saveFlagged: could not persist flags for slot {slot}: {error}");
        }
```

- [ ] **Step 5: Run the test (expect pass)**

Run: `cargo nextest run -p nextexpress logoff_saves_the_flag_set_for_the_user_slot`
Expected: PASS.

- [ ] **Step 6: Add the save-error-tolerance test**

```rust
#[tokio::test]
async fn logoff_proceeds_when_the_flag_save_fails() {
    let spy = Arc::new(SpyFlaggedStore { fail: true, ..SpyFlaggedStore::default() });
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome =
        dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;
    assert!(
        matches!(outcome, DispatchOutcome::LogoffComplete(_)),
        "a save failure must not block logoff"
    );
}
```

Run: `cargo nextest run -p nextexpress logoff_proceeds_when_the_flag_save_fails`
Expected: PASS.

- [ ] **Step 7: Run the suite + commit**

Run: `cargo nextest run` → all pass.

```bash
git add rust/src/app/menu_flow/mod.rs rust/src/app/menu_flow/tests.rs
git commit -m "$(printf 'D5-persist: save the flag set on logoff\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 6: Load the flag set on logon + emit the banner

A new `MenuFlow` method loads the saved set into the session and, when non-empty, writes `** Flagged File(s) Exist **`. Called in `session_driver` right before the menu loop.

**Files:**
- Modify: `rust/src/app/menu_flow/mod.rs` (`FLAGGED_FILES_EXIST` const + `restore_flags_and_announce`)
- Modify: `rust/src/app/session_driver.rs:210-211` (call it after `render_login_stats`, before `MenuFlow::run`)
- Test: `rust/src/app/menu_flow/tests.rs`

**Interfaces:**
- Consumes: `FlaggedStore`, `MenuSession::user().slot_number()`, `MenuSession::flagged_files_mut()`.
- Produces: `MenuFlow::restore_flags_and_announce(&mut self, session: &mut MenuSession) -> Result<(), T::Error>`; `const FLAGGED_FILES_EXIST: &[u8]`.

- [ ] **Step 1: Write the failing tests**

Add to `rust/src/app/menu_flow/tests.rs`:

```rust
#[tokio::test]
async fn logon_restores_flags_and_announces_when_non_empty() {
    // Slice D5-persist: loadFlagged (express.e:2757) restores the set on
    // logon; a non-empty restore emits the banner (express.e:2791-2794).
    let spy = Arc::new(SpyFlaggedStore::default());
    {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(1, 0, "ansipack.lha"));
        spy.seeded.lock().unwrap().insert(2, flags); // slot 2 = test_user
    }
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow { terminal: &mut terminal, services: &services };
        flow.restore_flags_and_announce(&mut session).await.expect("restore");
    }
    assert!(
        session.flagged_files().contains(&FlaggedKey::new(1, 0, "ANSIPACK.LHA")),
        "the saved flag is restored into the session"
    );
    assert_eq!(
        terminal.output, FLAGGED_FILES_EXIST,
        "a non-empty restore emits exactly the banner"
    );
}

#[tokio::test]
async fn logon_with_no_saved_flags_is_silent() {
    let spy = Arc::new(SpyFlaggedStore::default()); // nothing seeded
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow { terminal: &mut terminal, services: &services };
        flow.restore_flags_and_announce(&mut session).await.expect("restore");
    }
    assert!(session.flagged_files().is_empty());
    assert!(terminal.output.is_empty(), "an empty restore emits no banner");
}

#[tokio::test]
async fn logon_with_a_load_error_starts_empty_and_silent() {
    let spy = Arc::new(SpyFlaggedStore { fail: true, ..SpyFlaggedStore::default() });
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow { terminal: &mut terminal, services: &services };
        flow.restore_flags_and_announce(&mut session).await.expect("restore");
    }
    assert!(session.flagged_files().is_empty());
    assert!(terminal.output.is_empty());
}
```

Import `FLAGGED_FILES_EXIST` in the test `use super::{...}` list.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo nextest run -p nextexpress logon_restores_flags_and_announces_when_non_empty`
Expected: FAIL — `no method named restore_flags_and_announce` / `cannot find FLAGGED_FILES_EXIST`.

- [ ] **Step 3: Add the const and the method**

In `rust/src/app/menu_flow/mod.rs`, add the const near `AUTOSAVING_FILE_FLAGS`:

```rust
/// `loadFlagged`'s restore notice (`amiexpress/express.e:2792-2793`),
/// emitted on logon when a non-empty flag set is restored: blank line,
/// the banner, then the `sendBELL` BEL and a trailing CRLF —
/// structurally identical to [`AUTOSAVING_FILE_FLAGS`]. Live-captured at
/// login in `comparison/transcripts/ae_tierd_alterflags.txt:77-81`.
const FLAGGED_FILES_EXIST: &[u8] = b"\r\n** Flagged File(s) Exist **\r\n\x07\r\n";
```

Add the method (in the `impl<'a, T> MenuFlow<'a, T>` block, near `run_logon_conference_scan`):

```rust
    /// Restores the user's saved flag set on logon (legacy `loadFlagged`,
    /// `amiexpress/express.e:2757`) and, when the restored set is
    /// non-empty, emits the `** Flagged File(s) Exist **` banner
    /// (`:2791-2794`) — the logon analogue of the logoff autosave banner.
    /// A load error logs and leaves the set empty; the caller still
    /// reaches the menu (slice D5-persist).
    pub(crate) async fn restore_flags_and_announce(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let slot = session.user().slot_number();
        match self.services.flagged_store.load(slot) {
            Ok(restored) => *session.flagged_files_mut() = restored,
            Err(error) => {
                eprintln!("loadFlagged: could not restore flags for slot {slot}: {error}");
            }
        }
        if !session.flagged_files_mut().is_empty() {
            self.write_and_flush(FLAGGED_FILES_EXIST).await?;
        }
        Ok(())
    }
```

- [ ] **Step 4: Run the tests (expect pass)**

Run: `cargo nextest run -p nextexpress 'logon_restores_flags\|logon_with_no_saved\|logon_with_a_load_error'`
Expected: PASS (3 tests).

- [ ] **Step 5: Call it in the logon sequence**

In `rust/src/app/session_driver.rs`, between `render_login_stats` (`:210`) and the menu run (`:211`), add the call. The block becomes:

```rust
                                self.render_login_stats(&menu).await?;
                                MenuFlow::new(&mut self.terminal, &self.services)
                                    .restore_flags_and_announce(&mut menu)
                                    .await?;
                                MenuFlow::new(&mut self.terminal, &self.services)
                                    .run(menu)
                                    .await?
```

- [ ] **Step 6: Run the suite**

Run: `cargo nextest run`
Expected: all pass. (Existing logon smokes start with an empty store → no banner → their wire is unchanged.)

- [ ] **Step 7: Commit**

```bash
git add rust/src/app/menu_flow/mod.rs rust/src/app/menu_flow/tests.rs rust/src/app/session_driver.rs
git commit -m "$(printf 'D5-persist: restore flags + Flagged File(s) Exist banner on logon\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 7: In-process logoff→logon→banner e2e smoke

Prove the round-trip over real telnet against the compiled listener, using the in-memory store shared across two connections (no spawned binary — avoids the SQLite-binary-spawn flakiness class).

**Files:**
- Modify: `rust/tests/tierd_file_list_smoke.rs` (new test + a `FLAGGED_FILES_EXIST` const)

**Interfaces:**
- Consumes: the existing `spawn_listener_with_demo_files`, `sign_in_seeded_sysop`, `write_line`, `drain_until`, `contains`, `end_session`, `PROMPT_TAIL` helpers in that file.

- [ ] **Step 1: Add the banner const**

In `rust/tests/tierd_file_list_smoke.rs`, near `AUTOSAVING_FILE_FLAGS`:

```rust
/// `loadFlagged`'s logon restore banner (`amiexpress/express.e:2792`),
/// live-captured at login (`ae_tierd_alterflags.txt:77-81`).
const FLAGGED_FILES_EXIST: &[u8] = b"\r\n** Flagged File(s) Exist **\r\n\x07\r\n";
```

- [ ] **Step 2: Write the failing smoke**

```rust
#[tokio::test]
async fn flags_persist_across_logoff_and_logon_over_telnet() {
    // Slice D5-persist: flag a name, log off (saveFlagged), then sign in
    // again as the same sysop on the same listener (shared in-memory
    // store) and see the `** Flagged File(s) Exist **` banner before the
    // menu, with `A` listing the restored name.
    let addr = spawn_listener_with_demo_files().await;

    // --- Session 1: flag MYDEMO.DMS via the A loop, then log off ---
    let mut s1 = sign_in_seeded_sysop(&addr).await;
    write_line(&mut s1, b"A").await;
    drain_until(&mut s1, PROMPT_TAIL).await;
    write_line(&mut s1, b"mydemo.dms").await; // flags + returns to menu
    drain_until(&mut s1, b"mins. left): ").await;
    write_line(&mut s1, b"G Y").await;
    drain_until(&mut s1, b"Goodbye").await;
    drop(s1);

    // --- Session 2: same user, same listener -> restored + banner ---
    let mut s2 = TcpStream::connect(addr).await.expect("reconnect");
    // Drive the login by hand so we can scan the whole logon stream for
    // the banner (sign_in_seeded_sysop drains past it to the menu).
    let login = drive_login_capturing(&mut s2).await;
    assert!(
        contains(&login, FLAGGED_FILES_EXIST),
        "the restored non-empty set announces at logon, got {:?}",
        String::from_utf8_lossy(&login),
    );

    write_line(&mut s2, b"A").await;
    let listed = drain_until(&mut s2, PROMPT_TAIL).await;
    assert!(
        contains(&listed, b"\r\nMYDEMO.DMS\r\n"),
        "A lists the restored flag, got {:?}",
        String::from_utf8_lossy(&listed),
    );
    // Clear so teardown is clean, then log off.
    write_line(&mut s2, b"C").await;
    drain_until(&mut s2, PROMPT_TAIL).await;
    write_line(&mut s2, b"*").await;
    drain_until(&mut s2, PROMPT_TAIL).await;
    write_line(&mut s2, b"").await;
    drain_until(&mut s2, b"mins. left): ").await;
    end_session(&mut s2).await;
}

/// Signs in `sysop`/`sysop` and returns the full byte stream from the
/// graphics prompt through to the menu prompt (so a caller can scan the
/// logon banners). Mirrors `sign_in_seeded_sysop` but returns the bytes.
async fn drive_login_capturing(stream: &mut TcpStream) -> Vec<u8> {
    drain_until(stream, b"(A/r/n)? ").await;
    write_line(stream, b"A").await;
    drain_until(stream, b"Name: ").await;
    write_line(stream, b"sysop").await;
    drain_until(stream, b"assword").await; // "Password:" / "PassWord:"
    write_line(stream, b"sysop").await;
    drain_until(stream, b"mins. left): ").await
}
```

(Check the exact name/graphics/password prompt needles against the existing `sign_in_seeded_sysop` in this file and reuse its literals; the helper above must drain the same markers it does.)

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo nextest run -p nextexpress --test tierd_file_list_smoke flags_persist_across_logoff_and_logon_over_telnet`
Expected: FAIL before Task 6 is in (banner absent). With Tasks 1–6 implemented it should already PASS — if so, confirm it genuinely exercises the path by temporarily asserting a wrong banner and seeing it fail, then revert.

- [ ] **Step 4: Verify it passes**

Run: `cargo nextest run -p nextexpress --test tierd_file_list_smoke flags_persist_across_logoff_and_logon_over_telnet`
Expected: PASS.

- [ ] **Step 5: Full suite + commit**

Run: `cargo nextest run` → all pass.

```bash
git add rust/tests/tierd_file_list_smoke.rs
git commit -m "$(printf 'D5-persist: in-process logoff/logon banner round-trip smoke\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

### Task 8: Mutation check, docs, and manual cross-restart verification

Close the slice: kill surviving mutants, update the parity/system docs, and do the human "type at it" check the scripted tests can't (cross-restart with SQLite).

**Files:**
- Modify: `SLICES.md`, `COMMAND_PARITY.md`, `SYSTEM.md`, `slices/cmds-files-list.md`, `designs/FILES.md`
- Update memory: `tierd-flag-confirm-capture.md` + `MEMORY.md` index line

- [ ] **Step 1: Run focused mutation testing**

Run: `make mutants-diff` (from repo root)
Expected: `0 missed`. For each surviving mutant, add or tighten a test (e.g. a mutant flipping the banner's `!is_empty()` gate → the `logon_with_no_saved_flags_is_silent` test must catch it; a mutant dropping the `save` call → `logoff_saves_the_flag_set...` catches it). Re-run until clean.

- [ ] **Step 2: Update `SLICES.md`**

Flip the D5-persist line to Done with: per-slot persistence behind the `FlaggedStore` port (in-memory default = process-lifetime, SQLite = durable, selected by `user_storage`); the `** Flagged File(s) Exist **` logon banner; keying `(conf, name)` with the documented `area`-drop / F-R-marker divergence.

- [ ] **Step 3: Update `COMMAND_PARITY.md`**

In the flag-story cell (the `On-row flag marker (slice D2f)` row) and the scope paragraph, record that cross-session persistence + the logon banner (slice D5-persist) have landed; note the `area`-drop divergence.

- [ ] **Step 4: Update `SYSTEM.md`**

Add the `FlaggedStore` port + its two adapters to the architecture (alongside `UserRepository`/SQLite). Update the `FlaggedFiles` description (it now persists via `FlaggedStore` on logoff and restores on logon). Ensure the diagram reflects the new port.

- [ ] **Step 5: Update `slices/cmds-files-list.md` and `designs/FILES.md`**

Add a D5-persist "Done" subsection mirroring the design (storage, keying, lifecycle, divergence). In `designs/FILES.md`, note the `flagged_files` table lives in `users.db`.

- [ ] **Step 6: Manual cross-restart check (SQLite)**

This is the path the in-process smoke can't cover (real restart). In a scratch dir:

```bash
cd /Users/paul/Documents/GitHub/nextexpress
printf 'port=2323\nbbs_path="."\nuser_storage="/tmp/d5check.db"\n' > /tmp/d5check.toml
rm -f /tmp/d5check.db
rust/target/debug/nextexpress /tmp/d5check.toml &  # boot 1
```

Then over telnet (`telnet 127.0.0.1 2323` or the byte driver): sign in `sysop`/`sysop`, `A`, type `mydemo.dms` to flag it, `G Y` to log off. Stop the server (`kill %1`), reboot it with the same config, sign in again, and confirm `** Flagged File(s) Exist **` renders before the menu and `A` lists `MYDEMO.DMS`. Clean up: clear with `C` `*`, `G Y`, `kill %1`, `rm -f /tmp/d5check.db`.

- [ ] **Step 7: Update memory**

In `tierd-flag-confirm-capture.md`, mark D5-persist DONE (the last owed flag surface); record the `FlaggedStore` port, the in-memory-default/SQLite-durable split, the `(conf, name)` keying + `area`-drop divergence, and the logon-banner const/position. Update the `MEMORY.md` index line.

- [ ] **Step 8: Final gate + commit**

Run: `cargo nextest run` (all pass), `cargo build` (no warnings), `cargo test --doc`, `cargo clippy --all-targets -- -D warnings`.

```bash
git add -A
git commit -m "$(printf 'D5-persist: docs, mutation hardening, parity records\n\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>')"
```

---

## Self-review

**Spec coverage:**
- Save on logoff → Task 5. Load + banner on logon → Task 6. Port + in-memory adapter → Task 2. SQLite adapter → Task 3. Config-driven selection → Task 4. Keying `(conf,name)`/area-drop → Tasks 1–3 (normalised in both adapters) + design divergence note. Graceful errors → Tasks 5 (save) + 6 (load). Default ephemeral / SQLite durable → Task 4 + Task 8 manual check. In-process smoke → Task 7. Deferred `dump`/`saveHistory` → not built (spec Out of scope). All spec sections map to a task.

**Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N". Two explicit "confirm the exact needle/slot against the existing helper" notes (Tasks 5, 7) are verification instructions, not missing code — the surrounding code is complete; they guard against fixture drift.

**Type consistency:** `FlaggedStore::{load,save}`, `FlaggedStoreError::Backend`, `SharedFlaggedStore`, `InMemoryFlaggedStore::new`, `SqliteFlaggedStore::{open,in_memory}`, `FlaggedFiles::entries`, `restore_flags_and_announce`, and the const `FLAGGED_FILES_EXIST` are named identically everywhere they appear across tasks. The save reads `session.user().slot_number()` + `session.flagged_files()`; the load writes `*session.flagged_files_mut()` — all accessors verified present in the codebase.
