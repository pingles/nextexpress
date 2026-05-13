//! [`post_mail`] rule (spec: `messaging.allium:PostMail`).
//!
//! Phase 7, Slice 42. A domain function that turns a caller-resolved
//! draft into a persisted [`Mail`], advances the message base's
//! high-water mark via the [`MailStore`] port, and bumps both
//! per-user and per-membership `messages_posted` counters.
//!
//! Slice 42 only models the single-addressee path: `broadcast_to =
//! none`, no censored-user routing, no ALL / EALL fan-out. Those land
//! in later slices (43, 47). The caller — the menu's `E` handler —
//! resolves the addressee through the user repository and the
//! conference's `accepted_name_type` *before* invoking this rule, so
//! the rule itself stays a pure domain function over already-resolved
//! data.
//!
//! Wire-level concerns (the line-mode editor that gathers
//! to/subject/body, locking the [`MailStore`], rendering the post
//! confirmation) live in the application layer.

use std::time::SystemTime;

use crate::domain::conference::{AllowedAddressing, MessageBaseRef};
use crate::domain::messaging::mail::{
    addressing_allows, BroadcastTo, Mail, MailDraft, MailVisibility,
};
use crate::domain::messaging::mail_store::{MailStore, MailStoreError};
use crate::domain::user::{Right, User};

/// Caller-resolved fields for a post (spec:
/// `messaging.allium:PostMail` consequent fields).
///
/// `to_name` and `from_name` are the display strings the caller has
/// already resolved via the spec's `display_name_of(_, conference)`
/// black box — the rule does not consult the conference catalogue.
///
/// `addressee_slot` is the spec's `resolved_addressee` — the user
/// repository looked the typed handle up before the rule fired — and
/// is `None` exactly when `broadcast_to` is ALL or EALL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostMailDraft {
    /// Display name written to `Mail.to_name` (`"ALL"` / `"EALL"` for
    /// broadcasts; the caller has already case-normalised these).
    pub to_name: String,
    /// Broadcast discriminator on `to_name` (Slice 43). `None` for the
    /// single-addressee path, `All` / `Eall` for broadcasts.
    pub broadcast_to: BroadcastTo,
    /// Resolved addressee's stable slot number
    /// (spec: `resolved_addressee != null`). Must be `Some(_)` when
    /// [`Self::broadcast_to`] is `None` and `None` for broadcasts.
    pub addressee_slot: Option<u32>,
    /// Author's display name in the current conference
    /// (spec: `Mail.from_name`, invariant `FromNameMatchesAuthor`).
    pub from_name: String,
    /// Free-text subject (spec: `Mail.subject`).
    pub subject: String,
    /// Free-text body (spec: `Mail.body`).
    pub body: String,
    /// User answered "yes" at the legacy `Private (y/n)?` prompt.
    ///
    /// Spec: visibility selector
    /// (`if draft.broadcast_to = eall: public else if user.censored:
    /// private_to_sysop else if draft.private: private else: public`).
    /// The censored branch is deferred to Slice 47 — for non-EALL
    /// drafts Slice 43 honours `private` directly; EALL forces public
    /// regardless of this flag.
    pub private: bool,
    /// `now` recorded as `Mail.posted_at` (spec: `posted_at: now`).
    pub posted_at: SystemTime,
}

/// Errors raised by [`post_mail`]. Each variant maps to one of
/// `messaging.allium:PostMail`'s `requires` clauses (the others are
/// enforced by the caller's typed session: `state = menu`,
/// `user != null`, `visit != null`, and `lock_msgbase` are all
/// upstream gates).
#[derive(Debug, thiserror::Error)]
pub enum PostMailError {
    /// The user lacks `has_access(user, enter_message)`. Mirrors the
    /// spec's `requires: has_access(session.user, enter_message)`.
    #[error("user lacks the enter_message right")]
    AccessDenied,
    /// The user has no granted [`crate::domain::conference::ConferenceMembership`]
    /// for the message base's parent conference. The spec models this
    /// via the `current_visit(session) != null` precondition; we
    /// surface it explicitly so a caller that constructs a draft
    /// outside the session-driven path still gets a clean error
    /// instead of silently dropping the per-conference
    /// `messages_posted` bump.
    #[error("user has no membership for the message base's conference")]
    NoMembership,
    /// The supplied draft has an empty `to_name` and is not a
    /// broadcast. The legacy code's empty-to-ALL reroute is the
    /// caller's responsibility (the menu's `E` handler); this rule
    /// refuses to silently persist a no-recipient message.
    #[error("recipient name is empty")]
    EmptyAddressee,
    /// The supplied draft addresses ALL or EALL but the message base's
    /// [`AllowedAddressing`] policy forbids that broadcast kind. Spec:
    /// `requires: addressing_allows(visit.msgbase, broadcast)`.
    #[error("message base does not accept this addressing kind")]
    AddressingNotAllowed,
    /// The draft's `broadcast_to` and `addressee_slot` disagree.
    /// `BroadcastTo::None` requires `addressee_slot = Some(_)`; `All`
    /// and `Eall` require `addressee_slot = None`.
    #[error("broadcast_to and addressee_slot are inconsistent")]
    AddresseeMismatch,
    /// The underlying [`MailStore`] rejected the insert.
    #[error("mail store rejected insert: {0}")]
    Store(#[from] MailStoreError),
}

/// Applies `messaging.allium:PostMail` (Slice 42, single-addressee
/// path) to `(user, msgbase, store, draft)`.
///
/// On success:
/// - The store has persisted the new [`Mail`] under
///   `store.highest_message() + 1`;
/// - `user.messages_posted` and the membership for `msgbase.conference`
///   have each been incremented by one;
/// - The returned [`Mail`] is the freshly-inserted record (the same
///   value the store wrote).
///
/// On failure no state is mutated — neither the user, the store,
/// nor the bound membership.
///
/// # Errors
/// Returns the matching [`PostMailError`] variant when a `requires`
/// gate fails or the store rejects the write.
///
/// # Panics
/// Panics if the membership-lookup branch runs after the granted
/// membership check fails to find a row — this is unreachable because
/// the membership existence is verified earlier in the same function;
/// the panic guards against a future refactor breaking that
/// invariant.
pub fn post_mail(
    user: &mut User,
    msgbase: MessageBaseRef,
    allowed_addressing: AllowedAddressing,
    store: &mut dyn MailStore,
    draft: PostMailDraft,
) -> Result<Mail, PostMailError> {
    if !user.has_access(Right::EnterMessage) {
        return Err(PostMailError::AccessDenied);
    }
    apply_post_mail(user, msgbase, allowed_addressing, store, draft)
}

/// Shared post-mail body used by [`post_mail`] and the
/// `PostCommentToSysop` rule (Slice 44). Performs every gate other than
/// the per-rule access check, then persists the message and bumps the
/// counters. The caller is responsible for verifying that the user has
/// the appropriate [`Right`] (e.g. [`Right::EnterMessage`] or
/// [`Right::CommentToSysop`]).
///
/// # Errors
/// Same as [`post_mail`] modulo the access-check variant the caller
/// fired before invoking this helper.
pub(crate) fn apply_post_mail(
    user: &mut User,
    msgbase: MessageBaseRef,
    allowed_addressing: AllowedAddressing,
    store: &mut dyn MailStore,
    draft: PostMailDraft,
) -> Result<Mail, PostMailError> {
    if draft.to_name.is_empty() {
        return Err(PostMailError::EmptyAddressee);
    }
    match (draft.broadcast_to, draft.addressee_slot) {
        (BroadcastTo::None, Some(_)) | (BroadcastTo::All | BroadcastTo::Eall, None) => {}
        _ => return Err(PostMailError::AddresseeMismatch),
    }
    if !addressing_allows(allowed_addressing, draft.broadcast_to) {
        return Err(PostMailError::AddressingNotAllowed);
    }
    if !user.has_granted_membership_for(msgbase.conference_number()) {
        return Err(PostMailError::NoMembership);
    }

    // Spec visibility selector:
    //   if draft.broadcast_to = eall: public
    //   else if user.censored: private_to_sysop   (Slice 47)
    //   else if draft.private: private
    //   else: public
    let visibility = if matches!(draft.broadcast_to, BroadcastTo::Eall) {
        MailVisibility::Public
    } else if draft.private {
        MailVisibility::Private
    } else {
        MailVisibility::Public
    };

    let PostMailDraft {
        to_name,
        broadcast_to,
        addressee_slot,
        from_name,
        subject,
        body,
        private: _,
        posted_at,
    } = draft;

    let mail_draft = MailDraft {
        visibility,
        from_name,
        to_name,
        broadcast_to,
        subject,
        posted_at,
        author_slot: user.slot_number(),
        addressee_slot,
        body,
    };

    let mail = store.insert(mail_draft)?;

    // Spec PostMail consequents:
    //   session.user.messages_posted += 1
    //   if exists membership: membership.messages_posted += 1
    //
    // The membership existence check above guarantees a granted row
    // exists, so the lookup must succeed here.
    user.bump_messages_posted();
    let membership = user
        .memberships_mut()
        .iter_mut()
        .find(|m| m.conference_number() == msgbase.conference_number())
        .expect("membership existence was checked above");
    membership.bump_messages_posted();

    Ok(mail)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::conference::ConferenceMembership;
    use crate::domain::messaging::mail::{Mail, MailDraft, MailVisibility};
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

    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;

    fn sample_draft() -> PostMailDraft {
        PostMailDraft {
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            addressee_slot: Some(3),
            from_name: "alice".to_string(),
            subject: "Hi".to_string(),
            body: "Hello, Bob.".to_string(),
            private: false,
            posted_at: t(100),
        }
    }

    #[test]
    fn happy_path_persists_mail_and_increments_counters() {
        // Spec PostMail consequents: Mail.created + highest_message += 1
        // + user.messages_posted += 1 + membership.messages_posted += 1.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let mail = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect("happy path");

        assert_eq!(mail.number(), 1);
        assert_eq!(mail.author_slot(), 2);
        assert_eq!(mail.addressee_slot(), Some(3));
        assert_eq!(mail.from_name(), "alice");
        assert_eq!(mail.to_name(), "bob");
        assert_eq!(mail.subject(), "Hi");
        assert_eq!(mail.body(), "Hello, Bob.");
        assert_eq!(mail.visibility(), MailVisibility::Public);
        assert_eq!(mail.broadcast_to(), BroadcastTo::None);
        assert_eq!(mail.posted_at(), t(100));
        assert_eq!(mail.received_at(), None);
        assert_eq!(store.highest_message(), 1);
        assert_eq!(user.messages_posted(), 1);
        let membership = user
            .memberships()
            .iter()
            .find(|m| m.conference_number() == 2)
            .expect("present");
        assert_eq!(membership.messages_posted(), 1);
    }

    #[test]
    fn private_draft_persists_as_private_visibility() {
        // Spec visibility selector: `else if draft.private: private`.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mut draft = sample_draft();
        draft.private = true;

        let mail = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect("happy path");
        assert_eq!(mail.visibility(), MailVisibility::Private);
    }

    #[test]
    fn second_post_allocates_the_next_message_number() {
        // Spec PostMail: next_number = visit.msgbase.highest_message + 1.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let first = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .unwrap();
        let second = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .unwrap();
        assert_eq!(first.number(), 1);
        assert_eq!(second.number(), 2);
        assert_eq!(store.highest_message(), 2);
        assert_eq!(user.messages_posted(), 2);
        assert_eq!(
            user.memberships()
                .iter()
                .find(|m| m.conference_number() == 2)
                .unwrap()
                .messages_posted(),
            2,
        );
    }

    #[test]
    fn rejects_when_user_lacks_enter_message_right() {
        // A pending-validation new user has only ReadMessage and
        // CommentToSysop; EnterMessage is gated out. Spec PostMail
        // `requires: has_access(session.user, enter_message)`.
        let mut new_user = User::register_new(crate::domain::user::NewUserRegistration {
            slot_number: 9,
            handle: "newcomer".to_string(),
            location: None,
            phone_number: None,
            email: None,
            password_hash: "h".to_string(),
            password_salt: Some("s".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 0,
            ansi_colour: false,
            flags: std::collections::BTreeSet::new(),
            ratio_mode: crate::domain::user::RatioMode::Disabled,
            ratio_value: 0,
            now: SystemTime::UNIX_EPOCH,
        })
        .expect("valid");
        new_user.upsert_membership(ConferenceMembership::new(2, true));
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let err = post_mail(
            &mut new_user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect_err("expect access denied for new user");
        assert!(matches!(err, PostMailError::AccessDenied), "got {err:?}",);
        // No side-effects on rejection.
        assert_eq!(new_user.messages_posted(), 0);
        assert_eq!(store.highest_message(), 0);
    }

    #[test]
    fn rejects_when_user_has_no_membership_for_the_conference() {
        // A user with no granted membership for the message base's
        // parent conference cannot post there.
        let mut user = User::new(
            5,
            "no-grants".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect_err("expect no membership");
        assert!(matches!(err, PostMailError::NoMembership), "got {err:?}");
        assert_eq!(store.highest_message(), 0);
        assert_eq!(user.messages_posted(), 0);
    }

    #[test]
    fn rejects_revoked_membership_as_no_membership() {
        // A revoked membership row (`granted = false`) must not
        // satisfy the membership gate. Mirrors
        // `has_membership(user, conference)`'s requirement that the
        // row be granted.
        let mut user = User::new(
            2,
            "revoked".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid");
        user.upsert_membership(ConferenceMembership::new(2, false));
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect_err("expect no membership for revoked row");
        assert!(matches!(err, PostMailError::NoMembership), "got {err:?}");
    }

    #[test]
    fn broadcast_all_persists_with_no_addressee_when_msgbase_allows() {
        // Spec messaging.allium:PostMail with `broadcast_to = all` and
        // `addressing_allows(msgbase, all) = true`:
        //   - `Mail.broadcast_to = all`
        //   - `Mail.addressee = null`
        //   - per-user / per-membership counters still bump.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            addressee_slot: None,
            from_name: "alice".to_string(),
            subject: "Notice".to_string(),
            body: "Hi all.".to_string(),
            private: false,
            posted_at: t(100),
        };

        let mail = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect("broadcast post should succeed on an Any base");

        assert_eq!(mail.broadcast_to(), BroadcastTo::All);
        assert_eq!(mail.addressee_slot(), None);
        assert_eq!(mail.to_name(), "ALL");
        assert_eq!(mail.visibility(), MailVisibility::Public);
        assert_eq!(user.messages_posted(), 1);
        assert_eq!(
            user.memberships()
                .iter()
                .find(|m| m.conference_number() == 2)
                .unwrap()
                .messages_posted(),
            1,
        );
    }

    #[test]
    fn broadcast_eall_forces_public_visibility_even_when_draft_private() {
        // Spec messaging.allium:PostMail visibility selector:
        //   if draft.broadcast_to = eall: public
        // EALL is fan-out; private-EALL has no addressee to scope it to,
        // so the rule forces public.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "EALL".to_string(),
            broadcast_to: BroadcastTo::Eall,
            addressee_slot: None,
            from_name: "alice".to_string(),
            subject: "Echo".to_string(),
            body: "Hi everywhere.".to_string(),
            private: true,
            posted_at: t(100),
        };

        let mail = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect("EALL on Any base");
        assert_eq!(mail.visibility(), MailVisibility::Public);
        assert_eq!(mail.broadcast_to(), BroadcastTo::Eall);
        assert_eq!(mail.addressee_slot(), None);
    }

    #[test]
    fn rejects_broadcast_all_when_base_forbids_it() {
        // Spec messaging.allium:PostMail:
        //   requires: draft.broadcast_to != all or addressing_allows(visit.msgbase, all)
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            addressee_slot: None,
            from_name: "alice".to_string(),
            subject: "Notice".to_string(),
            body: "Hi all.".to_string(),
            private: false,
            posted_at: t(100),
        };

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::IndividualOnly,
            &mut store,
            draft,
        )
        .expect_err("ALL must be rejected when AllowedAddressing forbids it");
        assert!(
            matches!(err, PostMailError::AddressingNotAllowed),
            "got {err:?}",
        );
        assert_eq!(store.highest_message(), 0);
        assert_eq!(user.messages_posted(), 0);
    }

    #[test]
    fn rejects_broadcast_eall_when_base_only_allows_all() {
        // Spec: `IndividualOrAll` permits ALL but not EALL.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "EALL".to_string(),
            broadcast_to: BroadcastTo::Eall,
            addressee_slot: None,
            from_name: "alice".to_string(),
            subject: "Echo".to_string(),
            body: "Across".to_string(),
            private: false,
            posted_at: t(100),
        };

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::IndividualOrAll,
            &mut store,
            draft,
        )
        .expect_err("EALL must be rejected when only ALL is allowed");
        assert!(
            matches!(err, PostMailError::AddressingNotAllowed),
            "got {err:?}",
        );
    }

    #[test]
    fn broadcast_draft_with_addressee_slot_is_rejected() {
        // Spec messaging.allium:Mail constraint:
        //   addressee: core/User? when broadcast_to = none
        // A draft that asks for ALL/EALL must not carry an addressee.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            addressee_slot: Some(3),
            from_name: "alice".to_string(),
            subject: "Mixed".to_string(),
            body: "Hi.".to_string(),
            private: false,
            posted_at: t(100),
        };

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect_err("broadcast draft with addressee_slot is malformed");
        assert!(
            matches!(err, PostMailError::AddresseeMismatch),
            "got {err:?}",
        );
    }

    #[test]
    fn individual_draft_without_addressee_slot_is_rejected() {
        // The single-addressee path requires a resolved addressee
        // (`requires: resolved_addressee != null` when broadcast_to =
        // none).
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let draft = PostMailDraft {
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            addressee_slot: None,
            from_name: "alice".to_string(),
            subject: "Hi".to_string(),
            body: "Hello, Bob.".to_string(),
            private: false,
            posted_at: t(100),
        };

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect_err("individual post without addressee_slot is malformed");
        assert!(
            matches!(err, PostMailError::AddresseeMismatch),
            "got {err:?}",
        );
    }

    #[test]
    fn rejects_empty_addressee() {
        // For Slice 42 the legacy reroute of empty -> ALL is deferred
        // to Slice 43; the rule refuses an empty `to_name` outright so
        // an editor mishap doesn't silently persist an addressee-less
        // mail.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mut draft = sample_draft();
        draft.to_name.clear();

        let err = post_mail(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            draft,
        )
        .expect_err("expect EmptyAddressee");
        assert!(matches!(err, PostMailError::EmptyAddressee), "got {err:?}",);
        assert_eq!(store.highest_message(), 0);
        assert_eq!(user.messages_posted(), 0);
    }
}
