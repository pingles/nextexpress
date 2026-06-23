//! [`forward_mail`] rule (spec: `messaging.allium:ForwardMail`).
//!
//! Phase 8, Slice 46. Composes a `Fwd: …` mail in the same message
//! base from a `source` mail and a [`ForwardMailRequest`] then
//! delegates persistence to [`post_mail`]. The new mail's body
//! always carries the [`forward_header_for`] block followed by the
//! original body; if the request supplies an
//! [`ForwardMailRequest::additional_note`] it is appended after a
//! `--` separator.
//!
//! Visibility follows the spec's selector
//! `private: request.source.visibility != public` — a forward of a
//! Private or `PrivateToSysop` mail stays private; a forward of a
//! Public mail stays public.
//!
//! Wire-level concerns (the post-read `F` prompt, name resolution,
//! optional-note editor) live in the application layer.

use std::time::SystemTime;

use time::OffsetDateTime;

use crate::domain::conference::{AllowedAddressing, MessageBaseRef};
use crate::domain::messaging::limits::MAX_MAIL_BODY_BYTES;
use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility};
use crate::domain::messaging::mail_store::MailStore;
use crate::domain::messaging::post_mail::{post_mail, PostMailDraft, PostMailError};
use crate::domain::messaging::read_mail::can_read;
use crate::domain::user::User;

/// Caller-supplied fields for a forward
/// (spec: `messaging.allium:ForwardMail` `request` fields).
///
/// `new_addressee_name` and `new_addressee_slot` are the caller's
/// pre-resolved equivalents of the spec's
/// `display_name_of(request.new_addressee, …)` — the menu handler
/// looks the typed handle up through the user repository before
/// invoking this rule. The `from_name` follows
/// `messaging.allium:FromNameMatchesAuthor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardMailRequest {
    /// Display name of the new addressee (spec: `display_name_of`).
    pub new_addressee_name: String,
    /// Stable slot number of the new addressee
    /// (spec: `resolved_addressee != null`).
    pub new_addressee_slot: u32,
    /// Optional free-text note appended after a `--` separator
    /// (spec: `request.additional_note`).
    pub additional_note: Option<String>,
    /// Author's display name in the current conference
    /// (spec: `Mail.from_name`, invariant `FromNameMatchesAuthor`).
    pub from_name: String,
    /// `now` recorded as `Mail.posted_at` (spec: `posted_at: now`).
    pub posted_at: SystemTime,
}

/// Errors raised by [`forward_mail`]. Source-message gates surface as
/// [`ForwardMailError::SourceDeleted`] and
/// [`ForwardMailError::SourceNotPermitted`]; every other gate (access,
/// membership, addressing) bubbles up from the underlying [`post_mail`]
/// call as [`ForwardMailError::Post`].
#[derive(Debug, thiserror::Error)]
pub enum ForwardMailError {
    /// `requires: not request.source.is_deleted` — the message
    /// being forwarded has been soft-deleted.
    #[error("source mail is deleted")]
    SourceDeleted,
    /// The caller is not permitted to read the source message. A
    /// forward includes the original body, so it must not bypass the
    /// same private-mail visibility gate as `ReadMail`.
    #[error("user is not permitted to read the source mail")]
    SourceNotPermitted,
    /// The underlying [`post_mail`] call rejected the derived draft.
    #[error(transparent)]
    Post(#[from] PostMailError),
}

/// Spec black box `forward_header_for(mail)` — the
/// "From: … Date: … Subject: …" block prepended to the body of a
/// forwarded mail. The date is rendered in RFC3339 form (matching
/// `app::menu_flow::read_mail::render_mail_header`); the block ends with a
/// trailing newline so the spec's `header + "\n" + body` join
/// produces the expected two-newline boundary.
#[must_use]
pub fn forward_header_for(mail: &Mail) -> String {
    let posted = OffsetDateTime::from(mail.posted_at())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    format!(
        "From: {}\nDate: {}\nSubject: {}\n",
        mail.from_name(),
        posted,
        mail.subject(),
    )
}

/// Applies `messaging.allium:ForwardMail` to
/// `(user, msgbase, store, source, request)`.
///
/// On success the forwarded mail is persisted under
/// `store.highest_message() + 1`, and the user's and membership's
/// `messages_posted` counters each bump by one (per [`post_mail`]).
///
/// # Errors
/// - [`ForwardMailError::SourceDeleted`] when the source mail has
///   `visibility = Deleted`;
/// - [`ForwardMailError::SourceNotPermitted`] when the caller cannot
///   read the source mail;
/// - [`ForwardMailError::Post`] wrapping any [`PostMailError`] that
///   the underlying post raises (access denied, missing membership,
///   addressing not allowed, store failure).
pub fn forward_mail(
    user: &mut User,
    msgbase: MessageBaseRef,
    allowed_addressing: AllowedAddressing,
    store: &mut dyn MailStore,
    source: &Mail,
    request: ForwardMailRequest,
) -> Result<Mail, ForwardMailError> {
    if matches!(source.visibility(), MailVisibility::Deleted) {
        return Err(ForwardMailError::SourceDeleted);
    }
    if !can_read(user, source) {
        return Err(ForwardMailError::SourceNotPermitted);
    }

    let ForwardMailRequest {
        new_addressee_name,
        new_addressee_slot,
        additional_note,
        from_name,
        posted_at,
    } = request;

    let mut body = forward_header_for(source);
    let note_bytes = additional_note
        .as_ref()
        .map_or(0, |note| "\n--\n".len() + note.len());
    let Some(forwarded_body_len) = body
        .len()
        .checked_add(1)
        .and_then(|len| len.checked_add(source.body().len()))
        .and_then(|len| len.checked_add(note_bytes))
    else {
        return Err(ForwardMailError::Post(PostMailError::BodyTooLong));
    };
    if forwarded_body_len > MAX_MAIL_BODY_BYTES {
        return Err(ForwardMailError::Post(PostMailError::BodyTooLong));
    }
    body.push('\n');
    body.push_str(source.body());
    if let Some(note) = additional_note {
        body.push_str("\n--\n");
        body.push_str(&note);
    }

    let private = !matches!(source.visibility(), MailVisibility::Public);
    let subject = format!("Fwd: {}", source.subject());

    let mail = post_mail(
        user,
        msgbase,
        allowed_addressing,
        store,
        PostMailDraft {
            to_name: new_addressee_name,
            broadcast_to: BroadcastTo::None,
            addressee_slot: Some(new_addressee_slot),
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
    use crate::domain::messaging::forward_mail::{
        forward_header_for, forward_mail, ForwardMailError, ForwardMailRequest,
    };
    use crate::domain::messaging::limits::MAX_MAIL_BODY_BYTES;
    use crate::domain::messaging::mail::{BroadcastTo, Mail, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::messaging::post_mail::PostMailError;
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
        subject: &str,
        body: &str,
        visibility: MailVisibility,
        posted_at: SystemTime,
    ) -> Mail {
        store
            .insert(MailDraft {
                visibility,
                from_name: from_name.to_string(),
                to_name: "alice".to_string(),
                broadcast_to: BroadcastTo::None,
                subject: subject.to_string(),
                posted_at,
                author_slot,
                addressee_slot: Some(2),
                body: body.to_string(),
            })
            .expect("source insert")
    }

    fn source_to(
        store: &mut InMemoryMailStore,
        from_name: &str,
        author_slot: u32,
        to_name: &str,
        addressee_slot: Option<u32>,
        visibility: MailVisibility,
    ) -> Mail {
        store
            .insert(MailDraft {
                visibility,
                from_name: from_name.to_string(),
                to_name: to_name.to_string(),
                broadcast_to: BroadcastTo::None,
                subject: "private subject".to_string(),
                posted_at: t(50),
                author_slot,
                addressee_slot,
                body: "private body".to_string(),
            })
            .expect("source insert")
    }

    #[test]
    fn forward_a_public_message_creates_a_fwd_mail_in_the_same_base() {
        // Spec ForwardMail: new mail is in the same msgbase, addressed
        // to the new addressee, with `Fwd: <subject>` and a body
        // containing the forward header + original body.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "lunch tomorrow?",
            "are we still on?",
            MailVisibility::Public,
            t(50),
        );

        let forwarded = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect("happy path");

        assert_eq!(forwarded.to_name(), "bob");
        assert_eq!(forwarded.addressee_slot(), Some(3));
        assert_eq!(forwarded.broadcast_to(), BroadcastTo::None);
        assert_eq!(forwarded.from_name(), "alice");
        assert_eq!(forwarded.subject(), "Fwd: lunch tomorrow?");
        assert_eq!(forwarded.visibility(), MailVisibility::Public);
        // Body must include both the forward header and the original
        // body, joined by a newline.
        let body = forwarded.body();
        assert!(
            body.contains("From: charlie"),
            "body should carry From: header, got {body:?}"
        );
        assert!(
            body.contains("Subject: lunch tomorrow?"),
            "body should carry Subject: header, got {body:?}"
        );
        assert!(
            body.ends_with("are we still on?"),
            "body should end with the original body, got {body:?}"
        );
    }

    #[test]
    fn additional_note_is_appended_after_a_dash_separator() {
        // Spec ForwardMail body:
        //   header + "\n" + source.body
        //   + (if additional_note: "\n--\n" + additional_note else: "")
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "subj",
            "original body",
            MailVisibility::Public,
            t(50),
        );

        let forwarded = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: Some("please action this".to_string()),
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect("happy path");

        let body = forwarded.body();
        assert!(
            body.contains("original body\n--\nplease action this"),
            "expected body to end with original + separator + note, got {body:?}"
        );
        assert!(body.ends_with("please action this"), "got {body:?}");
    }

    #[test]
    fn forwarding_a_private_source_marks_the_new_mail_private() {
        // Spec ForwardMail consequent: `private: source.visibility !=
        // public`. Private (and PrivateToSysop) forwards must stay
        // private.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "subj",
            "body",
            MailVisibility::Private,
            t(50),
        );

        let forwarded = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect("happy path");

        assert_eq!(forwarded.visibility(), MailVisibility::Private);
    }

    #[test]
    fn forwarding_a_public_source_keeps_the_new_mail_public() {
        // Mirror of the previous test: a Public source must NOT
        // become Private. Pins the `!= public` direction so a
        // future refactor that inverts the condition trips here.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_from(
            &mut store,
            "charlie",
            4,
            "subj",
            "body",
            MailVisibility::Public,
            t(50),
        );

        let forwarded = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect("happy path");

        assert_eq!(forwarded.visibility(), MailVisibility::Public);
    }

    #[test]
    fn rejects_when_source_is_deleted() {
        // Spec ForwardMail: `requires: not request.source.is_deleted`.
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mut source = source_from(
            &mut store,
            "charlie",
            4,
            "subj",
            "body",
            MailVisibility::Public,
            t(50),
        );
        source
            .transition_to(MailVisibility::Deleted)
            .expect("transition to Deleted");

        let err = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect_err("must refuse a deleted source");
        assert!(
            matches!(err, ForwardMailError::SourceDeleted),
            "got {err:?}"
        );
        assert_eq!(store.highest_message(), 1);
        assert_eq!(alice.messages_posted(), 0);
    }

    #[test]
    fn rejects_private_source_the_user_cannot_read() {
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let source = source_to(
            &mut store,
            "charlie",
            4,
            "bob",
            Some(3),
            MailVisibility::Private,
        );

        let err = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "alice".to_string(),
                new_addressee_slot: 2,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect_err("forward must not leak someone else's private mail");

        assert!(
            matches!(err, ForwardMailError::SourceNotPermitted),
            "got {err:?}"
        );
        assert_eq!(store.highest_message(), 1);
        assert_eq!(alice.messages_posted(), 0);
    }

    #[test]
    fn rejects_forward_when_the_derived_body_would_exceed_the_size_limit() {
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let huge_body = "x".repeat(MAX_MAIL_BODY_BYTES);
        let source = source_from(
            &mut store,
            "alice",
            2,
            "huge",
            &huge_body,
            MailVisibility::Public,
            t(50),
        );

        let err = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect_err("forward header must not push body past the limit");

        assert!(
            matches!(err, ForwardMailError::Post(PostMailError::BodyTooLong)),
            "got {err:?}"
        );
        assert_eq!(store.highest_message(), 1);
        assert_eq!(alice.messages_posted(), 0);
    }

    #[test]
    fn accepts_forward_when_the_derived_body_exactly_hits_the_size_limit() {
        let mut alice = make_user(2, "alice");
        let msgbase = MessageBaseRef::new(2, 1);
        let mut header_store = InMemoryMailStore::new(msgbase);
        let header_source = source_from(
            &mut header_store,
            "alice",
            2,
            "exact",
            "",
            MailVisibility::Public,
            t(50),
        );
        let note = "ok";
        let source_body_len = MAX_MAIL_BODY_BYTES
            - forward_header_for(&header_source).len()
            - 1
            - "\n--\n".len()
            - note.len();

        let mut store = InMemoryMailStore::new(msgbase);
        let source_body = "x".repeat(source_body_len);
        let source = source_from(
            &mut store,
            "alice",
            2,
            "exact",
            &source_body,
            MailVisibility::Public,
            t(50),
        );

        let forwarded = forward_mail(
            &mut alice,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: Some(note.to_string()),
                from_name: "alice".to_string(),
                posted_at: t(100),
            },
        )
        .expect("derived body exactly at limit should be accepted");

        assert_eq!(forwarded.body().len(), MAX_MAIL_BODY_BYTES);
        assert_eq!(store.highest_message(), 2);
        assert_eq!(alice.messages_posted(), 1);
    }

    #[test]
    fn propagates_post_mail_no_membership_error() {
        // ForwardMail expands to PostMail; the underlying gates
        // bubble up under the `Post(_)` wrapper.
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
            "subj",
            "body",
            MailVisibility::Public,
            t(50),
        );

        let err = forward_mail(
            &mut stranger,
            msgbase,
            AllowedAddressing::Any,
            &mut store,
            &source,
            ForwardMailRequest {
                new_addressee_name: "bob".to_string(),
                new_addressee_slot: 3,
                additional_note: None,
                from_name: "stranger".to_string(),
                posted_at: t(100),
            },
        )
        .expect_err("no membership for conference");
        assert!(
            matches!(err, ForwardMailError::Post(PostMailError::NoMembership)),
            "got {err:?}"
        );
    }

    #[test]
    fn forward_header_carries_from_date_and_subject() {
        // Spec black box `forward_header_for(mail)` — the block is
        // prepended to a forward's body. We pin the format so a
        // future change to the header layout is a deliberate
        // decision rather than silent drift.
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mail = source_from(
            &mut store,
            "charlie",
            4,
            "lunch",
            "are we on?",
            MailVisibility::Public,
            t(50),
        );

        let header = forward_header_for(&mail);
        assert!(header.contains("From: charlie\n"), "got {header:?}");
        assert!(
            header.contains("Date: 1970-01-01T00:00:50Z\n"),
            "got {header:?}"
        );
        assert!(header.contains("Subject: lunch\n"), "got {header:?}");
    }
}
