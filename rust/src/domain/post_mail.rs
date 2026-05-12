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

use crate::domain::conference::MessageBaseRef;
use crate::domain::mail::{BroadcastTo, Mail, MailDraft, MailVisibility};
use crate::domain::mail_store::{MailStore, MailStoreError};
use crate::domain::user::{Right, User};

/// Caller-resolved fields for a single-addressee post
/// (spec: `messaging.allium:PostMail` consequent fields).
///
/// `to_name` and `from_name` are the display strings the caller has
/// already resolved via the spec's `display_name_of(_, conference)`
/// black box — the rule does not consult the conference catalogue.
///
/// `addressee_slot` is the spec's `resolved_addressee` — the user
/// repository looked the typed handle up before the rule fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostMailDraft {
    /// Display name written to `Mail.to_name`.
    pub to_name: String,
    /// Resolved addressee's stable slot number
    /// (spec: `resolved_addressee != null`).
    pub addressee_slot: u32,
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
    /// (`if user.censored: private_to_sysop else if draft.private:
    /// private else: public`). The censored branch is deferred to
    /// Slice 47 — Slice 42 always reads `user.censored = false`.
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
    /// The supplied draft has an empty `to_name`. The legacy code
    /// reroutes empty `to_name` to ALL — that branch lands with
    /// Slice 43. For now, refuse the empty case explicitly so it
    /// doesn't silently persist a no-recipient message.
    #[error("recipient name is empty")]
    EmptyAddressee,
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
    store: &mut dyn MailStore,
    draft: PostMailDraft,
) -> Result<Mail, PostMailError> {
    if !user.has_access(Right::EnterMessage) {
        return Err(PostMailError::AccessDenied);
    }
    if draft.to_name.is_empty() {
        return Err(PostMailError::EmptyAddressee);
    }
    if !user
        .memberships()
        .iter()
        .any(|m| m.conference_number() == msgbase.conference_number() && m.is_granted())
    {
        return Err(PostMailError::NoMembership);
    }

    // Slice 42 visibility selector: censored users land in Slice 47,
    // so for now `user.censored` is implicitly false. Broadcast
    // routing (ALL / EALL) lands in Slice 43; until then every post
    // is `broadcast_to = none`.
    let visibility = if draft.private {
        MailVisibility::Private
    } else {
        MailVisibility::Public
    };

    let PostMailDraft {
        to_name,
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
        broadcast_to: BroadcastTo::None,
        subject,
        posted_at,
        author_slot: user.slot_number(),
        addressee_slot: Some(addressee_slot),
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
    use crate::domain::mail::{Mail, MailDraft, MailVisibility};
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

    /// In-memory [`MailStore`] for testing the rule's contract with
    /// the port. Mirrors `FileMailStore` semantics: monotonic numbers
    /// allocated at insert time, payload stored verbatim, save
    /// replaces.
    struct InMemoryStore {
        msgbase: MessageBaseRef,
        highest: u32,
        mails: Vec<Mail>,
    }

    impl InMemoryStore {
        fn new(msgbase: MessageBaseRef) -> Self {
            Self {
                msgbase,
                highest: 0,
                mails: Vec::new(),
            }
        }
    }

    impl MailStore for InMemoryStore {
        fn highest_message(&self) -> u32 {
            self.highest
        }
        fn msgbase(&self) -> MessageBaseRef {
            self.msgbase
        }
        fn insert(&mut self, draft: MailDraft) -> Result<Mail, MailStoreError> {
            let number = self.highest + 1;
            let mail = Mail::from_draft(self.msgbase, number, draft);
            self.mails.push(mail.clone());
            self.highest = number;
            Ok(mail)
        }
        fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError> {
            Ok(self.mails.iter().find(|m| m.number() == number).cloned())
        }
        fn save(&mut self, mail: &Mail) -> Result<(), MailStoreError> {
            if let Some(existing) = self.mails.iter_mut().find(|m| m.number() == mail.number()) {
                *existing = mail.clone();
            }
            Ok(())
        }
    }

    fn sample_draft() -> PostMailDraft {
        PostMailDraft {
            to_name: "bob".to_string(),
            addressee_slot: 3,
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
        let mut store = InMemoryStore::new(msgbase);

        let mail = post_mail(&mut user, msgbase, &mut store, sample_draft()).expect("happy path");

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
        let mut store = InMemoryStore::new(msgbase);
        let mut draft = sample_draft();
        draft.private = true;

        let mail = post_mail(&mut user, msgbase, &mut store, draft).expect("happy path");
        assert_eq!(mail.visibility(), MailVisibility::Private);
    }

    #[test]
    fn second_post_allocates_the_next_message_number() {
        // Spec PostMail: next_number = visit.msgbase.highest_message + 1.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);

        let first = post_mail(&mut user, msgbase, &mut store, sample_draft()).unwrap();
        let second = post_mail(&mut user, msgbase, &mut store, sample_draft()).unwrap();
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
        let mut store = InMemoryStore::new(msgbase);

        let err = post_mail(&mut new_user, msgbase, &mut store, sample_draft())
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
        let mut store = InMemoryStore::new(msgbase);

        let err = post_mail(&mut user, msgbase, &mut store, sample_draft())
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
        let mut store = InMemoryStore::new(msgbase);

        let err = post_mail(&mut user, msgbase, &mut store, sample_draft())
            .expect_err("expect no membership for revoked row");
        assert!(matches!(err, PostMailError::NoMembership), "got {err:?}");
    }

    #[test]
    fn rejects_empty_addressee() {
        // For Slice 42 the legacy reroute of empty -> ALL is deferred
        // to Slice 43; the rule refuses an empty `to_name` outright so
        // an editor mishap doesn't silently persist an addressee-less
        // mail.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);
        let mut draft = sample_draft();
        draft.to_name.clear();

        let err =
            post_mail(&mut user, msgbase, &mut store, draft).expect_err("expect EmptyAddressee");
        assert!(matches!(err, PostMailError::EmptyAddressee), "got {err:?}",);
        assert_eq!(store.highest_message(), 0);
        assert_eq!(user.messages_posted(), 0);
    }
}
