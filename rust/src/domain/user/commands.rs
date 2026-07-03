//! Command-style user-write payloads and their merge semantics
//! (SYSTEM.md item 1: "Evolve user persistence away from full
//! aggregate saves"; designs/USERS.md "command-style writes").
//!
//! Each type describes one narrow write against the stored user
//! record. The `apply_to` methods on this module's types are the
//! single source of truth for what each command means: the in-memory
//! repository adapter applies commands through them directly, and the
//! `SQLite` adapter implements the same semantics statement-by-statement
//! (pinned to each other by the shared contract-test suite in
//! `adapters::user_repository_contract`).
//!
//! The command shapes keep concurrent same-account sessions
//! commutative where the data allows it: counters are additive deltas,
//! timestamps and read pointers merge monotonically (`MAX`), and only
//! genuinely last-writer-wins preferences overwrite.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use crate::domain::conference::{ConferenceMembership, MessageBaseRef, ScanFlag};
use crate::domain::messaging::read_pointers::ReadPointers;
use crate::domain::password::PasswordHashKind;
use crate::domain::user::{PersistedUser, UserFlag};

/// Which accounting day a logon landed in, relative to the user's
/// previous `last_call` (spec: `session.allium:InitialiseDailyBudget`).
///
/// Computed by
/// [`daily_budget_outcome`](crate::domain::session::daily_budget_outcome)
/// so the persistence command and the in-session budget rule cannot
/// disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyBudgetOutcome {
    /// The logon crossed an accounting-day boundary: the daily
    /// counters reset. Note the legacy quirk this preserves: the reset
    /// does **not** count the current call, so `times_called_today`
    /// lands at `0` (`initialise_daily_budget` resets without
    /// bumping).
    NewDay,
    /// Same accounting day as the previous call: one more call today.
    SameDay,
}

/// Persistent consequences of one password-verification attempt
/// (`session.allium:VerifyPassword` and the post-onboarded rule
/// cluster).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    /// The password matched.
    Matched {
        /// Daily-budget outcome, or `None` when the logon was rejected
        /// (locked account / insufficient access) before the budget
        /// rule ran — in that case the daily counters stay untouched.
        daily: Option<DailyBudgetOutcome>,
        /// Whether the bound user's `force_password_reset` flag is set
        /// after the expiry rule ran. Merges one-way: `true` sets the
        /// stored flag, `false` leaves it alone (this command never
        /// clears a reset another writer requested).
        force_password_reset: bool,
    },
    /// The password did not match.
    Mismatched {
        /// Whether the failure-policy decision locked the account.
        /// Merges one-way: `true` locks, `false` leaves the stored
        /// flag alone.
        lock_account: bool,
    },
}

/// Authoritative credential replacement
/// (`session.allium:CompletePasswordReset`). Overwrites the stored
/// hash/salt/kind triple, stamps `password_last_updated`, and clears
/// `force_password_reset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordChange {
    /// Opaque output of the password hasher.
    pub hash: String,
    /// Salt the hash was bound to (`None` for kinds without one).
    pub salt: Option<String>,
    /// Algorithm used for `hash`.
    pub kind: PasswordHashKind,
    /// When the change happened; becomes `password_last_updated`.
    pub changed_at: SystemTime,
}

/// The per-conference scan-preference flags as one value (the M/A/F/Z
/// columns of the `CF` command), carried whole because the flags are
/// edited as a set and last-writer-wins is acceptable for them.
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors the membership row's four independent flag columns"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanFlagSettings {
    /// `M` — include in the new-mail scan.
    pub mail_scan: bool,
    /// `A` — scan all messages, not just those addressed to the caller.
    pub mailscan_all: bool,
    /// `F` — include in the new-files scan.
    pub file_scan: bool,
    /// `Z` — include in the ZOOM/QWK gather.
    pub zoom_scan: bool,
}

/// One read-pointer row to merge. `last_read` / `last_scanned` merge
/// pairwise with `MAX` against any stored row (which preserves the
/// `last_read <= last_scanned` invariant); `new_since` is used only
/// when the row does not exist yet — an existing row keeps its own
/// `new_since` (the field has no in-session mutator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerPatch {
    /// Message base the row belongs to (conference comes from the
    /// enclosing [`MembershipPatch`]).
    pub msgbase_number: u32,
    /// Highest message number the user has read.
    pub last_read: u32,
    /// Highest message number the mail scan has covered.
    pub last_scanned: u32,
    /// Row-creation timestamp, applied only to a newly created row.
    pub new_since: SystemTime,
}

/// Changes to one conference-membership row.
///
/// Patches produced by [`UserPatch::between`] never carry pointer rows
/// for a membership that is absent from storage unless
/// `create_if_missing` is set; a hand-built patch that does so fails
/// (and rolls back) in the `SQLite` adapter's foreign-key check and is
/// skipped by the pure-Rust applier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipPatch {
    /// Conference the row belongs to.
    pub conference_number: u32,
    /// Create the membership row first when it does not exist
    /// (granted from [`Self::granted`], other columns at their schema
    /// defaults) — the session joined a conference it had no row for.
    pub create_if_missing: bool,
    /// New `granted` value, set only when it changed.
    pub granted: Option<bool>,
    /// Messages posted in this conference during the diff window.
    pub messages_posted_delta: u32,
    /// New scan-flag values, set only when any flag changed
    /// (last-writer-wins as a set).
    pub scan_flags: Option<ScanFlagSettings>,
    /// Pointer rows that are new or advanced during the diff window.
    pub pointers: Vec<PointerPatch>,
}

/// A delta/patch write covering the session-mutable state families:
/// call/usage counters, read pointers, scan flags, conference
/// position, display preferences, and `last_call`.
///
/// Produced by [`UserPatch::between`] from a baseline snapshot and the
/// live user, applied at `enter_menu` and `finalise_logoff`.
/// Credential changes are deliberately not part of the patch — they go
/// through [`PasswordChange`] — and fields with no in-session mutator
/// (access level, censor flag, ratio policy, time limits, profile
/// strings, line length, ANSI preference) are not patch families;
/// adding one later is a payload extension, not a schema change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserPatch {
    /// Completed logons to add (additive).
    pub times_called_delta: u32,
    /// Logons-today to add (additive).
    pub times_called_today_delta: u32,
    /// Time used today to add (additive).
    pub time_used_today_delta: Duration,
    /// Cross-conference messages posted to add (additive).
    pub messages_posted_delta: u32,
    /// New `last_call`; merges with `MAX` against the stored value so
    /// an older session finishing late cannot move it backwards.
    pub last_call: Option<SystemTime>,
    /// New expert-mode preference, set only when it changed
    /// (last-writer-wins).
    pub expert_mode: Option<bool>,
    /// New preference-flag set, set only when it changed
    /// (last-writer-wins as a set).
    pub flags: Option<BTreeSet<UserFlag>>,
    /// New last-joined `(conference, msgbase)` pair, set only when it
    /// changed (last-writer-wins; the pair always travels together).
    pub last_joined: Option<MessageBaseRef>,
    /// Per-conference membership changes.
    pub memberships: Vec<MembershipPatch>,
}

impl AuthOutcome {
    /// Applies this command's merge semantics to a stored-user
    /// snapshot. This is the reference implementation the adapters are
    /// pinned to.
    pub fn apply_to(&self, snapshot: &mut PersistedUser) {
        match self {
            Self::Matched {
                daily,
                force_password_reset,
            } => {
                snapshot.invalid_attempts = 0;
                if *force_password_reset {
                    snapshot.force_password_reset = true;
                }
                match daily {
                    Some(DailyBudgetOutcome::NewDay) => {
                        snapshot.times_called_today = 0;
                        snapshot.time_used_today = Duration::ZERO;
                    }
                    Some(DailyBudgetOutcome::SameDay) => {
                        snapshot.times_called_today = snapshot.times_called_today.saturating_add(1);
                    }
                    None => {}
                }
            }
            Self::Mismatched { lock_account } => {
                snapshot.invalid_attempts = snapshot.invalid_attempts.saturating_add(1);
                if *lock_account {
                    snapshot.account_locked = true;
                }
            }
        }
    }
}

impl PasswordChange {
    /// Applies this command's merge semantics to a stored-user
    /// snapshot. This is the reference implementation the adapters are
    /// pinned to.
    pub fn apply_to(&self, snapshot: &mut PersistedUser) {
        snapshot.password_hash.clone_from(&self.hash);
        snapshot.password_salt.clone_from(&self.salt);
        snapshot.password_hash_kind = self.kind;
        snapshot.password_last_updated = self.changed_at;
        snapshot.force_password_reset = false;
    }
}

impl UserPatch {
    /// Computes the patch that carries `current`'s changes since
    /// `baseline` — the not-yet-persisted mutations of one session
    /// window.
    ///
    /// # Parameters
    /// - `baseline`: the user's state as last persisted.
    /// - `current`: the live in-session state.
    ///
    /// # Returns
    /// A patch such that applying it to `baseline` reproduces
    /// `current` for every supported state family. Memberships present
    /// in `baseline` but missing from `current` are ignored
    /// (memberships are never removed mid-session).
    #[must_use]
    pub fn between(baseline: &PersistedUser, current: &PersistedUser) -> UserPatch {
        let memberships = current
            .memberships
            .iter()
            .filter_map(|cur| {
                let base = baseline
                    .memberships
                    .iter()
                    .find(|b| b.conference_number() == cur.conference_number());
                membership_patch_between(base, cur)
            })
            .collect();
        UserPatch {
            times_called_delta: current.times_called.saturating_sub(baseline.times_called),
            times_called_today_delta: current
                .times_called_today
                .saturating_sub(baseline.times_called_today),
            time_used_today_delta: current
                .time_used_today
                .saturating_sub(baseline.time_used_today),
            messages_posted_delta: current
                .messages_posted
                .saturating_sub(baseline.messages_posted),
            last_call: changed(&baseline.last_call, current.last_call).flatten(),
            expert_mode: changed(&baseline.expert_mode, current.expert_mode),
            flags: (current.flags != baseline.flags).then(|| current.flags.clone()),
            last_joined: changed(&baseline.last_joined, current.last_joined).flatten(),
            memberships,
        }
    }

    /// Applies this patch's merge semantics to a stored-user snapshot.
    /// This is the reference implementation the adapters are pinned
    /// to.
    pub fn apply_to(&self, snapshot: &mut PersistedUser) {
        snapshot.times_called = snapshot
            .times_called
            .saturating_add(self.times_called_delta);
        snapshot.times_called_today = snapshot
            .times_called_today
            .saturating_add(self.times_called_today_delta);
        snapshot.time_used_today = snapshot
            .time_used_today
            .saturating_add(self.time_used_today_delta);
        snapshot.messages_posted = snapshot
            .messages_posted
            .saturating_add(self.messages_posted_delta);
        if let Some(at) = self.last_call {
            snapshot.last_call = Some(snapshot.last_call.map_or(at, |existing| existing.max(at)));
        }
        if let Some(expert) = self.expert_mode {
            snapshot.expert_mode = expert;
        }
        if let Some(flags) = &self.flags {
            snapshot.flags.clone_from(flags);
        }
        if let Some(joined) = self.last_joined {
            snapshot.last_joined = Some(joined);
        }
        for patch in &self.memberships {
            patch.apply_to(&mut snapshot.memberships);
        }
    }
}

/// `Some(current)` when the value changed from `baseline`, else `None`
/// — the "set iff changed" convention every last-writer-wins patch
/// field uses.
fn changed<T: PartialEq>(baseline: &T, current: T) -> Option<T> {
    (current != *baseline).then_some(current)
}

/// Reads a membership's scan flags as one [`ScanFlagSettings`] value.
fn scan_flag_settings(membership: &ConferenceMembership) -> ScanFlagSettings {
    ScanFlagSettings {
        mail_scan: membership.scan_flag(ScanFlag::MailScan),
        mailscan_all: membership.scan_flag(ScanFlag::MailScanAll),
        file_scan: membership.scan_flag(ScanFlag::FileScan),
        zoom_scan: membership.scan_flag(ScanFlag::Zoom),
    }
}

/// Diffs one conference membership; `None` when nothing changed.
fn membership_patch_between(
    base: Option<&ConferenceMembership>,
    cur: &ConferenceMembership,
) -> Option<MembershipPatch> {
    let Some(base) = base else {
        // The session joined a conference it had no row for: carry the
        // full row.
        return Some(MembershipPatch {
            conference_number: cur.conference_number(),
            create_if_missing: true,
            granted: Some(cur.is_granted()),
            messages_posted_delta: cur.messages_posted(),
            scan_flags: Some(scan_flag_settings(cur)),
            pointers: cur.pointers().iter().map(pointer_patch).collect(),
        });
    };
    let granted = changed(&base.is_granted(), cur.is_granted());
    let scan_flags = changed(&scan_flag_settings(base), scan_flag_settings(cur));
    let messages_posted_delta = cur.messages_posted().saturating_sub(base.messages_posted());
    let pointers: Vec<PointerPatch> = cur
        .pointers()
        .iter()
        .filter(|p| {
            base.pointers_for(p.msgbase_number()).is_none_or(|bp| {
                p.last_read() > bp.last_read() || p.last_scanned() > bp.last_scanned()
            })
        })
        .map(pointer_patch)
        .collect();
    (granted.is_some() || scan_flags.is_some() || messages_posted_delta > 0 || !pointers.is_empty())
        .then_some(MembershipPatch {
            conference_number: cur.conference_number(),
            create_if_missing: false,
            granted,
            messages_posted_delta,
            scan_flags,
            pointers,
        })
}

fn pointer_patch(row: &ReadPointers) -> PointerPatch {
    PointerPatch {
        msgbase_number: row.msgbase_number(),
        last_read: row.last_read(),
        last_scanned: row.last_scanned(),
        new_since: row.new_since(),
    }
}

impl MembershipPatch {
    /// Applies this membership patch against the snapshot's membership
    /// list — the pure-Rust mirror of the `SQLite` adapter's
    /// per-membership statements.
    fn apply_to(&self, memberships: &mut Vec<ConferenceMembership>) {
        let position = memberships
            .iter()
            .position(|m| m.conference_number() == self.conference_number);
        let membership = if let Some(index) = position {
            &mut memberships[index]
        } else {
            if !self.create_if_missing {
                // A hand-built patch referencing a missing row: the
                // SQL adapter rejects it (FK); the pure applier skips
                // it. `between` never produces one.
                return;
            }
            memberships.push(ConferenceMembership::new(
                self.conference_number,
                self.granted.unwrap_or(true),
            ));
            memberships.last_mut().expect("row was just pushed")
        };
        if let Some(granted) = self.granted {
            membership.set_granted(granted);
        }
        if let Some(flags) = self.scan_flags {
            membership.set_scan_flag(ScanFlag::MailScan, flags.mail_scan);
            membership.set_scan_flag(ScanFlag::MailScanAll, flags.mailscan_all);
            membership.set_scan_flag(ScanFlag::FileScan, flags.file_scan);
            membership.set_scan_flag(ScanFlag::Zoom, flags.zoom_scan);
        }
        for _ in 0..self.messages_posted_delta {
            membership.bump_messages_posted();
        }
        for patch in &self.pointers {
            let merged = match membership.pointers_for(patch.msgbase_number) {
                Some(existing) => ReadPointers::new(
                    patch.msgbase_number,
                    existing.last_read().max(patch.last_read),
                    existing.last_scanned().max(patch.last_scanned),
                    existing.new_since(),
                ),
                None => ReadPointers::new(
                    patch.msgbase_number,
                    patch.last_read,
                    patch.last_scanned,
                    patch.new_since,
                ),
            }
            .expect("pairwise MAX of valid pointer rows keeps last_read <= last_scanned");
            membership.upsert_pointers(merged);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::RatioMode;

    const EPOCH: SystemTime = SystemTime::UNIX_EPOCH;

    fn at(secs: u64) -> SystemTime {
        EPOCH + Duration::from_secs(secs)
    }

    fn membership_fixture() -> ConferenceMembership {
        let mut m = ConferenceMembership::new(1, true);
        m.bump_messages_posted();
        m.bump_messages_posted();
        m.set_scan_flag(ScanFlag::MailScan, true);
        m.upsert_pointers(ReadPointers::new(1, 5, 9, at(500)).expect("valid pointers"));
        m
    }

    fn snapshot() -> PersistedUser {
        PersistedUser {
            slot_number: 7,
            handle: "alice".to_string(),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_last_updated: EPOCH,
            force_password_reset: false,
            access_level: 100,
            invalid_attempts: 5,
            account_locked: false,
            is_new_user: false,
            censored: false,
            times_called: 10,
            times_called_today: 7,
            last_call: Some(at(1_000_000)),
            time_limit_per_call: Duration::from_secs(1800),
            time_limit_per_day: Duration::from_secs(3600),
            time_used_today: Duration::from_secs(600),
            location: None,
            phone_number: None,
            email: None,
            line_length: 0,
            ansi_colour: true,
            expert_mode: false,
            account_created: EPOCH,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::Disabled,
            ratio_value: 0,
            memberships: vec![membership_fixture()],
            last_joined: Some(MessageBaseRef::new(1, 1)),
            messages_posted: 3,
        }
    }

    #[test]
    fn between_then_apply_reproduces_current() {
        let baseline = snapshot();
        let mut current = snapshot();
        current.times_called += 1;
        current.times_called_today += 1;
        current.time_used_today += Duration::from_secs(120);
        current.messages_posted += 2;
        current.last_call = Some(at(2_000_000));
        current.expert_mode = true;
        current.flags.insert(UserFlag::EditorPrompts);
        current.last_joined = Some(MessageBaseRef::new(2, 1));
        {
            let m = &mut current.memberships[0];
            m.bump_messages_posted();
            m.set_scan_flag(ScanFlag::Zoom, true);
            // Advanced row keeps its original new_since.
            m.upsert_pointers(ReadPointers::new(1, 8, 12, at(500)).expect("valid"));
            // Brand-new row in an existing membership.
            m.upsert_pointers(ReadPointers::new(2, 1, 1, at(900)).expect("valid"));
        }
        let mut joined = ConferenceMembership::new(2, true);
        joined.upsert_pointers(ReadPointers::new(1, 0, 3, at(950)).expect("valid"));
        current.memberships.push(joined);

        let patch = UserPatch::between(&baseline, &current);
        let mut rebuilt = snapshot();
        patch.apply_to(&mut rebuilt);
        assert_eq!(rebuilt, current);
    }

    /// Round-trips a `current` that differs from the baseline in
    /// exactly one way. Each single-family case isolates one condition
    /// of `membership_patch_between`'s include-gate and pointer
    /// predicate, so a mutated comparison or connective cannot hide
    /// behind another changed family (the way a kitchen-sink round
    /// trip lets it).
    fn assert_lone_change_round_trips(mutate: impl Fn(&mut PersistedUser)) {
        let baseline = snapshot();
        let mut current = snapshot();
        mutate(&mut current);
        let patch = UserPatch::between(&baseline, &current);
        let mut rebuilt = snapshot();
        patch.apply_to(&mut rebuilt);
        assert_eq!(rebuilt, current);
    }

    #[test]
    fn between_detects_a_lone_granted_change() {
        assert_lone_change_round_trips(|current| {
            current.memberships[0].set_granted(false);
        });
    }

    #[test]
    fn between_detects_a_lone_scan_flag_change() {
        assert_lone_change_round_trips(|current| {
            current.memberships[0].set_scan_flag(ScanFlag::Zoom, true);
        });
    }

    #[test]
    fn between_detects_a_lone_posted_bump() {
        assert_lone_change_round_trips(|current| {
            current.memberships[0].bump_messages_posted();
        });
    }

    #[test]
    fn between_detects_a_lone_last_read_advance() {
        assert_lone_change_round_trips(|current| {
            current.memberships[0]
                .upsert_pointers(ReadPointers::new(1, 7, 9, at(500)).expect("valid"));
        });
    }

    #[test]
    fn between_detects_a_lone_last_scanned_advance() {
        assert_lone_change_round_trips(|current| {
            current.memberships[0]
                .upsert_pointers(ReadPointers::new(1, 5, 12, at(500)).expect("valid"));
        });
    }

    #[test]
    fn between_of_identical_snapshots_is_default_patch() {
        assert_eq!(
            UserPatch::between(&snapshot(), &snapshot()),
            UserPatch::default()
        );
    }

    #[test]
    fn patch_last_call_max_keeps_later_stored_value() {
        let mut snap = snapshot();
        let patch = UserPatch {
            last_call: Some(at(500_000)),
            ..UserPatch::default()
        };
        patch.apply_to(&mut snap);
        assert_eq!(snap.last_call, Some(at(1_000_000)));
    }

    #[test]
    fn patch_last_call_fills_a_never_called_user() {
        let mut snap = snapshot();
        snap.last_call = None;
        let patch = UserPatch {
            last_call: Some(at(500_000)),
            ..UserPatch::default()
        };
        patch.apply_to(&mut snap);
        assert_eq!(snap.last_call, Some(at(500_000)));
    }

    #[test]
    fn pointer_merge_is_pairwise_max_and_keeps_existing_new_since() {
        let mut snap = snapshot(); // stored row: (5, 9), new_since at(500)
        let patch = UserPatch {
            memberships: vec![MembershipPatch {
                conference_number: 1,
                create_if_missing: false,
                granted: None,
                messages_posted_delta: 0,
                scan_flags: None,
                pointers: vec![PointerPatch {
                    msgbase_number: 1,
                    last_read: 3,
                    last_scanned: 12,
                    new_since: EPOCH,
                }],
            }],
            ..UserPatch::default()
        };
        patch.apply_to(&mut snap);
        let row = snap.memberships[0].pointers_for(1).expect("row exists");
        assert_eq!((row.last_read(), row.last_scanned()), (5, 12));
        assert_eq!(row.new_since(), at(500));
    }

    #[test]
    fn patch_creates_missing_membership_with_pointer_rows() {
        let mut snap = snapshot();
        let patch = UserPatch {
            memberships: vec![MembershipPatch {
                conference_number: 3,
                create_if_missing: true,
                granted: Some(true),
                messages_posted_delta: 1,
                scan_flags: Some(ScanFlagSettings {
                    mail_scan: true,
                    mailscan_all: false,
                    file_scan: true,
                    zoom_scan: false,
                }),
                pointers: vec![PointerPatch {
                    msgbase_number: 1,
                    last_read: 0,
                    last_scanned: 2,
                    new_since: at(900),
                }],
            }],
            ..UserPatch::default()
        };
        patch.apply_to(&mut snap);
        let m = snap
            .memberships
            .iter()
            .find(|m| m.conference_number() == 3)
            .expect("membership created");
        assert!(m.is_granted());
        assert_eq!(m.messages_posted(), 1);
        let row = m.pointers_for(1).expect("pointer row created");
        assert_eq!((row.last_read(), row.last_scanned()), (0, 2));
        assert_eq!(row.new_since(), at(900));
    }

    #[test]
    fn auth_mismatch_is_additive_and_lock_is_one_way() {
        let mut snap = snapshot();
        AuthOutcome::Mismatched {
            lock_account: false,
        }
        .apply_to(&mut snap);
        assert_eq!(snap.invalid_attempts, 6);
        assert!(!snap.account_locked);

        AuthOutcome::Mismatched { lock_account: true }.apply_to(&mut snap);
        assert_eq!(snap.invalid_attempts, 7);
        assert!(snap.account_locked);
    }

    #[test]
    fn auth_matched_new_day_resets_but_does_not_bump() {
        let mut snap = snapshot();
        AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::NewDay),
            force_password_reset: false,
        }
        .apply_to(&mut snap);
        assert_eq!(snap.invalid_attempts, 0);
        assert_eq!(snap.times_called_today, 0);
        assert_eq!(snap.time_used_today, Duration::ZERO);
    }

    #[test]
    fn auth_matched_same_day_bumps_today_counter() {
        let mut snap = snapshot();
        AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: false,
        }
        .apply_to(&mut snap);
        assert_eq!(snap.times_called_today, 8);
        assert_eq!(snap.time_used_today, Duration::from_secs(600));
    }

    #[test]
    fn auth_matched_rejected_path_leaves_daily_counters() {
        let mut snap = snapshot();
        AuthOutcome::Matched {
            daily: None,
            force_password_reset: false,
        }
        .apply_to(&mut snap);
        assert_eq!(snap.invalid_attempts, 0);
        assert_eq!(snap.times_called_today, 7);
        assert_eq!(snap.time_used_today, Duration::from_secs(600));
    }

    #[test]
    fn auth_matched_never_clears_force_reset() {
        let mut snap = snapshot();
        snap.force_password_reset = true;
        AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: false,
        }
        .apply_to(&mut snap);
        assert!(snap.force_password_reset);
    }

    #[test]
    fn auth_matched_sets_force_reset_when_due() {
        let mut snap = snapshot();
        AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: true,
        }
        .apply_to(&mut snap);
        assert!(snap.force_password_reset);
    }

    #[test]
    fn password_change_replaces_credentials_and_clears_flag() {
        let mut snap = snapshot();
        snap.force_password_reset = true;
        PasswordChange {
            hash: "new-hash".to_string(),
            salt: Some("new-salt".to_string()),
            kind: PasswordHashKind::Pbkdf210000,
            changed_at: at(42),
        }
        .apply_to(&mut snap);
        assert_eq!(snap.password_hash, "new-hash");
        assert_eq!(snap.password_salt, Some("new-salt".to_string()));
        assert_eq!(snap.password_last_updated, at(42));
        assert!(!snap.force_password_reset);
    }
}
