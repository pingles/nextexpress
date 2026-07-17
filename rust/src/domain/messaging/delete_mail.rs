//! [`delete_mail`] rule (spec: `messaging.allium:DeleteMail`).
//!
//! Phase 8, Slice 49. Soft-deletes a mail via
//! [`Mail::soft_delete`] (which owns the visibility transition and
//! the spec's `for a in mail.attachments: not exists a`
//! consequent), and persists the result. The
//! `lowest_undeleted_message` bump is observed via the store's
//! derived getter — every read recomputes from the live
//! visibility set, so the consequent is automatically honoured.

use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::mail_store::{MailStore, MailStoreError};
use crate::domain::user::User;

/// Access threshold (spec: `session.user.access_level >= 210`)
/// that grants a non-sysop the right to delete any mail.
const DELETE_ANY_ACCESS_LEVEL: u8 = 210;

/// Errors raised by [`delete_mail`].
#[derive(Debug, thiserror::Error)]
pub enum DeleteMailError {
    /// No mail at `number` exists in the store.
    #[error("no mail at number {0}")]
    NotFound(u32),
    /// `requires: not mail.is_deleted` — the mail is already
    /// soft-deleted.
    #[error("mail is already deleted")]
    AlreadyDeleted,
    /// None of the four delete-permission disjuncts (sysop,
    /// `access_level >= 210`, author, addressee) hold for `user`.
    #[error("user lacks delete permission for this mail")]
    NotPermitted,
    /// The underlying [`MailStore`] rejected the read/write.
    #[error(transparent)]
    Store(#[from] MailStoreError),
}

/// Applies `messaging.allium:DeleteMail` to `(user, store, number)`.
///
/// Looks up the mail by `number` in the bound store, checks the
/// spec's `requires` clauses, applies [`Mail::soft_delete`]
/// (visibility to deleted, attachments stripped), and persists
/// the row.
///
/// # Errors
/// Returns the matching [`DeleteMailError`] variant when a
/// `requires` gate fails or storage rejects the operation. No
/// state is mutated on error.
///
/// # Panics
/// Panics if [`Mail::soft_delete`] is rejected — structurally
/// unreachable because the `not deleted` gate above already
/// filtered the only state (`Deleted`) it errors on.
pub fn delete_mail(
    user: &User,
    store: &mut dyn MailStore,
    number: u32,
) -> Result<(), DeleteMailError> {
    let Some(mut mail) = store.load(number)? else {
        return Err(DeleteMailError::NotFound(number));
    };
    if mail.is_deleted() {
        return Err(DeleteMailError::AlreadyDeleted);
    }
    if !can_delete(user, &mail) {
        return Err(DeleteMailError::NotPermitted);
    }
    // Spec consequents (visibility = deleted, attachments stripped,
    // received_at cascade-cleared) are owned by `Mail::soft_delete`.
    mail.soft_delete()
        .expect("already-deleted gate above filters Deleted");
    store.save(&mail)?;
    Ok(())
}

/// True when `user` may delete `mail` — the four spec disjuncts
/// (`messaging.allium:DeleteMail`): the sysop, an `access_level >= 210`
/// caller, the message's author, or its addressee. The legacy
/// `readMSG` sub-prompt uses this to gate the `D`elete option (it is
/// the `ACS_DELETE_MESSAGE` check at `express.e:12148` expressed in
/// `NextExpress`'s per-message permission model).
#[must_use]
pub fn can_delete(user: &User, mail: &Mail) -> bool {
    user.is_sysop()
        || user.access_level() >= DELETE_ANY_ACCESS_LEVEL
        || mail.author_slot() == user.slot_number()
        || mail.addressee_slot() == Some(user.slot_number())
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use crate::domain::bytes::Bytes;
    use crate::domain::conference::{ConferenceMembership, MessageBaseRef};
    use crate::domain::messaging::delete_mail::{delete_mail, DeleteMailError};
    use crate::domain::messaging::mail::{
        BroadcastTo, Mail, MailAttachment, MailDraft, MailVisibility,
    };
    use crate::domain::messaging::mail_store::test_support::{make_user, t, InMemoryMailStore};
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::User;

    fn insert_sample(store: &mut InMemoryMailStore, author_slot: u32, addressee_slot: u32) -> Mail {
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: format!("user{author_slot}"),
                to_name: format!("user{addressee_slot}"),
                broadcast_to: BroadcastTo::None,
                subject: "subj".to_string(),
                posted_at: t(50),
                author_slot,
                addressee_slot: Some(addressee_slot),
                body: "body".to_string(),
            })
            .expect("insert")
    }

    #[test]
    fn author_can_delete_their_own_mail() {
        // Spec DeleteMail: author qualifies via the
        // `mail.author = session.user` disjunct. Visibility goes
        // to deleted; the row is persisted.
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, author.slot_number(), 3);

        delete_mail(&author, &mut store, mail.number()).expect("happy path");

        let reloaded = store.load(mail.number()).unwrap().expect("present");
        assert_eq!(reloaded.visibility(), MailVisibility::Deleted);
    }

    #[test]
    fn addressee_can_delete_a_mail_addressed_to_them() {
        // Spec disjunct: `mail.addressee = session.user`.
        let addressee = make_user(3);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, 2, addressee.slot_number());

        delete_mail(&addressee, &mut store, mail.number()).expect("happy path");

        assert_eq!(
            store.load(mail.number()).unwrap().unwrap().visibility(),
            MailVisibility::Deleted,
        );
    }

    #[test]
    fn sysop_can_delete_any_mail() {
        // Spec disjunct: `session.user.is_sysop`. The sysop owns
        // the highest unprivileged tier and may flatten any
        // message base.
        let sysop = make_user(1);
        assert!(sysop.is_sysop());
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, 4, 5); // someone else's mail

        delete_mail(&sysop, &mut store, mail.number()).expect("sysop bypasses");

        assert_eq!(
            store.load(mail.number()).unwrap().unwrap().visibility(),
            MailVisibility::Deleted,
        );
    }

    #[test]
    fn access_level_210_can_delete_any_mail() {
        // Spec disjunct: `session.user.access_level >= 210`. A
        // co-sysop with the 210-tier may delete any mail without
        // being the sysop themselves.
        let mut co_sysop = User::new(
            7,
            "co".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            210,
        )
        .expect("valid");
        co_sysop.upsert_membership(ConferenceMembership::new(2, true));
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, 4, 5);

        delete_mail(&co_sysop, &mut store, mail.number()).expect("210 bypasses");

        assert_eq!(
            store.load(mail.number()).unwrap().unwrap().visibility(),
            MailVisibility::Deleted,
        );
    }

    #[test]
    fn bystander_cannot_delete_someone_elses_mail() {
        // None of the four disjuncts (sysop / 210 / author /
        // addressee) hold for a bystander.
        let bystander = make_user(7);
        assert!(!bystander.is_sysop());
        assert!(bystander.access_level() < 210);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, 4, 5);

        let err = delete_mail(&bystander, &mut store, mail.number())
            .expect_err("bystander has no permission");
        assert!(matches!(err, DeleteMailError::NotPermitted), "got {err:?}");
        assert_eq!(
            store.load(mail.number()).unwrap().unwrap().visibility(),
            MailVisibility::Public, // still public — no state change
        );
    }

    #[test]
    fn rejects_when_mail_is_already_deleted() {
        // Spec requires: `not mail.is_deleted`.
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mail = insert_sample(&mut store, author.slot_number(), 3);
        delete_mail(&author, &mut store, mail.number()).unwrap();

        let err =
            delete_mail(&author, &mut store, mail.number()).expect_err("second delete is rejected");
        assert!(
            matches!(err, DeleteMailError::AlreadyDeleted),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_when_mail_does_not_exist() {
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);

        let err = delete_mail(&author, &mut store, 99).expect_err("missing mail");
        assert!(matches!(err, DeleteMailError::NotFound(99)), "got {err:?}");
    }

    #[test]
    fn delete_strips_attachments_from_the_persisted_mail() {
        // Spec consequent: `for a in mail.attachments: not exists a`.
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let mut mail = insert_sample(&mut store, author.slot_number(), 3);
        mail.push_attachment(MailAttachment::new("a.txt".to_string(), Bytes::new(1)));
        mail.push_attachment(MailAttachment::new("b.txt".to_string(), Bytes::new(2)));
        store.save(&mail).unwrap();
        assert_eq!(
            store
                .load(mail.number())
                .unwrap()
                .unwrap()
                .attachments()
                .len(),
            2
        );

        delete_mail(&author, &mut store, mail.number()).expect("happy path");

        let reloaded = store.load(mail.number()).unwrap().unwrap();
        assert!(reloaded.attachments().is_empty());
        assert_eq!(reloaded.visibility(), MailVisibility::Deleted);
    }

    #[test]
    fn lowest_undeleted_message_bumps_when_the_lowest_is_deleted() {
        // Spec consequent: if `mail.msgbase.lowest_undeleted_message =
        // mail.number`, advance past it. Starts at 1 for a fresh
        // store; after deleting #1 it should advance to #2 (or
        // beyond, if subsequent messages are also deleted).
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let m1 = insert_sample(&mut store, author.slot_number(), 3);
        let _m2 = insert_sample(&mut store, author.slot_number(), 3);
        let _m3 = insert_sample(&mut store, author.slot_number(), 3);
        assert_eq!(store.lowest_undeleted_message().expect("lowest"), 1);

        delete_mail(&author, &mut store, m1.number()).expect("happy path");

        assert_eq!(store.lowest_undeleted_message().expect("lowest"), 2);
    }

    #[test]
    fn lowest_undeleted_message_skips_consecutive_deletes() {
        // After deleting #1 and #2 the pointer should jump to #3.
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let m1 = insert_sample(&mut store, author.slot_number(), 3);
        let m2 = insert_sample(&mut store, author.slot_number(), 3);
        let _m3 = insert_sample(&mut store, author.slot_number(), 3);

        delete_mail(&author, &mut store, m1.number()).unwrap();
        delete_mail(&author, &mut store, m2.number()).unwrap();

        assert_eq!(store.lowest_undeleted_message().expect("lowest"), 3);
    }

    #[test]
    fn lowest_undeleted_message_stays_put_when_non_lowest_is_deleted() {
        // Deleting #2 while #1 is alive must not move the pointer.
        let author = make_user(2);
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(msgbase);
        let _m1 = insert_sample(&mut store, author.slot_number(), 3);
        let m2 = insert_sample(&mut store, author.slot_number(), 3);

        delete_mail(&author, &mut store, m2.number()).unwrap();

        assert_eq!(store.lowest_undeleted_message().expect("lowest"), 1);
    }
}
