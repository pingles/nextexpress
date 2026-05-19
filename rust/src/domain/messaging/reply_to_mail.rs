//! [`reply_to_mail`] rule (spec: `messaging.allium:ReplyToMail`).
//!
//! Phase 8, Slice 45. A thin domain function that turns a reply
//! draft + the message being replied to into a fully-formed
//! [`PostMailDraft`], then delegates to the shared
//! [`crate::domain::messaging::post_mail::post_mail`] body. The spec
//! defines `ReplyToMail` as `PostMail` with the addressee derived
//! from the source: by default the source's `from_name` (and
//! `author_slot`), or `"ALL"` when the source was an ALL broadcast
//! and the caller asks to keep the broadcast.
//!
//! Wire-level concerns (the post-read `R` prompt, body editor,
//! quoting the original message) live in the application layer.
//! Slice 45 only models the pure domain rule.

use std::time::SystemTime;

use crate::domain::conference::{AllowedAddressing, MessageBaseRef};
use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility};
use crate::domain::messaging::mail_store::MailStore;
use crate::domain::messaging::post_mail::{post_mail, PostMailDraft, PostMailError};
use crate::domain::user::User;

/// Caller-supplied fields for a reply
/// (spec: `messaging.allium:ReplyToMail` consequent fields, minus
/// the addressee fields which the rule derives from the source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyToMailDraft {
    /// Author's display name in the current conference
    /// (spec: `Mail.from_name`, invariant `FromNameMatchesAuthor`).
    pub from_name: String,
    /// Free-text subject (spec: `Mail.subject`).
    pub subject: String,
    /// Free-text body (spec: `Mail.body`).
    pub body: String,
    /// User answered "yes" at the legacy `Private (y/n)?` prompt
    /// (spec: `draft.private`).
    pub private: bool,
    /// Spec black box `reply_keeps_broadcast(draft)`. When `true` and
    /// `source.broadcast_to = all`, the reply is addressed back to
    /// `"ALL"` instead of the original author.
    pub reply_keeps_broadcast: bool,
    /// `now` recorded as `Mail.posted_at` (spec: `posted_at: now`).
    pub posted_at: SystemTime,
}

/// Errors raised by [`reply_to_mail`]. The rule's own `requires`
/// surface as [`ReplyToMailError::SourceDeleted`]; every other gate
/// (access, membership, addressing) bubbles up from the underlying
/// [`post_mail`] call as [`ReplyToMailError::Post`].
#[derive(Debug, thiserror::Error)]
pub enum ReplyToMailError {
    /// `requires: source.visibility != deleted` — the message being
    /// replied to has been soft-deleted by the sysop.
    #[error("source mail is deleted")]
    SourceDeleted,
    /// The underlying [`post_mail`] call rejected the derived draft.
    #[error(transparent)]
    Post(#[from] PostMailError),
}

/// Applies `messaging.allium:ReplyToMail` to
/// `(user, msgbase, store, source, draft)`.
///
/// Spec expansion: the rule's body is `PostMail(session, draft = {
/// broadcast_to: <see below>, to_name: <see below>, private,
/// subject, body })`, where the addressee follows the spec's
/// `reply_to_name` selector:
///
/// - `source.broadcast_to = all` and `reply_keeps_broadcast` →
///   `to_name: "ALL"`, `broadcast_to: All`, `addressee_slot: None`;
/// - otherwise → `to_name: source.from_name`,
///   `broadcast_to: None`, `addressee_slot: Some(source.author_slot)`.
///
/// EALL sources fall through the second branch (the spec's selector
/// only matches `all`), so a reply to an echo-all message goes
/// privately back to its author.
///
/// On success the reply is persisted under
/// `store.highest_message() + 1`, and the user's and membership's
/// `messages_posted` counters each bump by one (per [`post_mail`]).
///
/// # Errors
/// - [`ReplyToMailError::SourceDeleted`] when the source mail has
///   `visibility = Deleted`;
/// - [`ReplyToMailError::Post`] wrapping any [`PostMailError`] that
///   the underlying post raises (access denied, missing membership,
///   addressing not allowed, store failure).
pub fn reply_to_mail(
    user: &mut User,
    msgbase: MessageBaseRef,
    allowed_addressing: AllowedAddressing,
    store: &mut dyn MailStore,
    source: &Mail,
    draft: ReplyToMailDraft,
) -> Result<Mail, ReplyToMailError> {
    if matches!(source.visibility(), MailVisibility::Deleted) {
        return Err(ReplyToMailError::SourceDeleted);
    }

    let keeps_all =
        matches!(source.broadcast_to(), BroadcastTo::All) && draft.reply_keeps_broadcast;
    let (to_name, broadcast_to, addressee_slot) = if keeps_all {
        ("ALL".to_string(), BroadcastTo::All, None)
    } else {
        (
            source.from_name().to_string(),
            BroadcastTo::None,
            Some(source.author_slot()),
        )
    };

    let ReplyToMailDraft {
        from_name,
        subject,
        body,
        private,
        reply_keeps_broadcast: _,
        posted_at,
    } = draft;

    let mail = post_mail(
        user,
        msgbase,
        allowed_addressing,
        store,
        PostMailDraft {
            to_name,
            broadcast_to,
            addressee_slot,
            from_name,
            subject,
            body,
            private,
            posted_at,
        },
    )?;
    Ok(mail)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::domain::conference::{AllowedAddressing, ConferenceMembership, MessageBaseRef};
    use crate::domain::messaging::mail::{BroadcastTo, Mail, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::messaging::reply_to_mail::{
        reply_to_mail, ReplyToMailDraft, ReplyToMailError,
    };
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::User;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn make_user(slot: u32, handle: &str) -> User {
        let mut user = User::new(
            slot,
            handle.to_string(),
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

    fn source_from(
        store: &mut InMemoryMailStore,
        from_name: &str,
        author_slot: u32,
        to_name: &str,
        addressee_slot: Option<u32>,
        broadcast_to: BroadcastTo,
    ) -> Mail {
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: from_name.to_string(),
                to_name: to_name.to_string(),
                broadcast_to,
                subject: "original".to_string(),
                posted_at: t(50),
                author_slot,
                addressee_slot,
                body: "original body".to_string(),
            })
            .expect("source insert")
    }

    fn reply_draft() -> ReplyToMailDraft {
        ReplyToMailDraft {
            from_name: "alice".to_string(),
            subject: "Re: original".to_string(),
            body: "thanks".to_string(),
            private: false,
            reply_keeps_broadcast: false,
            posted_at: t(100),
        }
    }

    #[test]
    fn reply_to_single_addressee_addresses_the_original_author() {
        // Spec ReplyToMail: when source.broadcast_to != all, the
        // reply's to_name = source.from_name and addressee_slot =
        // source.author_slot, with broadcast_to = None.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "alice",
            Some(2),
            BroadcastTo::None,
        );

        let reply = reply_to_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            reply_draft(),
        )
        .expect("happy path");

        assert_eq!(reply.to_name(), "charlie");
        assert_eq!(reply.addressee_slot(), Some(4));
        assert_eq!(reply.broadcast_to(), BroadcastTo::None);
        assert_eq!(reply.from_name(), "alice");
        assert_eq!(reply.subject(), "Re: original");
        assert_eq!(reply.body(), "thanks");
        assert_eq!(reply.visibility(), MailVisibility::Public);
        assert_eq!(reply.posted_at(), t(100));
    }

    #[test]
    fn reply_to_all_broadcast_keeps_all_when_reply_keeps_broadcast_is_set() {
        // Spec ReplyToMail: if source.broadcast_to = all and
        // reply_keeps_broadcast(draft), reply_to_name = "ALL"; the
        // post then receives broadcast_to = All.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(&mut store, "charlie", 4, "ALL", None, BroadcastTo::All);

        let mut draft = reply_draft();
        draft.reply_keeps_broadcast = true;
        let reply = reply_to_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            draft,
        )
        .expect("broadcast reply allowed");

        assert_eq!(reply.to_name(), "ALL");
        assert_eq!(reply.broadcast_to(), BroadcastTo::All);
        assert_eq!(reply.addressee_slot(), None);
    }

    #[test]
    fn reply_to_all_broadcast_falls_back_to_author_when_reply_keeps_broadcast_is_unset() {
        // Spec selector: `else: source.from_name`. The default
        // (`reply_keeps_broadcast = false`) treats the ALL source as
        // if it were a personal message — the reply goes privately
        // back to the original author.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(&mut store, "charlie", 4, "ALL", None, BroadcastTo::All);

        let reply = reply_to_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            reply_draft(), // reply_keeps_broadcast = false
        )
        .expect("personal reply allowed");

        assert_eq!(reply.to_name(), "charlie");
        assert_eq!(reply.broadcast_to(), BroadcastTo::None);
        assert_eq!(reply.addressee_slot(), Some(4));
    }

    #[test]
    fn rejects_when_source_is_deleted() {
        // Spec ReplyToMail: `requires: source.visibility != deleted`.
        // The store still holds the row (DeleteMail is a soft delete
        // — Slice 49) but we refuse to thread a reply off it.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mut source = source_from(
            &mut store,
            "charlie",
            4,
            "alice",
            Some(2),
            BroadcastTo::None,
        );
        source
            .transition_to(MailVisibility::Deleted)
            .expect("transition to Deleted");

        let err = reply_to_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            reply_draft(),
        )
        .expect_err("must refuse a deleted source");

        assert!(
            matches!(err, ReplyToMailError::SourceDeleted),
            "got {err:?}"
        );
        // No write happened — only the original source is stored.
        assert_eq!(store.highest_message(), 1);
        assert_eq!(alice.messages_posted(), 0);
    }

    #[test]
    fn reply_to_eall_source_addresses_author_even_when_reply_keeps_broadcast_is_set() {
        // Spec selector matches `source.broadcast_to = all`
        // explicitly; EALL is not covered, so an EALL reply falls
        // through to `else: source.from_name`. Pin this so a future
        // refactor that broadens the match to "any broadcast" trips
        // here.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(&mut store, "charlie", 4, "EALL", None, BroadcastTo::Eall);

        let mut draft = reply_draft();
        draft.reply_keeps_broadcast = true;
        let reply = reply_to_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            draft,
        )
        .expect("eall reply allowed");

        assert_eq!(reply.to_name(), "charlie");
        assert_eq!(reply.broadcast_to(), BroadcastTo::None);
        assert_eq!(reply.addressee_slot(), Some(4));
    }

    #[test]
    fn propagates_post_mail_no_membership_error() {
        // ReplyToMail expands to PostMail; the underlying gates
        // (access, membership, addressing) bubble up unchanged
        // under the `Post(_)` wrapper. Pin one variant — the
        // membership gate — so the `?` chain stays intact.
        let mut stranger = User::new(
            5,
            "stranger".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid"); // no memberships granted
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "stranger",
            Some(5),
            BroadcastTo::None,
        );

        let err = reply_to_mail(
            &mut stranger,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            reply_draft(),
        )
        .expect_err("no membership for conference");

        assert!(
            matches!(
                err,
                ReplyToMailError::Post(
                    crate::domain::messaging::post_mail::PostMailError::NoMembership
                )
            ),
            "got {err:?}"
        );
        assert_eq!(store.highest_message(), 1);
        assert_eq!(stranger.messages_posted(), 0);
    }
}
