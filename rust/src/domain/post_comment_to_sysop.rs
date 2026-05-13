//! [`post_comment_to_sysop`] rule (spec:
//! `messaging.allium:PostCommentToSysop`).
//!
//! Phase 7, Slice 44. A specialised post that addresses the sysop
//! directly, gates on [`Right::CommentToSysop`] (not
//! [`Right::EnterMessage`]), and always marks the resulting mail as
//! `private`. The legacy `C` command (`amiexpress/express.e:24910`)
//! is the surface user-facing flow.
//!
//! Per the spec the rule expands to `PostMail` with a fixed draft
//! (`broadcast_to = none, to_name = "Sysop", private = true`). The
//! caller has already resolved the sysop's stable slot number via the
//! [`crate::domain::user_repository::UserRepository::find_sysop`]
//! port, so this rule stays a pure domain function over already-
//! resolved data.

use std::time::SystemTime;

use crate::domain::conference::MessageBaseRef;
use crate::domain::mail::{AllowedAddressing, BroadcastTo, Mail};
use crate::domain::mail_store::MailStore;
use crate::domain::post_mail::{apply_post_mail, PostMailDraft, PostMailError};
use crate::domain::user::{Right, User};

/// Caller-resolved fields for a comment-to-sysop post
/// (spec: `messaging.allium:PostCommentToSysop` consequent fields).
///
/// The recipient is fixed by the rule (`"Sysop"`), so the caller only
/// supplies the sysop's stable slot number, the author display
/// metadata and the message body/subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentToSysopDraft {
    /// Stable slot number of the sysop record (spec invariant
    /// `is_sysop: slot_number = 1`).
    pub sysop_slot: u32,
    /// Author's display name in the current conference
    /// (spec: `Mail.from_name`, invariant `FromNameMatchesAuthor`).
    pub from_name: String,
    /// Free-text subject (spec: `Mail.subject`).
    pub subject: String,
    /// Free-text body (spec: `Mail.body`).
    pub body: String,
    /// `now` recorded as `Mail.posted_at` (spec: `posted_at: now`).
    pub posted_at: SystemTime,
}

/// Applies `messaging.allium:PostCommentToSysop` to
/// `(user, msgbase, store, draft)`.
///
/// Spec expansion: the rule's body is `PostMail(session, draft = {
/// broadcast_to: none, to_name: "Sysop", private: true, subject,
/// body })`. This implementation calls the shared
/// [`apply_post_mail`] helper rather than recursing through
/// [`crate::domain::post_mail::post_mail`] — the latter gates on
/// [`Right::EnterMessage`], which a pending-validation new user
/// (Slice 21) explicitly lacks even though they retain
/// [`Right::CommentToSysop`].
///
/// On success the mail is persisted with `visibility = Private` and
/// `broadcast_to = None`; both [`User::messages_posted`] and the
/// matching membership row's counter are incremented.
///
/// # Errors
/// Returns the matching [`PostMailError`] variant when a `requires`
/// gate fails or the store rejects the write:
/// - [`PostMailError::AccessDenied`] when the user lacks
///   [`Right::CommentToSysop`];
/// - [`PostMailError::NoMembership`] when the user has no granted
///   membership for the message base's parent conference;
/// - [`PostMailError::Store`] for underlying store failures.
pub fn post_comment_to_sysop(
    user: &mut User,
    msgbase: MessageBaseRef,
    allowed_addressing: AllowedAddressing,
    store: &mut dyn MailStore,
    draft: CommentToSysopDraft,
) -> Result<Mail, PostMailError> {
    if !user.has_access(Right::CommentToSysop) {
        return Err(PostMailError::AccessDenied);
    }
    let CommentToSysopDraft {
        sysop_slot,
        from_name,
        subject,
        body,
        posted_at,
    } = draft;
    apply_post_mail(
        user,
        msgbase,
        allowed_addressing,
        store,
        PostMailDraft {
            to_name: "Sysop".to_string(),
            broadcast_to: BroadcastTo::None,
            addressee_slot: Some(sysop_slot),
            from_name,
            subject,
            body,
            private: true,
            posted_at,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::domain::conference::ConferenceMembership;
    use crate::domain::mail::{Mail, MailDraft, MailVisibility};
    use crate::domain::mail_store::MailStoreError;
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

    fn sample_draft() -> CommentToSysopDraft {
        CommentToSysopDraft {
            sysop_slot: 1,
            from_name: "alice".to_string(),
            subject: "Need help".to_string(),
            body: "There's a typo on the welcome screen.".to_string(),
            posted_at: t(100),
        }
    }

    #[test]
    fn persists_as_private_to_sysop_and_bumps_counters() {
        // Spec messaging.allium:PostCommentToSysop ensures PostMail with
        //   to_name: "Sysop", broadcast_to: none, private: true.
        // The store records the mail and counters bump as in PostMail.
        let mut user = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);

        let mail = post_comment_to_sysop(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect("comment to sysop should succeed");

        assert_eq!(mail.to_name(), "Sysop");
        assert_eq!(mail.from_name(), "alice");
        assert_eq!(mail.subject(), "Need help");
        assert_eq!(mail.body(), "There's a typo on the welcome screen.");
        assert_eq!(mail.broadcast_to(), BroadcastTo::None);
        assert_eq!(mail.addressee_slot(), Some(1));
        assert_eq!(mail.visibility(), MailVisibility::Private);
        assert_eq!(store.highest_message(), 1);
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
    fn new_user_can_comment_even_without_enter_message_right() {
        // Spec Slice 21: a pending-validation new user has only
        // ReadMessage and CommentToSysop rights, but PostCommentToSysop
        // must still fire — the rule's gate is `comment_to_sysop`, not
        // `enter_message`.
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
        assert!(new_user.has_access(Right::CommentToSysop));
        assert!(!new_user.has_access(Right::EnterMessage));

        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);
        let mail = post_comment_to_sysop(
            &mut new_user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect("a new user must still be able to comment to sysop");
        assert_eq!(mail.visibility(), MailVisibility::Private);
        assert_eq!(mail.to_name(), "Sysop");
    }

    #[test]
    fn happy_path_user_has_comment_to_sysop_right() {
        // The current user tiers (validated + new-user) both grant
        // CommentToSysop, so the access-gate's failure branch is
        // structurally unreachable until Slice 47 introduces the
        // censored tier. Pin the happy-path precondition so a future
        // refactor that revokes the right unexpectedly trips this
        // assertion long before the rule starts silently dropping
        // operator pages.
        let user = make_user(2);
        assert!(user.has_access(Right::CommentToSysop));
        let new_user = User::register_new(crate::domain::user::NewUserRegistration {
            slot_number: 9,
            handle: "n2".to_string(),
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
        assert!(new_user.has_access(Right::CommentToSysop));
    }

    #[test]
    fn rejects_when_user_has_no_membership_for_conference() {
        // Spec: a user posting to a base must have a granted
        // membership for the parent conference. Comment-to-sysop is no
        // exception — the message lands inside a message base.
        let mut stranger = User::new(
            5,
            "stranger".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);
        let err = post_comment_to_sysop(
            &mut stranger,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .expect_err("no membership for conference");
        assert!(matches!(err, PostMailError::NoMembership), "got {err:?}");
        assert_eq!(store.highest_message(), 0);
        assert_eq!(stranger.messages_posted(), 0);
    }

    #[test]
    fn forces_private_visibility_even_for_validated_users() {
        // Spec: `private: true` is hard-coded in the rule's PostMail
        // expansion. A regular validated user therefore cannot leave a
        // "public" comment to sysop.
        let mut user = make_user(7);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryStore::new(msgbase);
        let mail = post_comment_to_sysop(
            &mut user,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            sample_draft(),
        )
        .unwrap();
        assert_eq!(mail.visibility(), MailVisibility::Private);
    }
}
