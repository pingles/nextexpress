//! [`attach_file_to_mail`] rule
//! (spec: `messaging.allium:AttachFileToMail`).
//!
//! Phase 8, Slice 48. A pure-domain rule that records a
//! [`MailAttachment`] row on an existing [`Mail`].
//!
//! The spec's two attachment paths — pre-upload (file then
//! message) and post-upload (message then file) — both converge
//! on the same call: the file lives in the file area (Phase 9)
//! and is referenced by name; this rule only persists the
//! metadata binding. Wire transfer of the underlying bytes is
//! Phase 10's job.

use crate::domain::bytes::Bytes;
use crate::domain::messaging::mail::{Mail, MailAttachment, MailVisibility};
use crate::domain::user::{Right, User};

/// Errors raised by [`attach_file_to_mail`]. Each variant
/// corresponds to one of `messaging.allium:AttachFileToMail`'s
/// `requires` clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttachFileToMailError {
    /// `requires: session.user = mail.author or session.user.is_sysop`
    /// — only the original author or the sysop may attach files to
    /// a posted message.
    #[error("user is neither the mail's author nor the sysop")]
    NotAuthorOrSysop,
    /// `requires: not mail.is_deleted` — a soft-deleted mail
    /// refuses further mutation.
    #[error("mail is deleted")]
    Deleted,
    /// `requires: has_access(session.user, attach_files)` — the
    /// user lacks the [`Right::AttachFiles`] grant.
    #[error("user lacks the attach_files right")]
    AccessDenied,
}

/// Applies `messaging.allium:AttachFileToMail` to `(user, mail,
/// file_name, file_size)`.
///
/// On success a fresh [`MailAttachment`] row is appended to
/// [`Mail::attachments`].
///
/// # Errors
/// Returns the matching [`AttachFileToMailError`] when any of the
/// rule's `requires` clauses fail. No state is mutated on error.
pub fn attach_file_to_mail(
    user: &User,
    mail: &mut Mail,
    file_name: String,
    file_size: Bytes,
) -> Result<(), AttachFileToMailError> {
    if !user.is_sysop() && user.slot_number() != mail.author_slot() {
        return Err(AttachFileToMailError::NotAuthorOrSysop);
    }
    if matches!(mail.visibility(), MailVisibility::Deleted) {
        return Err(AttachFileToMailError::Deleted);
    }
    if !user.has_access(Right::AttachFiles) {
        return Err(AttachFileToMailError::AccessDenied);
    }
    mail.push_attachment(MailAttachment::new(file_name, file_size));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::domain::bytes::Bytes;
    use crate::domain::conference::{ConferenceMembership, MessageBaseRef};
    use crate::domain::messaging::attach_file_to_mail::{
        attach_file_to_mail, AttachFileToMailError,
    };
    use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::{Right, User};

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

    fn make_mail(author_slot: u32) -> Mail {
        Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "alice".to_string(),
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "subj".to_string(),
            posted_at: t(50),
            author_slot,
            addressee_slot: Some(3),
            body: "body".to_string(),
        })
    }

    #[test]
    fn author_can_attach_a_file_to_their_own_mail() {
        // Spec AttachFileToMail: when session.user = mail.author and
        // the user has the AttachFiles right and the mail is not
        // deleted, a new MailAttachment row is added with the given
        // file_name and file_size.
        let author = make_user(2);
        assert!(author.has_access(Right::AttachFiles));
        let mut mail = make_mail(author.slot_number());

        attach_file_to_mail(
            &author,
            &mut mail,
            "report.txt".to_string(),
            Bytes::new(1_234),
        )
        .expect("happy path");

        assert_eq!(mail.attachments().len(), 1);
        assert_eq!(mail.attachments()[0].file_name(), "report.txt");
        assert_eq!(mail.attachments()[0].file_size(), Bytes::new(1_234));
    }

    #[test]
    fn sysop_can_attach_a_file_to_someone_elses_mail() {
        // Spec selector: `session.user = mail.author or
        // session.user.is_sysop`. The sysop bypasses the
        // author check.
        let sysop = make_user(1);
        assert!(sysop.is_sysop());
        let mut mail = make_mail(4); // authored by someone else

        attach_file_to_mail(&sysop, &mut mail, "log.txt".to_string(), Bytes::new(42))
            .expect("sysop may attach to others' mail");

        assert_eq!(mail.attachments().len(), 1);
    }

    #[test]
    fn second_attachment_appends_to_existing_row() {
        // The spec's `attachments: MailAttachment with mail = this`
        // is a *collection*; consecutive attachments accumulate.
        let author = make_user(2);
        let mut mail = make_mail(author.slot_number());

        attach_file_to_mail(&author, &mut mail, "a.txt".to_string(), Bytes::new(1)).unwrap();
        attach_file_to_mail(&author, &mut mail, "b.bin".to_string(), Bytes::new(2)).unwrap();

        assert_eq!(mail.attachments().len(), 2);
        assert_eq!(mail.attachments()[0].file_name(), "a.txt");
        assert_eq!(mail.attachments()[1].file_name(), "b.bin");
    }

    #[test]
    fn rejects_a_non_author_non_sysop() {
        // Spec requires: `session.user = mail.author or
        // session.user.is_sysop`. A bystander can't attach.
        let bystander = make_user(7);
        assert!(!bystander.is_sysop());
        let mut mail = make_mail(4); // authored by someone else

        let err = attach_file_to_mail(
            &bystander,
            &mut mail,
            "sneaky.txt".to_string(),
            Bytes::new(0),
        )
        .expect_err("non-author non-sysop is rejected");
        assert_eq!(err, AttachFileToMailError::NotAuthorOrSysop);
        assert!(mail.attachments().is_empty());
    }

    #[test]
    fn rejects_when_mail_is_deleted() {
        // Spec requires: `not mail.is_deleted`. Deleted mail is
        // immutable from this rule's perspective.
        let author = make_user(2);
        let mut mail = make_mail(author.slot_number());
        mail.transition_to(MailVisibility::Deleted).unwrap();

        let err = attach_file_to_mail(&author, &mut mail, "f.txt".to_string(), Bytes::new(1))
            .expect_err("deleted mail refuses attachments");
        assert_eq!(err, AttachFileToMailError::Deleted);
        assert!(mail.attachments().is_empty());
    }

    #[test]
    fn rejects_when_user_lacks_attach_files_right() {
        // Spec requires: `has_access(user, attach_files)`. A
        // pending-validation new user holds only ReadMessage and
        // CommentToSysop — AttachFiles is denied even when they
        // try to attach to a mail they themselves authored.
        let mut new_user = User::register_new(
            9,
            crate::domain::user::NewUserDraft {
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
            },
        )
        .expect("valid");
        new_user.upsert_membership(ConferenceMembership::new(2, true));
        assert!(!new_user.has_access(Right::AttachFiles));
        let mut mail = make_mail(new_user.slot_number());

        let err = attach_file_to_mail(&new_user, &mut mail, "f.txt".to_string(), Bytes::new(1))
            .expect_err("missing AttachFiles right is rejected");
        assert_eq!(err, AttachFileToMailError::AccessDenied);
        assert!(mail.attachments().is_empty());
    }
}
