//! Shared behavioural contract for [`UserRepository`] command writes.
//!
//! Each adapter's test module instantiates every function here with its
//! own factory, so the `SQLite` statements and the domain appliers
//! (`domain::user::commands`) cannot drift apart. SQL string contents
//! are invisible to cargo-mutants, so these sentinel-value assertions
//! carry that verification load: they are what catches a `+` drifting
//! to `=`, a `MAX` drifting to `MIN`, and an absolute write clobbering
//! a concurrent bump.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use crate::domain::conference::{ConferenceMembership, MessageBaseRef, ScanFlag};
use crate::domain::messaging::read_pointers::ReadPointers;
use crate::domain::password::PasswordHashKind;
use crate::domain::user::{
    AuthOutcome, DailyBudgetOutcome, MembershipPatch, PasswordChange, PersistedUser, PointerPatch,
    RatioMode, ScanFlagSettings, User, UserPatch,
};
use crate::domain::user_repository::{NameLookupResult, UserRepository, UserRepositoryError};

const EPOCH: SystemTime = SystemTime::UNIX_EPOCH;

fn at(secs: u64) -> SystemTime {
    EPOCH + Duration::from_secs(secs)
}

/// A user with distinct sentinel counters so an absolute write is
/// distinguishable from an additive one in every assertion.
pub(crate) fn seeded_user(slot: u32, handle: &str) -> User {
    seeded_user_with(slot, handle, |_| {})
}

/// [`seeded_user`] with a post-fixture adjustment (e.g. pre-setting
/// `force_password_reset`).
pub(crate) fn seeded_user_with(
    slot: u32,
    handle: &str,
    tune: impl FnOnce(&mut PersistedUser),
) -> User {
    let mut membership = ConferenceMembership::new(1, true);
    membership.bump_messages_posted();
    membership.bump_messages_posted();
    membership.set_scan_flag(ScanFlag::MailScan, true);
    membership.upsert_pointers(ReadPointers::new(1, 5, 9, at(500)).expect("valid pointers"));
    let mut snapshot = PersistedUser {
        slot_number: slot,
        handle: handle.to_string(),
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
        memberships: vec![membership],
        last_joined: Some(MessageBaseRef::new(1, 1)),
        messages_posted: 3,
    };
    tune(&mut snapshot);
    User::from_persisted(snapshot).expect("valid persisted user")
}

fn stored<R: UserRepository>(repo: &R, handle: &str) -> PersistedUser {
    match repo.find_by_handle(handle).expect("lookup succeeds") {
        NameLookupResult::Found(user) => user.to_persisted(),
        NameLookupResult::NotFound => panic!("user {handle} should exist"),
    }
}

pub(crate) fn mismatch_bumps_additively<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    let outcome = AuthOutcome::Mismatched {
        lock_account: false,
    };
    repo.record_auth_outcome(7, &outcome).expect("first bump");
    repo.record_auth_outcome(7, &outcome).expect("second bump");
    assert_eq!(stored(&repo, "alice").invalid_attempts, 7);
}

pub(crate) fn mismatch_lock_is_one_way<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.record_auth_outcome(7, &AuthOutcome::Mismatched { lock_account: true })
        .expect("locking bump");
    let after_lock = stored(&repo, "alice");
    assert!(after_lock.account_locked);
    assert_eq!(after_lock.invalid_attempts, 6);
    // A later non-locking failure must not unlock.
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Mismatched {
            lock_account: false,
        },
    )
    .expect("plain bump");
    assert!(stored(&repo, "alice").account_locked);
}

pub(crate) fn matched_clears_attempts<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: false,
        },
    )
    .expect("match");
    assert_eq!(stored(&repo, "alice").invalid_attempts, 0);
}

pub(crate) fn matched_new_day_resets_counters<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::NewDay),
            force_password_reset: false,
        },
    )
    .expect("match");
    let after = stored(&repo, "alice");
    assert_eq!(after.times_called_today, 0);
    assert_eq!(after.time_used_today, Duration::ZERO);
}

pub(crate) fn matched_same_day_bumps_today<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: false,
        },
    )
    .expect("match");
    let after = stored(&repo, "alice");
    assert_eq!(after.times_called_today, 8);
    assert_eq!(after.time_used_today, Duration::from_secs(600));
}

pub(crate) fn matched_rejected_path_leaves_daily_counters<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Matched {
            daily: None,
            force_password_reset: false,
        },
    )
    .expect("match");
    let after = stored(&repo, "alice");
    assert_eq!(after.invalid_attempts, 0);
    assert_eq!(after.times_called_today, 7);
    assert_eq!(after.time_used_today, Duration::from_secs(600));
}

pub(crate) fn matched_does_not_unset_force_reset<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user_with(7, "alice", |s| {
        s.force_password_reset = true;
    })]);
    repo.record_auth_outcome(
        7,
        &AuthOutcome::Matched {
            daily: Some(DailyBudgetOutcome::SameDay),
            force_password_reset: false,
        },
    )
    .expect("match");
    assert!(stored(&repo, "alice").force_password_reset);
}

pub(crate) fn password_change_replaces_credentials_and_clears_flag<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user_with(7, "alice", |s| {
        s.force_password_reset = true;
    })]);
    repo.record_password_change(
        7,
        &PasswordChange {
            hash: "new-hash".to_string(),
            salt: Some("new-salt".to_string()),
            kind: PasswordHashKind::Pbkdf210000,
            changed_at: at(42),
        },
    )
    .expect("change");
    let after = stored(&repo, "alice");
    assert_eq!(after.password_hash, "new-hash");
    assert_eq!(after.password_salt, Some("new-salt".to_string()));
    assert_eq!(after.password_last_updated, at(42));
    assert!(!after.force_password_reset);
}

pub(crate) fn patch_counters_are_additive<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    let patch = UserPatch {
        times_called_delta: 1,
        times_called_today_delta: 1,
        time_used_today_delta: Duration::from_secs(60),
        messages_posted_delta: 2,
        ..UserPatch::default()
    };
    repo.apply_user_patch(7, &patch).expect("first patch");
    repo.apply_user_patch(7, &patch).expect("second patch");
    let after = stored(&repo, "alice");
    assert_eq!(after.times_called, 12);
    assert_eq!(after.times_called_today, 9);
    assert_eq!(after.time_used_today, Duration::from_secs(720));
    assert_eq!(after.messages_posted, 7);
}

pub(crate) fn patch_last_call_is_monotonic<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    // Earlier than the stored value: must not move it backwards.
    repo.apply_user_patch(
        7,
        &UserPatch {
            last_call: Some(at(500_000)),
            ..UserPatch::default()
        },
    )
    .expect("stale patch");
    assert_eq!(stored(&repo, "alice").last_call, Some(at(1_000_000)));
    // Later: advances.
    repo.apply_user_patch(
        7,
        &UserPatch {
            last_call: Some(at(2_000_000)),
            ..UserPatch::default()
        },
    )
    .expect("fresh patch");
    assert_eq!(stored(&repo, "alice").last_call, Some(at(2_000_000)));
}

pub(crate) fn patch_pointer_rows_max_merge_and_keep_new_since<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]); // stored row: (5, 9), new_since at(500)
    repo.apply_user_patch(
        7,
        &UserPatch {
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
        },
    )
    .expect("patch");
    let after = stored(&repo, "alice");
    let row = after.memberships[0].pointers_for(1).expect("row exists");
    assert_eq!((row.last_read(), row.last_scanned()), (5, 12));
    assert_eq!(row.new_since(), at(500));
}

pub(crate) fn patch_creates_missing_membership_with_pointer_rows<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.apply_user_patch(
        7,
        &UserPatch {
            memberships: vec![MembershipPatch {
                conference_number: 2,
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
        },
    )
    .expect("patch with new membership");
    let after = stored(&repo, "alice");
    let membership = after
        .memberships
        .iter()
        .find(|m| m.conference_number() == 2)
        .expect("membership created");
    assert!(membership.is_granted());
    assert_eq!(membership.messages_posted(), 1);
    assert!(membership.scan_flag(ScanFlag::MailScan));
    assert!(!membership.scan_flag(ScanFlag::Zoom));
    let row = membership.pointers_for(1).expect("pointer row created");
    assert_eq!((row.last_read(), row.last_scanned()), (0, 2));
    assert_eq!(row.new_since(), at(900));
}

/// `between` always carries `granted`/`scan_flags` for a new
/// membership, but the adapters must agree on the defaults when a
/// hand-built patch omits them: granted, mail-scan and file-scan on,
/// the rest off (the `ConferenceMembership::new` / schema defaults).
pub(crate) fn patch_creates_membership_with_defaults_when_fields_unset<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.apply_user_patch(
        7,
        &UserPatch {
            memberships: vec![MembershipPatch {
                conference_number: 4,
                create_if_missing: true,
                granted: None,
                messages_posted_delta: 0,
                scan_flags: None,
                pointers: Vec::new(),
            }],
            ..UserPatch::default()
        },
    )
    .expect("patch with defaulted new membership");
    let after = stored(&repo, "alice");
    let membership = after
        .memberships
        .iter()
        .find(|m| m.conference_number() == 4)
        .expect("membership created");
    assert!(membership.is_granted());
    assert_eq!(membership.messages_posted(), 0);
    assert!(membership.scan_flag(ScanFlag::MailScan));
    assert!(!membership.scan_flag(ScanFlag::MailScanAll));
    assert!(membership.scan_flag(ScanFlag::FileScan));
    assert!(!membership.scan_flag(ScanFlag::Zoom));
}

pub(crate) fn patch_preferences_are_last_writer_wins<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]);
    repo.apply_user_patch(
        7,
        &UserPatch {
            expert_mode: Some(true),
            last_joined: Some(MessageBaseRef::new(2, 3)),
            ..UserPatch::default()
        },
    )
    .expect("preference patch");
    let after = stored(&repo, "alice");
    assert!(after.expert_mode);
    assert_eq!(after.last_joined, Some(MessageBaseRef::new(2, 3)));
    // A patch with no preference fields leaves them alone.
    repo.apply_user_patch(7, &UserPatch::default())
        .expect("empty patch");
    let unchanged = stored(&repo, "alice");
    assert!(unchanged.expert_mode);
    assert_eq!(unchanged.last_joined, Some(MessageBaseRef::new(2, 3)));
}

/// The headline lost-update pin: two sessions' interleaved commands
/// must both survive. Under a whole-aggregate save this interleaving
/// loses one session's logon.
pub(crate) fn interleaved_sessions_do_not_lose_updates<R: UserRepository>(
    make: impl Fn(Vec<User>) -> R,
) {
    let repo = make(vec![seeded_user(7, "alice")]);
    let auth = AuthOutcome::Matched {
        daily: Some(DailyBudgetOutcome::SameDay),
        force_password_reset: false,
    };
    let logon = UserPatch {
        times_called_delta: 1,
        ..UserPatch::default()
    };
    // Session A authenticates, then session B authenticates, then B
    // finishes its logon before A does.
    repo.record_auth_outcome(7, &auth).expect("A auth");
    repo.record_auth_outcome(7, &auth).expect("B auth");
    repo.apply_user_patch(7, &logon).expect("B logon");
    repo.apply_user_patch(7, &logon).expect("A logon");
    let after = stored(&repo, "alice");
    assert_eq!(after.times_called, 12);
    assert_eq!(after.times_called_today, 9);
}

pub(crate) fn unknown_slot_is_user_not_found<R: UserRepository>(make: impl Fn(Vec<User>) -> R) {
    let repo = make(vec![seeded_user(7, "alice")]);
    let auth_err = repo
        .record_auth_outcome(
            99,
            &AuthOutcome::Mismatched {
                lock_account: false,
            },
        )
        .expect_err("unknown slot");
    assert!(matches!(auth_err, UserRepositoryError::UserNotFound { .. }));
    let change_err = repo
        .record_password_change(
            99,
            &PasswordChange {
                hash: "h".to_string(),
                salt: None,
                kind: PasswordHashKind::Pbkdf210000,
                changed_at: EPOCH,
            },
        )
        .expect_err("unknown slot");
    assert!(matches!(
        change_err,
        UserRepositoryError::UserNotFound { .. }
    ));
    let patch_err = repo
        .apply_user_patch(99, &UserPatch::default())
        .expect_err("unknown slot");
    assert!(matches!(
        patch_err,
        UserRepositoryError::UserNotFound { .. }
    ));
}
