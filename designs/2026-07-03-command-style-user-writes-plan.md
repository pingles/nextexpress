# Command-Style User Writes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the whole-aggregate `UserRepository::save(User)` with
command-style writes (SYSTEM.md ordered-table row 9 / detail item 1) so
concurrent writers compose instead of overwriting each other, and so no
multi-statement user write runs outside a transaction.

**Architecture:** Three new port methods — `record_auth_outcome`
(semantic auth command at `verify_password`), `record_password_change`
(authoritative credential write at `complete_password_reset`), and
`apply_user_patch` (delta/patch write used at both `enter_menu` and
`finalise_logoff`) — with the merge semantics defined once in the domain
(`domain/user/commands.rs` appliers). The domain yields per-call deltas
via baseline-snapshot-and-diff: `Session` keeps a `persist_baseline:
Option<Box<PersistedUser>>` refreshed after every successful persist;
`UserPatch::between(baseline, current)` computes the patch. No mutation
site re-routes. The in-memory adapter applies commands through the domain
appliers; the SQLite adapter implements the same semantics in SQL (one
transaction per command); a shared contract-test suite pins the two
adapters to each other. `save()` is deleted at the end — its
DELETE+reinsert tear path goes with it.

**Design note (deviation from the SYSTEM.md sketch):** SYSTEM.md item 1
sketched `record_logon` and `apply_logoff_patch` as separate commands.
This plan collapses them into one `apply_user_patch` with two call
sites: both are baseline-diff patches with identical payload shape and
adapter internals (enter_menu's patch is `times_called_delta = 1` plus
whatever auto-rejoin/logon-scan touched; finalise's patch is the
mid-session deltas plus `last_call`). Two names for one mechanism would
duplicate the adapter code the parity suite has to pin. SYSTEM.md also
put `last_call` in `record_logon`; the code writes `last_call` at
`finalise_logoff` (`lifecycle.rs:131-133`) and behaviour parity wins.
The fifth save site (`complete_password_reset`, `session_flow.rs:698`)
was not in the sketch; it gets `record_password_change` per
designs/USERS.md ("security fields are immediate authoritative writes").

**Tech Stack:** Rust, rusqlite (single `Mutex<Connection>`), cargo-nextest,
cargo-mutants.

## Global Constraints

- TDD: every task starts with a failing test (AGENTS.md "Key Workflow").
- `make mutants-diff` (working tree vs last commit) must come back clean
  or with explicitly-justified survivors before each commit (run from the
  repo root; it generates the crate-relative diff itself).
- No compile warnings (`cargo build`), doctests pass
  (`cargo test --doc`), clippy is enforced by the session-stop hook.
- No schema change to users.db — every column the commands write already
  exists (`sqlite_user_repository.rs:96-161`). Do NOT add columns.
- No wire/behaviour change: session_flow's existing tests pin persisted
  effects at each save point and must stay green unmodified in Task 5
  (except the two test doubles they construct).
- All `cargo` commands run from `rust/`; `make mutants-diff` from repo
  root. Do not run other cargo commands while mutants is running.
- Commit per task, message style matching recent history
  (`Users: ...` prefix), each ending with the two required trailers
  (Co-Authored-By + Claude-Session, see session config).
- New public items need doc comments (params/returns/errors documented).

## File Structure

- **Create** `rust/src/domain/user/commands.rs` — command payload types
  (`AuthOutcome`, `DailyBudgetOutcome`, `PasswordChange`, `UserPatch`,
  `MembershipPatch`, `PointerPatch`, `ScanFlagSettings`), the
  `UserPatch::between` diff, and the `apply_to(&mut PersistedUser)`
  appliers that define each command's merge semantics.
- **Create** `rust/src/adapters/user_repository_contract.rs` —
  `#[cfg(test)]` generic contract suite run against both adapters.
- **Create** `rust/tests/two_session_logons_smoke.rs` — e2e lost-update
  regression.
- **Modify** `rust/src/domain/user/mod.rs` (module decl + re-exports),
  `rust/src/domain/session/budget.rs` (`daily_budget_outcome` helper),
  `rust/src/domain/session/mod.rs` + `identity.rs` + `registration.rs`
  (baseline field and methods), `rust/src/domain/user_repository.rs`
  (trait), `rust/src/adapters/in_memory_user_repository.rs`,
  `rust/src/adapters/sqlite_user_repository.rs`,
  `rust/src/adapters/mod.rs` (contract module decl),
  `rust/src/app/session_flow.rs` (five call sites + helpers + TestRepo),
  `rust/src/app/session_driver.rs` (SaveFailingRepo),
  `SYSTEM.md`, `designs/USERS.md`.

---

### Task 1: Domain command vocabulary and appliers

**Files:**
- Create: `rust/src/domain/user/commands.rs`
- Modify: `rust/src/domain/user/mod.rs` (module decl + re-export, next to
  the existing `persisted` module pattern)
- Modify: `rust/src/domain/session/budget.rs` (extract
  `daily_budget_outcome`)

**Interfaces (produced, used by every later task):**

```rust
// domain/user/commands.rs — all pub, re-exported from domain::user
pub enum DailyBudgetOutcome { NewDay, SameDay }

pub enum AuthOutcome {
    Matched { daily: Option<DailyBudgetOutcome>, force_password_reset: bool },
    Mismatched { lock_account: bool },
}

pub struct PasswordChange {
    pub hash: String,
    pub salt: Option<String>,
    pub kind: PasswordHashKind,
    pub changed_at: SystemTime,
}

#[derive(Default)]
pub struct UserPatch {
    pub times_called_delta: u32,
    pub times_called_today_delta: u32,
    pub time_used_today_delta: Duration,
    pub messages_posted_delta: u32,
    pub last_call: Option<SystemTime>,          // monotonic MAX
    pub expert_mode: Option<bool>,              // set iff changed (LWW)
    pub flags: Option<BTreeSet<UserFlag>>,      // set iff changed (LWW)
    pub last_joined: Option<MessageBaseRef>,    // set iff changed (LWW, pair)
    pub memberships: Vec<MembershipPatch>,
}

pub struct MembershipPatch {
    pub conference_number: u32,
    pub create_if_missing: bool,
    pub granted: Option<bool>,                  // set iff changed
    pub messages_posted_delta: u32,
    pub scan_flags: Option<ScanFlagSettings>,   // set iff any flag changed
    pub pointers: Vec<PointerPatch>,            // rows new or advanced
}

#[derive(Clone, Copy)]
pub struct ScanFlagSettings {
    pub mail_scan: bool,
    pub mailscan_all: bool,
    pub file_scan: bool,
    pub zoom_scan: bool,
}

pub struct PointerPatch {
    pub msgbase_number: u32,
    pub last_read: u32,       // MAX-merged
    pub last_scanned: u32,    // MAX-merged
    pub new_since: SystemTime, // used only when the row is new; existing rows keep theirs
}

impl UserPatch {
    pub fn between(baseline: &PersistedUser, current: &PersistedUser) -> UserPatch;
    pub fn apply_to(&self, snapshot: &mut PersistedUser);
}
impl AuthOutcome  { pub fn apply_to(&self, snapshot: &mut PersistedUser); }
impl PasswordChange { pub fn apply_to(&self, snapshot: &mut PersistedUser); }

// domain/session/budget.rs
pub fn daily_budget_outcome(
    last_call: Option<SystemTime>,
    now: SystemTime,
    daily_reset_offset: Duration,
) -> DailyBudgetOutcome;
```

Derive `Debug, Clone, PartialEq` on all of these (plus `Eq` where the
fields allow; `Copy` only on `ScanFlagSettings` and
`DailyBudgetOutcome`). `Default` on `UserPatch` (all-zero/None/empty).

**Merge semantics (the single source of truth — SQL and in-memory must
both implement exactly this):**

- `AuthOutcome::Matched`: `invalid_attempts = 0`;
  `force_password_reset |= force_password_reset` field (never cleared by
  this command — MAX semantics); daily `NewDay` →
  `times_called_today = 0` and `time_used_today = 0` (absolutes — note
  the legacy quirk: a new-day logon leaves the counter at 0 because
  `initialise_daily_budget` resets without bumping, `budget.rs:54-58`;
  preserve it); daily `SameDay` → `times_called_today += 1`; daily
  `None` (rejected-logon path where the budget rule never ran) → leave
  both counters alone.
- `AuthOutcome::Mismatched`: `invalid_attempts += 1` (additive — two
  concurrent wrong-password sessions must both count);
  `lock_account: true` → `account_locked = true`, `false` → leave it.
- `PasswordChange`: overwrite hash/salt/kind, `password_last_updated =
  changed_at`, `force_password_reset = false`.
- `UserPatch`: the four counters are additive; `last_call` merges as
  `max(existing, patch)` (existing `None` → take patch); `expert_mode`
  / `flags` / `last_joined` overwrite only when `Some`. Membership
  patches match rows by `conference_number`; `create_if_missing` creates
  the row first (granted from the patch, other fields at schema
  defaults: posted 0, mail_scan 1, mailscan_all 0, file_scan 1,
  zoom_scan 0); pointer rows MAX-merge `last_read`/`last_scanned`
  pairwise and keep the existing row's `new_since` (a new row takes the
  patch's `new_since`). Pairwise MAX preserves the
  `last_read <= last_scanned` invariant. Memberships never disappear
  mid-session, so rows in baseline but not current are ignored.

**Diff semantics (`UserPatch::between`):** deltas are
`current.saturating_sub(baseline)` for the four counters; `last_call` /
`expert_mode` / `flags` / `last_joined` are `Some(current)` iff
`current != baseline`; a current membership missing from baseline
becomes `create_if_missing: true` with `granted: Some`, `scan_flags:
Some`, `messages_posted_delta = full count`, and all its pointer rows;
an existing membership contributes a `MembershipPatch` only if
something changed (granted, any scan flag, posted count, or a pointer
row that is new or has advanced). Document on the type: the patch
covers exactly the session-mutable families (counters, pointers,
scan flags, last_joined, expert/flags, last_call); credential changes
must go through `PasswordChange` and are deliberately not diffed.

- [ ] **Step 1: Write failing unit tests** in a `#[cfg(test)] mod tests`
  inside `commands.rs`. Build two `PersistedUser` fixtures by hand (a
  ~30-field struct literal; crib the field list from
  `domain/user/persisted.rs:27-93`) and cover, at minimum:
  - `between_then_apply_reproduces_current`: mutate a baseline clone
    through every supported family (bump the four counters, advance
    `last_call`, flip `expert_mode`, change `flags`, change
    `last_joined`, add a membership, advance a pointer row, add a new
    pointer row, flip a scan flag, bump membership posted count), then
    assert `patch.apply_to(&mut baseline_clone)` reproduces `current`
    field-for-field (compare via the fields, or derive `PartialEq` on
    `PersistedUser` if not present — check first; if you add a derive,
    keep it `cfg`-unconditional and note it in the commit).
  - `between_of_identical_snapshots_is_default_patch`.
  - `patch_last_call_max_keeps_later_stored_value`: apply a patch whose
    `last_call` is EARLIER than the snapshot's → snapshot unchanged.
  - `pointer_merge_is_pairwise_max_and_keeps_existing_new_since`:
    existing row (5, 9, new_since=T1), patch row (3, 12, new_since=T2)
    → (5, 12, T1).
  - `auth_mismatch_is_additive_and_lock_is_one_way`: snapshot with
    `invalid_attempts: 5` + `Mismatched { lock_account: false }` → 6,
    unlocked; then `Mismatched { lock_account: true }` → 7, locked.
  - `auth_matched_new_day_resets_but_does_not_bump`: tct 7 / tut 600s →
    0 / 0. `auth_matched_same_day_bumps`: tct 7 → 8, tut unchanged.
  - `auth_matched_never_clears_force_reset`: snapshot flag true +
    `Matched { force_password_reset: false, .. }` → still true.
  - `password_change_replaces_credentials_and_clears_flag`.
  - `daily_budget_outcome_matches_initialise_daily_budget`: port the
    day-boundary cases from the existing budget tests (same day, across
    the `daily_reset_offset` boundary, `last_call: None` → `NewDay`).

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `rust/`): `cargo nextest run commands`
Expected: FAIL — module doesn't exist / doesn't compile.

- [ ] **Step 3: Implement** `commands.rs` (types + `between` + the three
  `apply_to`s), declare `mod commands;` + `pub use commands::*;`-style
  re-exports in `domain/user/mod.rs` (follow the file's existing
  `persisted` re-export style), and extract `daily_budget_outcome` into
  `budget.rs`:

```rust
// budget.rs — new pub fn; initialise_daily_budget rewritten to use it
pub fn daily_budget_outcome(
    last_call: Option<SystemTime>,
    now: SystemTime,
    daily_reset_offset: Duration,
) -> DailyBudgetOutcome {
    let today = floor_to_day(now, daily_reset_offset);
    let last_call_day = last_call.map(|t| floor_to_day(t, daily_reset_offset));
    if last_call_day.is_none_or(|d| d != today) {
        DailyBudgetOutcome::NewDay
    } else {
        DailyBudgetOutcome::SameDay
    }
}
```

  In `initialise_daily_budget`, replace the inline `is_new_day`
  computation (`budget.rs:48-58`) with a match on this helper. For the
  membership-creation part of `UserPatch::apply_to`, use the same
  reconstruction technique as `SqliteUserRepository::load_memberships`
  (`sqlite_user_repository.rs:410-424`): `ConferenceMembership::new`,
  `set_scan_flag`, a `bump_messages_posted` loop, `upsert_pointers`.
  For pointer MAX-merge on an existing row, read via `pointers_for`,
  build the merged row with `ReadPointers::new(msgbase, max_read,
  max_scanned, existing.new_since())` (`.expect` with a reason — inputs
  come from rows that already satisfy the invariant), and
  `upsert_pointers` it back.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run commands` then the full `cargo nextest run`
(budget refactor must not break existing session tests).
Expected: PASS, full suite green.

- [ ] **Step 5: Mutants gate + commit**

Run from repo root: `make mutants-diff`
Expected: no missed mutants in `commands.rs` / `budget.rs` (the appliers
and diff are pure Rust — arithmetic and match-arm mutants must die to
the Step-1 tests; strengthen tests if any survive).

```bash
git add rust/src/domain/user/commands.rs rust/src/domain/user/mod.rs rust/src/domain/session/budget.rs
git commit -m "Users: domain command vocabulary for command-style writes"
```

---

### Task 2: Port methods + in-memory adapter + contract suite

**Files:**
- Modify: `rust/src/domain/user_repository.rs` (trait, after
  `create_user` at :169)
- Modify: `rust/src/adapters/in_memory_user_repository.rs`
- Create: `rust/src/adapters/user_repository_contract.rs`
- Modify: `rust/src/adapters/mod.rs` (add
  `#[cfg(test)] pub(crate) mod user_repository_contract;`)

**Interfaces:**
- Consumes: Task 1's types (`domain::user::{AuthOutcome, PasswordChange,
  UserPatch, ...}`).
- Produces (trait methods; NO default implementations — every impl and
  test double must be forced by the compiler to implement the real
  path, otherwise lost-update semantics silently survive in doubles):

```rust
/// Applies the persistent consequences of one password-verification
/// attempt. [documente the Matched/Mismatched semantics per Task 1]
///
/// # Errors
/// `UserNotFound` when no user occupies `slot` (handle reported as
/// "slot N"); `Storage` when the backing store fails.
fn record_auth_outcome(&self, slot: u32, outcome: &AuthOutcome)
    -> Result<(), UserRepositoryError>;

/// Authoritative credential replacement (CompletePasswordReset).
/// # Errors — as above.
fn record_password_change(&self, slot: u32, change: &PasswordChange)
    -> Result<(), UserRepositoryError>;

/// Applies a delta/patch write. Additive counters and monotonic
/// merges make concurrent same-account sessions compose instead of
/// overwriting each other (designs/USERS.md "command-style writes").
/// # Errors — as above.
fn apply_user_patch(&self, slot: u32, patch: &UserPatch)
    -> Result<(), UserRepositoryError>;
```

- [ ] **Step 1: Write the contract suite (failing).** In
  `user_repository_contract.rs`, write `pub(crate)` generic functions
  taking a factory, plus a fixture builder using
  `User::from_persisted` so counters carry distinct sentinels:

```rust
//! Shared behavioural contract for [`UserRepository`] implementations.
//! Each adapter's test module instantiates every function here; the
//! SQL implementation and the domain appliers must agree exactly.
//! SQL string contents are invisible to cargo-mutants, so these
//! sentinel-value assertions are the guard against `+` drifting to
//! `=`, MAX to MIN, and absolute writes clobbering concurrent bumps.

pub(crate) fn seeded_user(slot: u32, handle: &str) -> User { /* PersistedUser literal:
    invalid_attempts: 5, times_called: 10, times_called_today: 7,
    time_used_today: 600s, messages_posted: 3, last_call:
    Some(EPOCH+1_000_000s), force_password_reset: false, one membership
    (conf 1, granted, posted 2, mail_scan on, pointers (msgbase 1):
    last_read 5, last_scanned 9, new_since EPOCH+500s),
    last_joined: Some(MessageBaseRef::new(1, 1)), rest defaults —
    then User::from_persisted(...).expect("valid persisted user") */ }
```

  Contract functions (each takes `make: impl Fn(Vec<User>) -> R` with
  `R: UserRepository`, seeds `seeded_user(7, "alice")`, runs commands,
  re-reads via `find_by_handle("alice")` and asserts on
  `user.to_persisted()`):
  - `mismatch_bumps_additively`: two `Mismatched { lock_account: false }`
    → `invalid_attempts == 7`.
  - `mismatch_lock_is_one_way`, `matched_clears_attempts`,
    `matched_new_day_resets_counters` (tct 0, tut 0),
    `matched_same_day_bumps_today` (tct 8),
    `matched_rejected_path_leaves_daily_counters` (daily: None → 7/600s),
    `matched_does_not_unset_force_reset` (seed flag true via a variant
    fixture),
  - `password_change_replaces_credentials_and_clears_flag`,
  - `patch_counters_are_additive` (patch with times_called_delta 1
    applied twice → 12),
  - `patch_last_call_is_monotonic` (earlier patch value → stored keeps
    later; later → advances),
  - `patch_pointer_rows_max_merge_and_keep_new_since` (patch (3,12) on
    stored (5,9) → (5,12), new_since unchanged),
  - `patch_creates_missing_membership_with_pointer_rows` (conf 2,
    create_if_missing, one pointer row — exercises the FK ordering),
  - `patch_preferences_are_last_writer_wins` (expert Some(true) sets;
    None leaves; last_joined pair updates together),
  - `interleaved_sessions_do_not_lose_updates` — the headline: seed one
    user; simulate session A and session B each doing
    `record_auth_outcome(Matched, SameDay)` then
    `apply_user_patch(times_called_delta: 1)` interleaved
    (A-auth, B-auth, B-patch, A-patch); assert `times_called == 12`
    and `times_called_today == 9`. Under whole-aggregate saves this
    interleaving loses one logon.
  - `unknown_slot_is_user_not_found` for each of the three commands
    (assert `matches!(err, UserRepositoryError::UserNotFound { .. })`).

  Then in `in_memory_user_repository.rs`'s `mod tests`, instantiate each
  as a `#[test]` calling the contract fn with
  `|users| InMemoryUserRepository::new(users)`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run user_repository`
Expected: compile FAIL — trait methods don't exist.

- [ ] **Step 3: Implement.** Add the three methods to the trait (doc
  comments per the interface block). In-memory implementation — one
  private helper, three one-liners:

```rust
impl InMemoryUserRepository {
    /// Runs a command against the stored record for `slot` by
    /// snapshotting it, applying the domain merge, and rebuilding the
    /// aggregate — the same semantics the SQL adapter implements
    /// statement-by-statement.
    fn apply_command(
        &self,
        slot: u32,
        apply: impl FnOnce(&mut PersistedUser),
    ) -> Result<(), UserRepositoryError> {
        let mut users = self.users.lock().expect("user repository mutex");
        let Some(existing) = users.iter_mut().find(|u| u.slot_number() == slot) else {
            return Err(UserRepositoryError::UserNotFound { handle: format!("slot {slot}") });
        };
        let mut snapshot = existing.to_persisted();
        apply(&mut snapshot);
        *existing = User::from_persisted(snapshot)
            .map_err(|error| UserRepositoryError::storage("apply command", error))?;
        Ok(())
    }
}
// trait impls:
fn record_auth_outcome(&self, slot: u32, outcome: &AuthOutcome) -> Result<(), UserRepositoryError> {
    self.apply_command(slot, |s| outcome.apply_to(s))
}
// record_password_change / apply_user_patch identically.
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo nextest run` (full suite — trait change breaks the two flow
test doubles: add the three methods to `TestRepo`
(`session_flow.rs:755`) and `SaveFailingRepo` (`session_driver.rs:575`)
now, in the minimal form — `TestRepo` gets the same `apply_command`
technique (it already holds `Mutex<Vec<User>>`); `SaveFailingRepo`
routes all three through the same counter gate as `save` and delegates
to its inner repo — the gate refactor to shared `fn gate()` happens in
Task 5).
Expected: PASS.

- [ ] **Step 5: Mutants gate + commit**

Run: `make mutants-diff`. Fix or justify survivors.

```bash
git add rust/src/domain/user_repository.rs rust/src/adapters/in_memory_user_repository.rs rust/src/adapters/user_repository_contract.rs rust/src/adapters/mod.rs rust/src/app/session_flow.rs rust/src/app/session_driver.rs
git commit -m "Users: command-write port methods + in-memory adapter + contract suite"
```

---

### Task 3: SQLite implementation (transactional) + parity + seed fix

**Files:**
- Modify: `rust/src/adapters/sqlite_user_repository.rs`

**Interfaces:**
- Consumes: Task 1 types, Task 2 trait methods and contract suite.
- Produces: the SQLite implementations + `insert_seed` in a transaction.

- [ ] **Step 1: Failing tests.** In the sqlite adapter's `mod tests`:
  (a) instantiate every contract function with a factory:

```rust
fn seeded_sqlite(users: Vec<User>) -> SqliteUserRepository {
    let repo = SqliteUserRepository::in_memory().expect("open in-memory db");
    for user in &users { repo.insert_seed(user).expect("seed user"); }
    repo
}
```

  (b) add the **transaction-atomicity pin** (this is the tear fix's
  regression test — it fails against a non-transactional
  implementation):

```rust
#[test]
fn apply_user_patch_rolls_back_wholesale_on_mid_patch_failure() {
    // A pointer row for a conference with no membership row violates
    // the composite FK. The users-table counter bump in the same
    // patch must roll back with it — a partial (torn) application
    // was exactly the defect of the old bare-connection save().
    let repo = seeded_sqlite(vec![contract::seeded_user(7, "alice")]);
    let patch = UserPatch {
        times_called_delta: 1,
        memberships: vec![MembershipPatch {
            conference_number: 99,
            create_if_missing: false, // no membership row -> FK failure
            granted: None,
            messages_posted_delta: 0,
            scan_flags: None,
            pointers: vec![PointerPatch {
                msgbase_number: 1, last_read: 1, last_scanned: 1,
                new_since: SystemTime::UNIX_EPOCH,
            }],
        }],
        ..UserPatch::default()
    };
    repo.apply_user_patch(7, &patch).expect_err("FK violation");
    let NameLookupResult::Found(user) = repo.find_by_handle("alice").expect("lookup") else {
        panic!("seeded user must exist");
    };
    assert_eq!(user.to_persisted().times_called, 10, "counter bump must roll back");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run sqlite_user`
Expected: compile FAIL (methods unimplemented for
`SqliteUserRepository`) — rusqlite requires all trait methods.
(If you stub with `todo!()` first, the contract tests fail at runtime.)

- [ ] **Step 3: Implement the three methods.**

`record_auth_outcome` — single UPDATE (implicitly atomic), reusing the
existing conversion helpers (`system_time_to_secs`, `duration_to_secs`,
`flags_to_bitmask`, `hash_kind_to_str` — see the upsert at :266-299 for
usage):

```rust
fn record_auth_outcome(&self, slot: u32, outcome: &AuthOutcome) -> Result<(), UserRepositoryError> {
    let conn = self.conn.lock().expect("user db mutex");
    let changed = match outcome {
        AuthOutcome::Matched { daily, force_password_reset } => {
            let daily_sql = match daily {
                Some(DailyBudgetOutcome::NewDay) =>
                    "times_called_today = 0, time_used_today_secs = 0,",
                Some(DailyBudgetOutcome::SameDay) =>
                    "times_called_today = times_called_today + 1,",
                None => "",
            };
            let sql = format!(
                "UPDATE users SET {daily_sql}
                     invalid_attempts = 0,
                     force_password_reset = MAX(force_password_reset, ?2)
                 WHERE slot_number = ?1"
            );
            conn.execute(&sql, params![slot, i64::from(*force_password_reset)])
        }
        AuthOutcome::Mismatched { lock_account } => conn.execute(
            "UPDATE users SET
                 invalid_attempts = invalid_attempts + 1,
                 account_locked = MAX(account_locked, ?2)
             WHERE slot_number = ?1",
            params![slot, i64::from(*lock_account)],
        ),
    }
    .map_err(|error| UserRepositoryError::storage("record auth outcome", error))?;
    Self::require_row(changed, slot)
}

/// Maps "UPDATE matched zero rows" to the port's UserNotFound.
fn require_row(changed: usize, slot: u32) -> Result<(), UserRepositoryError> {
    if changed == 0 {
        return Err(UserRepositoryError::UserNotFound { handle: format!("slot {slot}") });
    }
    Ok(())
}
```

`record_password_change` — single UPDATE
(`password_hash = ?2, password_salt = ?3, password_hash_kind = ?4,
password_last_updated = ?5, force_password_reset = 0`), same
`require_row` check, context `"record password change"`.

`apply_user_patch` — one transaction (the `create_user` shape at
:513-546 is the reference: `let mut conn = self.conn.lock()...;
let tx = conn.transaction()...; ...; tx.commit()`):

1. Users UPDATE (always runs; doubles as the existence check):

```sql
UPDATE users SET
    times_called = times_called + ?2,
    times_called_today = times_called_today + ?3,
    time_used_today_secs = time_used_today_secs + ?4,
    messages_posted = messages_posted + ?5,
    last_call = CASE
        WHEN ?6 IS NULL THEN last_call
        WHEN last_call IS NULL THEN ?6
        ELSE MAX(last_call, ?6) END,
    expert_mode = COALESCE(?7, expert_mode),
    flags = COALESCE(?8, flags),
    last_joined_conference = COALESCE(?9, last_joined_conference),
    last_joined_msgbase = COALESCE(?10, last_joined_msgbase)
WHERE slot_number = ?1
```

   params: `slot, patch.times_called_delta, patch.times_called_today_delta,
   duration_to_secs(patch.time_used_today_delta), patch.messages_posted_delta,
   patch.last_call.map(system_time_to_secs),
   patch.expert_mode.map(i64::from),
   patch.flags.as_ref().map(flags_to_bitmask),
   patch.last_joined.map(|r| r.conference_number()),
   patch.last_joined.map(|r| r.msgbase_number())`.
   `?9`/`?10` are `Some` together or `None` together by construction
   (one `Option<MessageBaseRef>`), so the paired-NULL CHECK holds.

2. Per `MembershipPatch`: when `create_if_missing`,

```sql
INSERT INTO conference_memberships (
    slot_number, conference_number, granted, messages_posted,
    mail_scan, mailscan_all, file_scan, zoom_scan
) VALUES (?1, ?2, ?3, 0, 1, 0, 1, 0)
ON CONFLICT(slot_number, conference_number) DO NOTHING
```

   (granted from `m.granted.unwrap_or(true)`), then always:

```sql
UPDATE conference_memberships SET
    granted = COALESCE(?3, granted),
    messages_posted = messages_posted + ?4,
    mail_scan = COALESCE(?5, mail_scan),
    mailscan_all = COALESCE(?6, mailscan_all),
    file_scan = COALESCE(?7, file_scan),
    zoom_scan = COALESCE(?8, zoom_scan)
WHERE slot_number = ?1 AND conference_number = ?2
```

3. Per `PointerPatch` (after its membership statements — FK order):

```sql
INSERT INTO read_pointers (
    slot_number, conference_number, msgbase_number,
    last_read, last_scanned, new_since
) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(slot_number, conference_number, msgbase_number) DO UPDATE SET
    last_read = MAX(last_read, excluded.last_read),
    last_scanned = MAX(last_scanned, excluded.last_scanned)
```

   (`new_since` intentionally absent from the DO UPDATE — existing rows
   keep theirs.)

4. `tx.commit()`. Every rusqlite error maps via
   `UserRepositoryError::storage("apply user patch", e)`.

Also wrap `insert_seed` (:194-197) in a transaction — same
`conn.transaction()` / `upsert_user(&tx, user)` / `tx.commit()` shape
as `create_user` — closing the last bare multi-statement write that
will survive `save()`'s deletion.

- [ ] **Step 4: Run to verify pass**

Run: `cargo nextest run` — contract parity + rollback pin + full suite.
Expected: PASS.

- [ ] **Step 5: Mutants gate + commit**

Run: `make mutants-diff`. The SQL strings produce no mutants — the
contract sentinels carry that load; Rust-side match arms (daily_sql
selection, require_row) must die to the contract tests.

```bash
git add rust/src/adapters/sqlite_user_repository.rs
git commit -m "Users: transactional SQLite command writes + seed transaction"
```

---

### Task 4: Session persist-baseline

**Files:**
- Modify: `rust/src/domain/session/mod.rs` (field + two methods),
  `rust/src/domain/session/identity.rs` (bind),
  `rust/src/domain/session/registration.rs` (bind),
  `rust/src/domain/session/tests.rs`

**Interfaces:**
- Produces:

```rust
impl Session {
    /// Refreshes the persisted-state baseline to the bound user's
    /// current state. Call after every successful repository write so
    /// [`Session::pending_user_patch`] yields only not-yet-persisted
    /// changes. No-op when no user is bound.
    pub fn rebaseline_persisted(&mut self);

    /// Returns the bound user's slot and the [`UserPatch`] of changes
    /// since the last (re)baseline, or `None` when no user is bound.
    #[must_use]
    pub fn pending_user_patch(&self) -> Option<(u32, UserPatch)>;
}
```

- [ ] **Step 1: Failing domain tests** in `domain/session/tests.rs`:
  - `pending_user_patch_is_none_before_a_user_binds`.
  - `bind_baselines_so_the_first_patch_is_empty`: drive a session to
    `record_identified_user` (existing test helpers show the sequence:
    `prompt_for_name()` then `record_identified_user`), assert
    `pending_user_patch()` returns `Some((slot, patch))` with
    `patch == UserPatch::default()`.
  - `enter_menu_shows_up_as_a_times_called_delta`: continue the session
    through `apply_password_match` + `enter_menu` (crib the state
    sequence from existing lifecycle tests), then assert the pending
    patch has `times_called_delta == 1` — note it will ALSO carry the
    daily-counter changes the auth command owns; that is fine at the
    domain level because `verify_password` rebaselines in Task 5 —
    for THIS test call `rebaseline_persisted()` right after
    `apply_password_match` to mirror the flow.
  - `rebaseline_clears_the_pending_patch`.
  - `finalise_logoff_patch_carries_last_call`: run to `finalise_logoff`,
    assert pending patch `last_call == Some(now)` (the `Ended` phase
    retains the user — `session/mod.rs:204-210` — so the diff works
    after finalise).

- [ ] **Step 2: Run to verify failure** — `cargo nextest run session`,
  expected compile FAIL.

- [ ] **Step 3: Implement.** Field on `Session` (beside
  `activity`/`flagged_files`, `session/mod.rs:127-138`):

```rust
    /// Bound user's state as last persisted: set when a user binds,
    /// refreshed by [`Session::rebaseline_persisted`] after every
    /// repository command-write. [`Session::pending_user_patch`] diffs
    /// the live user against it. Held outside [`SessionPhase`] so it
    /// survives transitions.
    persist_baseline: Option<Box<PersistedUser>>,
```

  Initialise `None` in every `Session` constructor. Methods:

```rust
pub fn rebaseline_persisted(&mut self) {
    self.persist_baseline = self.phase.user().map(|u| Box::new(u.to_persisted()));
}

#[must_use]
pub fn pending_user_patch(&self) -> Option<(u32, UserPatch)> {
    let user = self.phase.user()?;
    debug_assert!(
        self.persist_baseline.is_some(),
        "user bound without a persist baseline"
    );
    let baseline = self.persist_baseline.as_ref()?;
    let current = user.to_persisted();
    Some((current.slot_number, UserPatch::between(baseline, &current)))
}
```

  Bind sites — set the baseline from the exact state the store
  currently holds, BEFORE any rule mutates it:
  - `record_identified_user` (`identity.rs:48-62`): before the phase
    assignment, `self.persist_baseline = Some(Box::new(user.to_persisted()));`
  - `complete_new_user_registration` (`registration.rs:139-160`): same
    line before the `SessionPhase::Onboarded` assignment (the created
    user was just persisted verbatim by `create_user`; the
    `on_enter_onboarded` rules that run next become the enter_menu
    patch, which on a fresh row is all-zero deltas).

- [ ] **Step 4: Run to verify pass** — full `cargo nextest run`.

- [ ] **Step 5: Mutants gate + commit**

```bash
git add rust/src/domain/session/mod.rs rust/src/domain/session/identity.rs rust/src/domain/session/registration.rs rust/src/domain/session/tests.rs
git commit -m "Users: session persist-baseline yields per-call patches"
```

---

### Task 5: Cut the five flow save points over to commands

**Files:**
- Modify: `rust/src/app/session_flow.rs` (sites :311, :318, :376, :402,
  :698; helper `save_bound_user` :407-416 stays until Task 6 only if
  still referenced — it should end this task with zero callers),
  `rust/src/app/session_driver.rs` (SaveFailingRepo gate)

**Interfaces:**
- Consumes: everything above.
- Produces: flows that never call `save()`.

- [ ] **Step 1: Failing test.** In `session_flow.rs`'s `mod tests`, add
  the flow-level lost-update pin (this is the test that CANNOT pass
  under whole-aggregate saves and characterises the slice):

```rust
#[test]
fn a_stale_sessions_logoff_does_not_revert_another_sessions_logon() {
    // Two sessions bind the same account; the second finishes its
    // logon after the first, then the FIRST logs off holding a stale
    // clone. Under whole-aggregate save the stale logoff clobbered
    // the second session's times_called bump.
    // Drive session A: name_typed -> verify_password -> enter_menu.
    // Drive session B: same, binding AFTER A entered the menu.
    // Then finalise A. Assert repo's times_called reflects BOTH
    // logons. (Reuse the module's existing helpers for building
    // sessions/policy/hasher; both sessions share one TestRepo.)
}
```

  Follow the existing verify/enter/finalise test shapes in this module
  (e.g. the tests around :1064-:1112 for the per-site persisted-effect
  pins) for the driving code.

- [ ] **Step 2: Run to verify failure** — the new test fails on the
  final assertion (one logon lost).

- [ ] **Step 3: Implement the cutover.** Two private helpers in
  `session_flow.rs`:

```rust
fn record_auth_and_rebaseline<R>(
    session: &mut Session,
    user_repo: &R,
    outcome: &AuthOutcome,
) -> Result<(), UserRepositoryError>
where
    R: UserRepository + ?Sized,
{
    let slot = session
        .user()
        .expect("verify_password runs with a bound user")
        .slot_number();
    user_repo.record_auth_outcome(slot, outcome)?;
    session.rebaseline_persisted();
    Ok(())
}

fn apply_patch_and_rebaseline<R>(
    session: &mut Session,
    user_repo: &R,
) -> Result<(), UserRepositoryError>
where
    R: UserRepository + ?Sized,
{
    if let Some((slot, patch)) = session.pending_user_patch() {
        user_repo.apply_user_patch(slot, &patch)?;
        session.rebaseline_persisted();
    }
    Ok(())
}
```

  Site edits:
  - `verify_password` match branch (:309-315):

```rust
let (outcome, rejection) = apply_password_match(&mut inner, policy, now)?;
let user = inner.user().expect("password match leaves the user bound");
let daily = (!matches!(outcome, VerifyPasswordOutcome::LogonRejected)).then(|| {
    daily_budget_outcome(user.last_call(), now, policy.daily_reset_offset())
});
let auth = AuthOutcome::Matched {
    daily,
    force_password_reset: user.force_password_reset(),
};
record_auth_and_rebaseline(&mut inner, user_repo, &auth)?;
```

    (`user.last_call()` is still the pre-session value here — it only
    mutates at `finalise_logoff` — so `daily_budget_outcome` sees the
    same inputs `initialise_daily_budget` used inside
    `apply_password_match`. Borrow note: bind
    `daily`/`force_password_reset` from a scoped `inner.user()` borrow
    before calling the helper that takes `&mut inner`.)
  - mismatch branch (:316-320):

```rust
let (outcome, entry) = apply_password_mismatch(&mut inner, policy, now)?;
let auth = AuthOutcome::Mismatched {
    lock_account: matches!(outcome, VerifyPasswordOutcome::AccountLocked),
};
record_auth_and_rebaseline(&mut inner, user_repo, &auth)?;
caller_log.append(entry);
```

  - `enter_menu` (:376) and `finalise_logoff` (:402): replace
    `save_bound_user(&inner, user_repo)?;` with
    `apply_patch_and_rebaseline(&mut inner, user_repo)?;`
  - `complete_password_reset` (:697-698):

```rust
let change = PasswordChange {
    hash: computed.hash.clone(),
    salt: computed.salt.clone(),
    kind,
    changed_at: now,
};
apply_password_change(session, computed.hash, computed.salt, kind, now)?;
let slot = session
    .user()
    .expect("reset flow verified a bound user")
    .slot_number();
user_repo.record_password_change(slot, &change)?;
session.rebaseline_persisted();
```

  - Delete `save_bound_user` (it now has no callers).
  - `SaveFailingRepo` (`session_driver.rs:559-596`): drop its `save`
    override's special role — extract the ordinal gate into
    `fn gate(&self) -> Result<(), UserRepositoryError>` (the
    fetch_add + threshold body of the old `save`), and route
    `record_auth_outcome` / `record_password_change` /
    `apply_user_patch` through `self.gate()?` then delegate to
    `self.inner`. The persist-call ordinals are unchanged (auth = call
    0, menu patch = call 1, finalise patch = next), so the existing
    `fail_from` driver tests (:598-650) keep their meaning. Update the
    doc comment at :554-558 to name the commands instead of `save`.

- [ ] **Step 4: Run to verify pass** — full `cargo nextest run`.
  The pre-existing per-site persistence pins (verify_password persists
  `invalid_attempts`, enter_menu persists `times_called` and defers
  during reset-pending, finalise persists `last_call`) must pass
  UNCHANGED — they are the behaviour-parity oracle for this cutover.
  Expected: PASS. Investigate any flow-test diff as a real regression,
  not a test to update (exception: tests that construct the doubles).

- [ ] **Step 5: Mutants gate + commit**

Run: `make mutants-diff` — the new helpers and branch logic in
`verify_password` are prime mutant targets (e.g. the
`LogonRejected` daily-suppression `!matches!` arm must be killed by the
rejected-path contract/flow tests).

```bash
git add rust/src/app/session_flow.rs rust/src/app/session_driver.rs
git commit -m "Users: session flows persist through command writes"
```

---

### Task 6: Delete `save()` and the DELETE+reinsert path

**Files:**
- Modify: `rust/src/domain/user_repository.rs`,
  `rust/src/adapters/in_memory_user_repository.rs`,
  `rust/src/adapters/sqlite_user_repository.rs`,
  `rust/src/app/session_flow.rs` (TestRepo), any other `.save(`
  reference `grep -rn "\.save(" rust/src rust/tests` finds.

- [ ] **Step 1: Delete trait method `save`** (:145-149) and every impl.
  This is compiler-driven; the failing state IS the build. Also delete
  the in-memory `save` tests (`save_updates_matching_user`,
  `save_unknown_user_errors`) — the contract suite supersedes them.

- [ ] **Step 2: Rework the sqlite save-based tests** rather than losing
  their coverage:
  - Round-trip pins (`save_round_trips_a_full_user_record` :807-861,
    `scan_flags_round_trip_through_sqlite`,
    `save_round_trips_account_locked_and_specific_timestamps`,
    `save_round_trips_last_joined_with_both_coordinates`): replace
    `repo.save(user)` with `repo.insert_seed(&user)` (same
    `upsert_user` path; rename `save_` prefixes to `seed_`). The
    schema round-trip coverage — the point of those tests — survives.
  - `save_unknown_user_returns_user_not_found` (:900-912): delete
    (contract's `unknown_slot_is_user_not_found` covers the port).
  - `save_returns_storage_error_when_lookup_query_fails` (:1011):
    re-target at a command (e.g. `DROP TABLE read_pointers` via a raw
    handle, then `apply_user_patch` → `Storage`), preserving the
    error-boundary coverage.
  - Rewrite the stale comment at :302-305: full-row rebuilds now happen
    only when a row is born (`create_user` / `insert_seed`), both
    transactional; incremental session writes go through the command
    methods, which never delete rows.
- [ ] **Step 3: Update `TestRepo`** in `session_flow.rs` tests: remove
  its `save`; if the compiler shows it is now behaviourally identical
  to `InMemoryUserRepository`, replace the hand-rolled struct with a
  thin wrapper around one (keep the type name so test call sites don't
  churn) — but only if no test reaches into its `users` field directly
  (grep first; if they do, keep the struct and just delete `save`).
- [ ] **Step 4: Full gates**

Run: `cargo nextest run && cargo build && cargo test --doc`
Expected: all green, zero warnings. `grep -rn "\.save(" rust/src rust/tests`
finds no `UserRepository` saves (mail-store saves are a different port —
leave them).

- [ ] **Step 5: Mutants gate + commit**

```bash
git add -A rust/
git commit -m "Users: delete whole-aggregate save; commands are the only user writes"
```

---

### Task 7: Two-session e2e lost-update smoke

**Files:**
- Create: `rust/tests/two_session_logons_smoke.rs`

**Interfaces:**
- Consumes: `tests/support/mod.rs` harness (`TestRuntime::new`,
  `.with_config(|c| c.max_nodes = 2)` — its doc comment anticipates
  exactly this smoke — `spawn_seeded_sysop`, `sign_in_seeded_sysop`,
  `end_session`, `write_line`, `drain_until`) and the S-screen needle
  pinned at `tests/quickwins_smoke.rs:135`
  (`b"\x1b[32m# Times On \x1b[33m:\x1b[0m "`).

- [ ] **Step 1: Write the smoke.** Mirror the fixture block of
  `tests/quickwins_smoke.rs` (conferences + empty mail stores + empty
  file repo + tempdir bbs_path) with `max_nodes = 2`. The interleaving
  that exposes the old bug is: A signs in, B signs in, **B logs off,
  then A logs off** (the stale writer must be last):

```rust
//! Two concurrent sessions on one account must both persist their
//! logons (SYSTEM.md item 1: command-style user writes). Before the
//! command cutover, A's logoff wrote back its stale aggregate and
//! erased B's times_called bump.
mod support;

#[tokio::test]
async fn concurrent_same_account_logons_both_persist() {
    // fixture: as quickwins_smoke.rs, plus .with_config(|c| c.max_nodes = 2)
    let addr = support::spawn_seeded_sysop(fixture).await;

    let mut a = support::sign_in_seeded_sysop(&addr).await; // logon #1
    let mut b = support::sign_in_seeded_sysop(&addr).await; // logon #2
    support::end_session(&mut b).await;
    support::end_session(&mut a).await; // stale clone writes last

    // Logon #3 renders `# Times On : 3` on the S screen. Under the
    // old whole-aggregate save the display read 2.
    let mut c = support::sign_in_seeded_sysop(&addr).await;
    support::write_line(&mut c, b"S").await;
    let capture = support::drain_until(&mut c, b"mins. left): ").await;
    let needle: &[u8] = b"\x1b[32m# Times On \x1b[33m:\x1b[0m 3";
    assert!(
        capture.windows(needle.len()).any(|w| w == needle),
        "expected third logon to show Times On: 3\n{}",
        String::from_utf8_lossy(&capture),
    );
    support::end_session(&mut c).await;
}
```

  Reconcile helper names/visibility against `tests/quickwins_smoke.rs`
  while writing (e.g. whether the sysop fixture needs
  `.with_sysop(...)` to clear scan flags so `sign_in_seeded_sysop`'s
  prompt sequence holds with empty mail stores — copy whatever that
  file's sign-in setup does). Verify the exact rendered needle by
  reading the S-screen renderer's spacing before pinning (check how
  `render_stats_screen` in `session_presenter.rs` formats the value).

- [ ] **Step 2: Run it** — `cargo nextest run two_session`
  Expected: PASS (the fix landed in Task 5). To confirm the test has
  teeth, temporarily `git stash` Tasks 4-5's flow cutover… is not
  practical at this point; instead assert its failure mode was already
  demonstrated by Task 5's flow-level red test. This smoke pins the
  behaviour at the wire, guarding future regressions.

- [ ] **Step 3: Full suite + commit**

```bash
git add rust/tests/two_session_logons_smoke.rs
git commit -m "Users: two-session lost-update smoke"
```

---

### Task 8: Documentation + final sweep

**Files:**
- Modify: `SYSTEM.md`, `designs/USERS.md`

- [ ] **Step 1: SYSTEM.md.** Ordered table row 9 → `**Landed**
  <date>`; detail item 1 gets a "**Landed**" paragraph in the style of
  items 9/10's landed notes, recording: the three port methods (and
  that `record_logon`/`apply_logoff_patch` collapsed into one
  `apply_user_patch` at two call sites — with the rationale), the
  baseline-diff mechanism (`Session::persist_baseline`,
  `pending_user_patch`), `record_password_change` covering the fifth
  save site the sketch missed, `save()` deleted, the tear fixed
  (transactional patch + `insert_seed`), and the dual-adapter contract
  suite + rollback pin + two-session smoke as the verification story.
  Update the "Suggested order" step-3 entry and the "User storage"
  section's description of the save path. Check the composition diagram
  for a `save` mention.
- [ ] **Step 2: designs/USERS.md.** Under "Multiple sessions per user
  are supported", add a landed note: the command-style writes exist as
  of <date> (`record_auth_outcome`, `record_password_change`,
  `apply_user_patch`); the session-end flush queue and per-field
  immediate/deferred split remain future work (deferred until a
  consumer needs them).
- [ ] **Step 3: Final gates, in order:**

```bash
cd rust && cargo nextest run && cargo build && cargo test --doc
cd .. && make mutants-diff DIFF_BASE=<commit-before-task-1>
```

  Expected: green, no warnings, no unjustified surviving mutants across
  the whole slice diff.

- [ ] **Step 4: Commit**

```bash
git add SYSTEM.md designs/USERS.md
git commit -m "Users: document landed command-style user writes (SYSTEM.md item 1)"
```

---

## Self-Review Notes

- **Spec coverage** (SYSTEM.md item 1 + designs/USERS.md): commutative
  command writes ✓ (Tasks 1-3), one SQL transaction per command ✓
  (Task 3; auth/password are single statements), domain yields per-call
  deltas ✓ (Task 4 baseline), all five save sites converted ✓ (Task 5),
  tear fixed ✓ (Task 3 transaction + rollback pin + insert_seed),
  dual-adapter parity tests ✓ (Tasks 2-3), lost-update verification
  without D-T2 ✓ (contract interleaving test, flow-level red test,
  e2e smoke), schema-free ✓ (no DDL change anywhere).
- **Known deviations recorded:** one `apply_user_patch` instead of
  `record_logon`+`apply_logoff_patch`; `last_call` carried by the
  finalise patch (behaviour parity) not `record_logon`;
  `record_password_change` added for save site five.
- **Deliberate exclusions:** no `row_version` (rejected until
  migrations), no session-end flush queue (no consumer), no
  presence/double-logon guard (Tier E), `new_since` merge =
  keep-existing (no production writer), fields with no in-session
  mutator (access_level, censored, ratio, limits, profile strings,
  line_length, ansi_colour) are not patch families — adding one later
  is a schema-free payload extension.
