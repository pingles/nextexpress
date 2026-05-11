//! [`scan_mail`] rule (spec: `messaging.allium:ScanMail`).
//!
//! Phase 6, Slice 40. Walks a [`MailStore`] from a caller-supplied or
//! pointer-derived `from_message` up to the store's `highest_message`,
//! counts the messages that are visible-and-unread to `user`, and
//! advances the user's [`ReadPointers`] row so subsequent scans skip
//! what was already surfaced.
//!
//! Wire-level concerns (how the menu surfaces the result, the `M` /
//! `N` parser) live in the application layer; this module is pure
//! domain plus a `MailStore` read.
//!
//! ### `count_unread_for(user, msgbase, from)`, `first_unread_number_for(...)`
//!
//! The spec names two black-box helpers
//! ([`count_unread_for`] / [`first_unread_number_for`]) the [`scan_mail`]
//! rule calls. They are implemented in terms of the same store walk so
//! a future caller (e.g. the per-conference summary the `CS` command
//! will render) can ask the same question without re-running the
//! pointer-advance.
//!
//! ### Unread semantics
//!
//! A message at [`Mail::number`] is *unread for `user`* when:
//! - it is not [`MailVisibility::Deleted`];
//! - [`can_read`](crate::domain::read_mail::can_read) returns `true`;
//! - **and** one of:
//!   - the message is broadcast (`ALL` / `EALL`), or
//!   - the message is addressed to `user` and its `received_at` is
//!     unset.
//!
//! The legacy `searchNewMail` (`amiexpress/express.e:11475`) walks the
//! same combination; this module preserves the wording for parity.
//!
//! [`MailStore`]: crate::domain::mail_store::MailStore
//! [`Mail::number`]: crate::domain::mail::Mail::number

use std::time::SystemTime;

use crate::domain::conference::MessageBaseRef;
use crate::domain::mail::{BroadcastTo, Mail, MailVisibility};
use crate::domain::mail_store::{MailStore, MailStoreError};
use crate::domain::read_mail::can_read;
use crate::domain::read_pointers::ReadPointers;
use crate::domain::user::User;

/// Result of a single-msgbase mail scan
/// (spec: `messaging.allium:MailScanCompleted` event payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanResult {
    /// The 1-indexed starting message number actually scanned (after
    /// resolving `from_message = 0` against `pointers.last_scanned`).
    pub from: u32,
    /// Number of messages visible-and-unread to the user in the
    /// inclusive range `[from, highest_message]`.
    pub unread_count: u32,
    /// Lowest unread number in that range, or `None` when no unread
    /// message was found.
    pub first_unread_number: Option<u32>,
    /// The store's `highest_message` at scan time. The caller can
    /// pass this to subsequent reads without re-querying.
    pub highest_message: u32,
}

/// Errors returned by [`scan_mail`].
#[derive(Debug, thiserror::Error)]
pub enum ScanMailError {
    /// The user has no granted membership for `msgbase`'s parent
    /// conference. The spec's preconditions on `ScanMail` don't name
    /// this gate directly, but lazy-create of a read-pointer row
    /// requires a membership for the parent conference — otherwise
    /// `pointers.last_scanned` would silently fail to persist.
    #[error("user has no membership for conference {0}")]
    NoMembership(u32),
    /// The supplied [`MailStore`]'s [`MailStore::msgbase`] disagrees
    /// with the requested `msgbase`. Catches a wire-up error.
    #[error(
        "mail store is bound to ({store_conf},{store_msg}) but scan was requested for \
         ({req_conf},{req_msg})"
    )]
    StoreMismatch {
        /// Conference number declared by the store.
        store_conf: u32,
        /// Msgbase number declared by the store.
        store_msg: u32,
        /// Conference number requested by the caller.
        req_conf: u32,
        /// Msgbase number requested by the caller.
        req_msg: u32,
    },
    /// The underlying mail store rejected a `load`. Wraps the
    /// originating [`MailStoreError`] so the caller can render the
    /// generic "mail store error" notice without examining variants.
    #[error("mail store load failed during scan: {0}")]
    Store(#[from] MailStoreError),
}

/// Returns the count of unread messages in `[from, store.highest_message]`
/// for `user` (spec: `messaging.allium:count_unread_for`).
///
/// Does *not* mutate the user's read pointers — that's the [`scan_mail`]
/// rule's responsibility. Pure read; callers that just want the
/// summary (e.g. the conference-scan walk) use this helper directly.
///
/// # Errors
/// Returns [`ScanMailError::Store`] when any `load` call fails.
/// Returns [`ScanMailError::StoreMismatch`] when `store` is bound to
/// a different `MessageBaseRef` than the caller's expectations.
pub fn count_unread_for<S>(
    user: &User,
    store: &S,
    msgbase: MessageBaseRef,
    from: u32,
) -> Result<u32, ScanMailError>
where
    S: MailStore + ?Sized,
{
    let summary = walk(user, store, msgbase, from)?;
    Ok(summary.unread_count)
}

/// Returns the lowest unread message number in
/// `[from, store.highest_message]` for `user`, or `None` when none
/// exists (spec: `messaging.allium:first_unread_number_for`).
///
/// # Errors
/// Same as [`count_unread_for`].
pub fn first_unread_number_for<S>(
    user: &User,
    store: &S,
    msgbase: MessageBaseRef,
    from: u32,
) -> Result<Option<u32>, ScanMailError>
where
    S: MailStore + ?Sized,
{
    let summary = walk(user, store, msgbase, from)?;
    Ok(summary.first_unread_number)
}

/// Applies `messaging.allium:ScanMail` to `(user, msgbase)` at `now`.
///
/// Side effects (per the spec's `ensures` block):
/// - emits [`ScanResult`] containing the unread count and the first
///   unread message number (the spec's [`MailScanCompleted`] payload);
/// - advances `pointers.last_scanned` to
///   `max(pointers.last_scanned, store.highest_message)`. The pointer
///   row is lazily created on first scan for a base.
///
/// The `from_message` argument follows the spec: `0` means "start
/// from `pointers.last_scanned + 1`" (the N-command "new mail
/// since" semantics); a positive value starts there (M-command,
/// caller-controlled).
///
/// # Errors
/// Returns [`ScanMailError::NoMembership`] when `user` has no
/// granted membership for `msgbase`'s parent conference (the
/// lazy-create of `ReadPointers` cannot proceed without one),
/// [`ScanMailError::StoreMismatch`] when `store`'s binding
/// disagrees, and [`ScanMailError::Store`] for any underlying
/// `MailStore` failure.
///
/// [`MailScanCompleted`]: https://juxt.github.io/allium/
pub fn scan_mail<S>(
    user: &mut User,
    store: &S,
    msgbase: MessageBaseRef,
    from_message: u32,
    now: SystemTime,
) -> Result<ScanResult, ScanMailError>
where
    S: MailStore + ?Sized,
{
    if store.msgbase() != msgbase {
        let s = store.msgbase();
        return Err(ScanMailError::StoreMismatch {
            store_conf: s.conference_number(),
            store_msg: s.msgbase_number(),
            req_conf: msgbase.conference_number(),
            req_msg: msgbase.msgbase_number(),
        });
    }

    if !user
        .memberships()
        .iter()
        .any(|m| m.conference_number() == msgbase.conference_number())
    {
        return Err(ScanMailError::NoMembership(msgbase.conference_number()));
    }

    let last_scanned = user
        .read_pointers_for(msgbase)
        .map_or(0, ReadPointers::last_scanned);
    let from = if from_message > 0 {
        from_message
    } else {
        last_scanned.saturating_add(1)
    };

    let summary = walk(user, store, msgbase, from)?;

    let target_last_scanned = summary.highest_message.max(last_scanned);
    if let Some(existing) = user.read_pointers_for_mut(msgbase) {
        existing.advance_last_scanned(target_last_scanned);
    } else {
        let mut fresh = ReadPointers::fresh(msgbase.msgbase_number(), now);
        fresh.advance_last_scanned(target_last_scanned);
        let upserted = user.upsert_read_pointers(fresh, msgbase.conference_number());
        debug_assert!(
            upserted,
            "membership existence was checked above; upsert must succeed",
        );
    }

    Ok(summary)
}

/// Internal helper used by [`count_unread_for`], [`first_unread_number_for`]
/// and [`scan_mail`]. Walks `[from, store.highest_message]` inclusive
/// and produces the [`ScanResult`] summary (without mutating
/// `user`).
fn walk<S>(
    user: &User,
    store: &S,
    msgbase: MessageBaseRef,
    from: u32,
) -> Result<ScanResult, ScanMailError>
where
    S: MailStore + ?Sized,
{
    if store.msgbase() != msgbase {
        let s = store.msgbase();
        return Err(ScanMailError::StoreMismatch {
            store_conf: s.conference_number(),
            store_msg: s.msgbase_number(),
            req_conf: msgbase.conference_number(),
            req_msg: msgbase.msgbase_number(),
        });
    }
    let highest = store.highest_message();
    let start = from.max(1);
    let mut count: u32 = 0;
    let mut first: Option<u32> = None;
    // `start..=highest` is empty when `start > highest`, which keeps
    // the inverted-bounds case (the `from_message=10` against a
    // 2-message store) from ever entering the loop body — mirrors a
    // `for` instead of a hand-rolled `while` so a future bounds-flip
    // mutation cannot drive the loop into infinite increment.
    for number in start..=highest {
        if let Some(mail) = store.load(number)? {
            if is_unread_for(user, &mail) {
                count = count.saturating_add(1);
                if first.is_none() {
                    first = Some(number);
                }
            }
        }
    }
    Ok(ScanResult {
        from: start,
        unread_count: count,
        first_unread_number: first,
        highest_message: highest,
    })
}

/// True when `mail` counts as *unread for `user`* under the spec's
/// scan semantics. See the module-level "Unread semantics" doc.
fn is_unread_for(user: &User, mail: &Mail) -> bool {
    if matches!(mail.visibility(), MailVisibility::Deleted) {
        return false;
    }
    if !can_read(user, mail) {
        return false;
    }
    match mail.broadcast_to() {
        BroadcastTo::All | BroadcastTo::Eall => true,
        BroadcastTo::None => {
            mail.addressee_slot() == Some(user.slot_number()) && mail.received_at().is_none()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::conference::ConferenceMembership;
    use crate::domain::mail::{MailDraft, NewMail};
    use crate::domain::password::PasswordHashKind;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn make_user(slot: u32) -> User {
        let mut user = User::new(
            slot,
            format!("user{slot}"),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user");
        user.upsert_membership(ConferenceMembership::new(2, true));
        user
    }

    /// In-memory single-msgbase [`MailStore`] used by these tests.
    /// Implements only what [`scan_mail`] / `walk` exercise — `insert`
    /// is left as `unimplemented!`.
    struct StubStore {
        msgbase: MessageBaseRef,
        mails: Vec<Mail>,
    }

    impl StubStore {
        fn new(msgbase: MessageBaseRef) -> Self {
            Self {
                msgbase,
                mails: Vec::new(),
            }
        }

        fn push(&mut self, draft: MailDraftBuilder) {
            let number = u32::try_from(self.mails.len() + 1).expect("test fixture stays small");
            let mail = Mail::new(NewMail {
                msgbase: self.msgbase,
                number,
                visibility: draft.visibility,
                from_name: "from".to_string(),
                to_name: draft.to_name,
                broadcast_to: draft.broadcast_to,
                subject: "subject".to_string(),
                posted_at: t(0),
                author_slot: draft.author_slot,
                addressee_slot: draft.addressee_slot,
                body: String::new(),
            });
            self.mails.push(mail);
        }

        fn mark_received(&mut self, number: u32, when: SystemTime) {
            let idx = (number - 1) as usize;
            self.mails[idx].mark_received(when).expect("not deleted");
        }
    }

    struct MailDraftBuilder {
        visibility: MailVisibility,
        to_name: String,
        broadcast_to: BroadcastTo,
        author_slot: u32,
        addressee_slot: Option<u32>,
    }

    fn addressed(addressee: u32) -> MailDraftBuilder {
        MailDraftBuilder {
            visibility: MailVisibility::Public,
            to_name: format!("user{addressee}"),
            broadcast_to: BroadcastTo::None,
            author_slot: 1,
            addressee_slot: Some(addressee),
        }
    }

    fn broadcast() -> MailDraftBuilder {
        MailDraftBuilder {
            visibility: MailVisibility::Public,
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            author_slot: 1,
            addressee_slot: None,
        }
    }

    impl MailStore for StubStore {
        fn highest_message(&self) -> u32 {
            u32::try_from(self.mails.len()).expect("test fixture")
        }
        fn msgbase(&self) -> MessageBaseRef {
            self.msgbase
        }
        fn insert(&mut self, _draft: MailDraft) -> Result<Mail, MailStoreError> {
            unimplemented!("scan tests don't post")
        }
        fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError> {
            if number == 0 || number as usize > self.mails.len() {
                return Ok(None);
            }
            Ok(Some(self.mails[(number - 1) as usize].clone()))
        }
        fn save(&mut self, mail: &Mail) -> Result<(), MailStoreError> {
            let idx = (mail.number() - 1) as usize;
            self.mails[idx] = mail.clone();
            Ok(())
        }
    }

    fn ref_2_1() -> MessageBaseRef {
        MessageBaseRef::new(2, 1)
    }

    #[test]
    fn scan_empty_store_reports_zero_unread() {
        let mut user = make_user(2);
        let store = StubStore::new(ref_2_1());
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 0);
        assert_eq!(result.first_unread_number, None);
        assert_eq!(result.highest_message, 0);
        // A zero-message scan must still create the pointer row so
        // subsequent scans don't repeatedly re-walk the same range.
        // (The pointer's last_scanned stays at 0 in this case, since
        // there's nothing to scan past.)
        let p = user.read_pointers_for(ref_2_1()).expect("created");
        assert_eq!(p.last_scanned(), 0);
    }

    #[test]
    fn scan_counts_unread_addressee_mail() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.push(addressed(2));
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 3);
        assert_eq!(result.first_unread_number, Some(1));
        assert_eq!(result.highest_message, 3);
    }

    #[test]
    fn scan_excludes_mail_addressed_to_someone_else() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(3));
        store.push(addressed(4));
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 0, "alice should see no mail");
        assert_eq!(result.first_unread_number, None);
    }

    #[test]
    fn scan_includes_broadcast_mail() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(broadcast());
        store.push(addressed(2));
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 2);
        assert_eq!(result.first_unread_number, Some(1));
    }

    #[test]
    fn scan_excludes_already_received_addressee_mail() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.mark_received(1, t(50));
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 1);
        assert_eq!(result.first_unread_number, Some(2));
    }

    #[test]
    fn scan_advances_last_scanned_to_highest_message() {
        // Spec: `pointers.last_scanned = max_of(pointers.last_scanned, msgbase.highest_message)`.
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.push(addressed(2));
        scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        let p = user.read_pointers_for(ref_2_1()).expect("present");
        assert_eq!(p.last_scanned(), 3);
    }

    #[test]
    fn scan_uses_last_scanned_plus_one_when_from_message_is_zero() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.push(addressed(2));
        // First scan covers messages 1..=3 and leaves last_scanned=3.
        scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        // Add two more messages.
        store.push(addressed(2));
        store.push(addressed(2));
        // Second scan with from_message=0 should resume from 4 (the
        // first message after the cached last_scanned).
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(200)).unwrap();
        assert_eq!(result.from, 4);
        assert_eq!(result.unread_count, 2);
        assert_eq!(result.first_unread_number, Some(4));
        let p = user.read_pointers_for(ref_2_1()).expect("present");
        assert_eq!(p.last_scanned(), 5);
    }

    #[test]
    fn scan_honours_caller_supplied_from_message() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        for _ in 0..5 {
            store.push(addressed(2));
        }
        let result = scan_mail(&mut user, &store, ref_2_1(), 3, t(100)).unwrap();
        assert_eq!(result.from, 3);
        assert_eq!(result.unread_count, 3);
        assert_eq!(result.first_unread_number, Some(3));
    }

    #[test]
    fn scan_with_from_message_beyond_highest_returns_zero_unread() {
        // Boundary: `from > highest` must skip the walk entirely
        // (rather than wrap into an infinite loop). Catches the
        // mutation that flips `number <= highest` to `number > highest`
        // — without this assertion the mutant infinite-loops on any
        // start > highest input.
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        let result = scan_mail(&mut user, &store, ref_2_1(), 10, t(100)).unwrap();
        assert_eq!(result.from, 10);
        assert_eq!(result.unread_count, 0);
        assert_eq!(result.first_unread_number, None);
        assert_eq!(result.highest_message, 2);
    }

    #[test]
    fn scan_pointer_advance_is_monotonic_forward() {
        // A subsequent scan with from_message > 1 must not drag
        // last_scanned backwards.
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        for _ in 0..5 {
            store.push(addressed(2));
        }
        scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        // last_scanned == 5. Re-scan from 1 to 5; pointer must stay at 5.
        scan_mail(&mut user, &store, ref_2_1(), 1, t(200)).unwrap();
        let p = user.read_pointers_for(ref_2_1()).expect("present");
        assert_eq!(p.last_scanned(), 5);
    }

    #[test]
    fn scan_skips_deleted_messages() {
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.mails[0]
            .transition_to(MailVisibility::Deleted)
            .unwrap();
        let result = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        assert_eq!(result.unread_count, 1);
        assert_eq!(result.first_unread_number, Some(2));
    }

    #[test]
    fn scan_rejects_when_user_has_no_membership_for_conference() {
        let mut user = User::new(
            5,
            "stranger".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid");
        let mut store = StubStore::new(ref_2_1());
        store.push(broadcast());
        let err = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).expect_err("no membership");
        assert!(matches!(err, ScanMailError::NoMembership(2)));
    }

    #[test]
    fn scan_rejects_store_bound_to_a_different_msgbase() {
        let mut user = make_user(2);
        let store = StubStore::new(MessageBaseRef::new(9, 4));
        let err = scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).expect_err("store mismatch");
        assert!(matches!(err, ScanMailError::StoreMismatch { .. }));
    }

    #[test]
    fn count_unread_for_matches_scan_result() {
        let user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(broadcast());
        store.push(addressed(3));
        let count = count_unread_for(&user, &store, ref_2_1(), 1).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn first_unread_number_for_returns_none_when_none_match() {
        let user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(3));
        let first = first_unread_number_for(&user, &store, ref_2_1(), 1).unwrap();
        assert_eq!(first, None);
    }

    #[test]
    fn first_unread_number_for_returns_lowest_match() {
        let user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(3));
        store.push(addressed(2));
        store.push(addressed(2));
        let first = first_unread_number_for(&user, &store, ref_2_1(), 1).unwrap();
        assert_eq!(first, Some(2));
    }

    #[test]
    fn count_unread_for_honours_from_filter() {
        let user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        store.push(addressed(2));
        store.push(addressed(2));
        // Skip the first two by passing from=3.
        let count = count_unread_for(&user, &store, ref_2_1(), 3).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn scan_does_not_double_advance_when_last_scanned_already_at_highest() {
        // A scan that finds no new mail still creates (or leaves) the
        // pointer row pinned at highest_message — it does not push it
        // past.
        let mut user = make_user(2);
        let mut store = StubStore::new(ref_2_1());
        store.push(addressed(2));
        scan_mail(&mut user, &store, ref_2_1(), 0, t(100)).unwrap();
        let after_first = user.read_pointers_for(ref_2_1()).unwrap().last_scanned();
        // Idempotent re-scan.
        scan_mail(&mut user, &store, ref_2_1(), 0, t(200)).unwrap();
        let after_second = user.read_pointers_for(ref_2_1()).unwrap().last_scanned();
        assert_eq!(after_first, after_second);
        assert_eq!(after_first, 1);
    }
}
