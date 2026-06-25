# Slice D5-persist — flagged-file persistence + logon banner — design

Tier D, slice **D5-persist** (`slices/cmds-files-list.md`; the last owed
flag surface after Ga / D5-banner / D6a / D6b). Persists the session's
flagged-file set across logons and emits the legacy
`** Flagged File(s) Exist **` logon banner when a non-empty set is
restored.

Pairs with the existing flag work: the in-memory `FlaggedFiles` set
(`domain/files/flagged.rs`, slice D2f) that `F`/`R` and `A` build, the
`checkFlagged` leave confirm (slice Ga), and the unconditional
`** AutoSaving File Flags **` logoff banner (slice D5-banner). This slice
makes that set durable.

## Scope

**In scope:**

- Save the session flag set on logoff (the legacy `saveFlagged`,
  `amiexpress/express.e:2806`).
- Load it on the next logon for the **same user slot** (the legacy
  `loadFlagged`, `:2757`).
- Emit `\r\n** Flagged File(s) Exist **\r\n\x07\r\n` on logon when the
  restored set is non-empty (`:2791-2794`).
- A `FlaggedStore` port with an in-memory adapter (default) and a
  SQLite adapter (durable), wired by `config.user_storage`.

**Out of scope / deferred:**

- The legacy `dump<slot>` **partial-downloads** file and `saveHistory` /
  `loadHistory` — those track in-progress transfers and belong to the
  not-yet-built file-transfer slice. `loadFlagged` merges `dump` into the
  same list (`:2764-2772`); NextExpress restores only the `flagged` set.
- Clearing flags by name and `F`-from at the `A` prompt (already
  deferred in D6b — `slices/cmds-files-list.md` §"Slice D6b").
- Per-node flag files (the legacy `ownPartFiles` `flagged<node>-<slot>`
  form, `:2763`). NextExpress keys by user slot only; see Decision ④.

## Parity authority

The legacy mechanism (`amiexpress/express.e`, cross-checked against the
live capture `comparison/transcripts/ae_tierd_alterflags.txt:77-81`):

- **Storage**: two per-user files in a `Partdownload/` directory keyed by
  `loggedOnUser.slotNumber` — `dump<slot>` (partial downloads, deferred)
  and `flagged<slot>` (the flag set). With the `OWN_PARTFILES` tooltype
  the names gain a `<node>-` prefix (`:2763-2781`); the default form is
  slot-only.
- **`flagged` format**: one record per line, `{confNum} {fileName}\n`
  (`:2822-2823`). No header, no count, **no area field** — `flagFileItem`
  carries only `fileName` + `confNum` (`axobjects.e:289-292`).
- **`saveFlagged`** (on every logoff path, `:565/:7950/:25064/:28611/
  :28630`): deletes both files, then rewrites `flagged` only if
  `flagFilesList.count() > 0` (`:2806-2827`). An empty set leaves no file
  — restore finds nothing.
- **`loadFlagged`** (logon, `SUBSTATE_DISPLAY_CONF_BULL`, `:28576`): runs
  after `joinConf` (auto-rejoin + bulletin, `:28574`) and before the menu
  (`:28582`). Reads the file(s), then if `count() > 0` emits the banner.
- **Banner wire**: `aePuts('\b\n** Flagged File(s) Exist **\b\n')` +
  `sendBELL()` (`:2792-2793`) → `\r\n** Flagged File(s) Exist **\r\n
  \x07\r\n` on the wire (plain ASCII + BEL, no ANSI). Structurally
  identical to the D5-banner `AUTOSAVING_FILE_FLAGS` const.

## Decisions

### ① Keying: persist `(conference, name)`, drop `area`

`FlaggedKey` is `(conference, area, name)` (`flagged.rs:11-14`). The
legacy stores only `(confNum, fileName)`; `area` is a NextExpress
session-local concern the `F`/`R` pager uses to render the on-row `[X]`
marker during a scan (`file_list/wire.rs:294`), and the `A` inline-add
already keys with `area = 0` (`menu_flow/mod.rs`, `flag_add`).

Persist `(conference, name)` and **restore with `area = 0`**. This:

- matches the legacy file format exactly (`{conf} {NAME}` per row),
- matches the `A` model (flags keyed by conference at flag-time),
- loses nothing the listing (`A` shows names only) or the eventual
  download (`checkForFileSize` takes name + confNum, `express.e:16010`)
  needs.

`(conference, name)` is the **persisted identity**; `area` is never
stored. The `FlaggedStore` contract is uniform across adapters: a flag
saved at any area loads back at `area = 0` (the in-memory adapter
normalises on save exactly as the SQLite projection does, so swapping
backends never changes behaviour).

Documented divergence: a file flagged via `F`/`R` in `(conf, area=3,
NAME)` round-trips as `(conf, area=0, NAME)`. It still appears in the
logon banner (count) and the `A` listing (names only). It will **not**
repaint the `[X]` marker on a *next-session* `F`/`R` scan, because that
scan builds `(conf, area=N, NAME)` keys and `contains` matches the full
key — the restored flag must be re-flagged to mark again. The legacy
matches by `(conf, name)` regardless of area, so this is a minor,
documented NextExpress limitation; making the flag-match area-agnostic
is a separate later refinement, out of scope here.

### ② Separate `SqliteFlaggedStore`, not an extension of `SqliteUserRepository`

The flag table lives **in `users.db`** (the user's chosen backend), but a
dedicated `SqliteFlaggedStore` opens its **own connection** to the same
path rather than growing `SqliteUserRepository` (which already owns a
single `Mutex<Connection>`, `sqlite_user_repository.rs:59`). Rationale:

- keeps the user adapter focused (it is already large),
- keeps the two ports decoupled and independently testable (the flag
  store gets its own `in_memory()` constructor for unit tests),
- two connections to one WAL file is safe at BBS write concurrency; the
  flag store sets the same pragmas the user repo does (WAL,
  `busy_timeout`).

No foreign key to `users` (the two connections own independent schemas);
the slot is a plain integer key, mirroring the legacy's referential
looseness.

### ③ Graceful error handling

Storage failures never degrade the session (matching the legacy's
tolerance — `DeleteFile`/`Open` failures are effectively ignored):

- **load failure** at logon → start with an empty set, **no banner**, log
  a warning (`eprintln!`). The caller still reaches the menu.
- **save failure** at logoff → log a warning, let the logoff proceed.

### ④ Default is ephemeral; durability requires SQLite

`config.user_storage = None` (the default) wires `InMemoryFlaggedStore`;
`Some(path)` wires `SqliteFlaggedStore(path)` — the **same switch** that
already selects the user repository (`bootstrap.rs:153-184`,
`config.rs:146`). No new config key.

- **`InMemoryFlaggedStore`** retains for the **process lifetime** (a
  `Mutex<HashMap<u32, FlaggedFiles>>`), the analogue of
  `InMemoryUserRepository`. A logoff→logon round-trip within one running
  server restores the set and fires the banner; **a process restart
  clears it**.
- **`SqliteFlaggedStore`** persists across restarts.

This is consistent with how users already behave (seeded in-memory by
default, durable under SQLite).

## Architecture

### The port

`pub trait FlaggedStore` in `domain/files/` (synchronous, matching
`UserRepository`):

```rust
/// Durable home of a user's flagged-file set (slice D5-persist).
pub trait FlaggedStore {
    /// Loads the flag set saved for `slot`, or an empty set when none.
    /// # Errors
    /// Returns [`FlaggedStoreError`] when the backing store cannot be read.
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError>;

    /// Replaces the saved flag set for `slot` with `flags` (an empty set
    /// clears it). # Errors as above.
    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError>;
}
```

`FlaggedFiles` / `FlaggedKey` widen from `pub(crate)` to `pub` so they can
appear in the port signature (mirroring `UserRepository` returning the
`pub` `User`). The port gains the small accessors it needs:

- `FlaggedFiles::entries(&self) -> impl Iterator<Item = (u32, &str)>`
  (conference + name, for `save`),
- restore reuses the existing `flag(FlaggedKey::new(conf, 0, name))`.

### Adapters

- `adapters/in_memory_flagged_store.rs` — `Mutex<HashMap<u32,
  FlaggedFiles>>`; `load` clones the entry (or empty), `save` replaces it.
- `adapters/sqlite_flagged_store.rs` — `Mutex<Connection>` opened at the
  `user_storage` path, WAL + `busy_timeout`, owning one table:

```sql
CREATE TABLE IF NOT EXISTS flagged_files (
  slot_number INTEGER NOT NULL,
  conference  INTEGER NOT NULL,
  name        TEXT    NOT NULL,
  PRIMARY KEY (slot_number, conference, name)
);
```

`save` is one transaction: `DELETE FROM flagged_files WHERE slot_number
= ?1`, then an `INSERT` per entry (an empty set performs only the
delete — the legacy "rewrite, or no file" semantics). `load` is
`SELECT conference, name FROM flagged_files WHERE slot_number = ?1`,
folded into a `FlaggedFiles` with `area = 0`.

### Composition

`AppServices` gains `flagged_store: SharedFlaggedStore` (`Arc<dyn
FlaggedStore + Send + Sync>`), threaded from `bootstrap::build_runtime`
alongside `user_repo`. Unit-test `AppServices` literals gain
`flagged_store: Arc::new(InMemoryFlaggedStore::new())` (mechanical).
Hexagonal boundary unchanged: the port lives in `domain/`, adapters in
`adapters/`, wired in `bootstrap.rs`; `tests/architecture.rs` continues
to forbid `crate::adapters` imports outside the composition root.

## Lifecycle integration

### Load + banner on logon

A new `MenuFlow` method — `restore_flags_and_announce` — inserted in
`session_driver.rs` **after `render_login_stats` (`:210`) and before
`MenuFlow::run(menu)` (`:211`)**, so the banner lands immediately before
the menu, matching the capture position (after the auto-rejoin/stats,
before the prompt). It:

1. reads `slot = menu.user().slot_number()`,
2. `self.services.flagged_store.load(slot)` → on `Ok`, replaces
   `menu.flagged_files_mut()` with the restored set; on `Err`, logs and
   leaves the set empty,
3. if the (restored) set is non-empty, writes
   `FLAGGED_FILES_EXIST = b"\r\n** Flagged File(s) Exist **\r\n\x07\r\n"`.

### Save on logoff

In `handle_logoff` (`menu_flow/mod.rs`), immediately **after the
`AUTOSAVING_FILE_FLAGS` emit (`:538`) and before `user_requests_logoff`
(`:539`)**: `self.services.flagged_store.save(slot,
session.flagged_files())`, `slot = session.user().slot_number()`. This
sits on every real logoff path (the Stay branch returns earlier). A save
error is logged and ignored.

## Testing

Test-first, per `AGENTS.md`:

- **Adapter unit tests**
  - `SqliteFlaggedStore` (via `Connection::open_in_memory`): save→load
    round-trip; `area` dropped on save and restored as `0`; empty-set
    save clears; idempotent re-save; slot isolation (slot 1 vs slot 2).
  - `InMemoryFlaggedStore`: same round-trip + process-lifetime retention;
    missing slot loads empty.
- **Lifecycle unit tests** (menu_flow, with a spy `FlaggedStore`)
  - logon restore populates the session set and emits the banner **only**
    when the loaded set is non-empty (empty → no banner bytes);
  - load error → empty set, no banner, session still continues;
  - logoff calls `save` with the live flag set; save error doesn't block
    logoff.
- **In-process e2e smoke** (the meaningful one; `quickwins_smoke.rs`
  shape — in-process `TelnetListener` on `127.0.0.1:0`, **not** a spawned
  binary, to avoid the SQLite-binary-spawn flakiness class): one listener
  with a shared `InMemoryFlaggedStore`; connection 1 flags a name via `A`
  and logs off with `G Y`; connection 2 signs in as the same sysop and
  asserts `** Flagged File(s) Exist **` + BEL appear before the menu and
  that `A` lists the restored name.
- `make mutants-diff` clean on the new code.
- **Manual "type at it"**: boot the binary with a `user_storage` SQLite
  path configured, flag a file, log off, restart the binary, log back on,
  and confirm the banner renders (the cross-restart path the in-process
  smoke can't cover).

## Files touched

- `domain/files/flagged.rs` — widen `FlaggedFiles`/`FlaggedKey` to `pub`;
  add `FlaggedFiles::entries()` (the `(conf, name)` iterator `save`
  consumes). Restore reuses the existing `flag()`, so no other new
  accessor is required.
- `domain/files/flagged_store.rs` (new) — the port + `FlaggedStoreError`.
- `adapters/in_memory_flagged_store.rs` (new), `adapters/sqlite_flagged_store.rs` (new).
- `app/services.rs` — `flagged_store` field; `bootstrap.rs` — wiring.
- `app/session_driver.rs` — the logon `restore_flags_and_announce` call.
- `app/menu_flow/mod.rs` — `FLAGGED_FILES_EXIST` const, the logon method,
  the logoff `save` call.
- `tests/` — a new in-process flag-persistence smoke.
- Docs: `SLICES.md`, `COMMAND_PARITY.md`, `SYSTEM.md`,
  `slices/cmds-files-list.md`, `designs/FILES.md` (flag table note).
