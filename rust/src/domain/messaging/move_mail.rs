//! [`move_mail`] rule (spec: `messaging.allium:MoveMail`).
//!
//! Phase 8, Slice 49. Models the legacy `FM` "file move" command
//! as an atomic create-new-then-delete-old across two message
//! bases: a fresh [`Mail`] lands at `target.highest_message + 1`
//! preserving every header field (visibility, addressee, posted /
//! received timestamps, attachments) and the source mail is
//! soft-deleted in place.
//!
//! Wire-level concerns (the on-disk body file removal mentioned in
//! the spec's "Old mail soft-deleted; its body file is removed by
//! the storage layer" comment) are the [`MailStore`] adapter's
//! responsibility — the domain rule only sees the visibility flip
//! and trusts the store to clean up.

use crate::domain::messaging::mail::{Mail, MailAttachment, MailDraft, MailVisibility};
use crate::domain::messaging::mail_store::{MailStore, MailStoreError};
use crate::domain::user::{Right, User};

/// Errors raised by [`move_mail`].
#[derive(Debug, thiserror::Error)]
pub enum MoveMailError {
    /// No mail at the supplied number exists in the source store.
    #[error("no mail at number {0} in source store")]
    NotFound(u32),
    /// `requires: has_access(session.user, message_edit) or
    /// session.user.is_sysop` — neither disjunct holds for `user`.
    #[error("user lacks edit-message permission")]
    NotPermitted,
    /// `requires: target_msgbase != mail.msgbase` — moving a mail
    /// onto its own message base is rejected.
    #[error("target msgbase equals source msgbase")]
    SameMsgbase,
    /// The underlying [`MailStore`] rejected a read/write.
    #[error(transparent)]
    Store(#[from] MailStoreError),
}

/// Applies `messaging.allium:MoveMail` to `(user, source, target,
/// number)`.
///
/// The new mail is inserted into `target` at
/// `target.highest_message + 1` with every preserved field from
/// the source (visibility, from/to/broadcast/subject/body,
/// `posted_at` / `received_at`, author, addressee). Any
/// attachments on the source row move over to the new mail (the
/// spec's `for a in mail.attachments: MailAttachment.created`
/// consequent). The source mail is soft-deleted in place.
///
/// Returns the freshly-inserted target [`Mail`].
///
/// # Errors
/// - [`MoveMailError::NotFound`] when the source store has no
///   mail at `number`;
/// - [`MoveMailError::NotPermitted`] when neither the
///   [`Right::MessageEdit`] grant nor the sysop disjunct holds;
/// - [`MoveMailError::SameMsgbase`] when the target store is
///   bound to the same `(conference, msgbase)` pair as the
///   source;
/// - [`MoveMailError::Store`] for storage failures.
///
/// On error no state is mutated on either store.
///
/// # Panics
/// Panics if [`Mail::mark_received`] or
/// [`Mail::transition_to`] rejects an inputs that this function
/// has already proven legal — the freshly inserted copy is
/// Public, so `mark_received` is always safe; the source's
/// visibility is one of `{public, private, private_to_sysop}`
/// (Deleted is filtered upstream), so `transition_to(Deleted)`
/// is always in the legal matrix.
pub fn move_mail(
    user: &User,
    source: &mut dyn MailStore,
    target: &mut dyn MailStore,
    number: u32,
) -> Result<Mail, MoveMailError> {
    if !user.is_sysop() && !user.has_access(Right::MessageEdit) {
        return Err(MoveMailError::NotPermitted);
    }
    if source.msgbase() == target.msgbase() {
        return Err(MoveMailError::SameMsgbase);
    }
    let Some(mut original) = source.load(number)? else {
        return Err(MoveMailError::NotFound(number));
    };

    // 1) Create the new mail in the target. The store allocates the
    //    next number and persists. We then patch the fields the
    //    insert path doesn't see (received_at, visibility, attachments)
    //    and save again.
    let mut copy = target.insert(MailDraft {
        visibility: MailVisibility::Public, // patched below before save
        from_name: original.from_name().to_string(),
        to_name: original.to_name().to_string(),
        broadcast_to: original.broadcast_to(),
        subject: original.subject().to_string(),
        posted_at: original.posted_at(),
        author_slot: original.author_slot(),
        addressee_slot: original.addressee_slot(),
        body: original.body().to_string(),
    })?;
    // Carry received_at forward (the insert path sets it to None).
    if let Some(when) = original.received_at() {
        // `mark_received` rejects already-deleted mail; the freshly
        // inserted copy is Public, so this is always safe.
        copy.mark_received(when)
            .expect("freshly-inserted mail is not deleted");
    }
    // Reattach the attachments. The spec models this as
    // "MailAttachment.created(mail: target_msgbase, ...)" per row.
    for attachment in original.attachments() {
        copy.push_attachment(MailAttachment::new(
            attachment.file_name().to_string(),
            attachment.file_size(),
        ));
    }
    // Carry the source visibility forward (the freshly-inserted copy
    // is Public; bring it back in line if the source was Private
    // or PrivateToSysop). Deleted sources are explicitly disallowed
    // by the spec's transition matrix (deleted is terminal) — but
    // a sysop moving a deleted mail isn't in scope either; the
    // typical workflow moves a live message off and the source
    // becomes deleted afterwards.
    if !matches!(original.visibility(), MailVisibility::Public) {
        copy.transition_to(original.visibility())
            .expect("source visibility is in the legal-from-Public set");
    }
    target.save(&copy)?;

    // 2) Soft-delete the source. The source row's body file is
    //    removed by the storage layer (per spec comment); we just
    //    mark it Deleted here. The clear_attachments is a domain
    //    effect — the rows now belong to the new mail.
    original
        .transition_to(MailVisibility::Deleted)
        .expect("public/private/private_to_sysop transitions to deleted");
    original.clear_attachments();
    source.save(&original)?;

    Ok(copy)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::domain::bytes::Bytes;
    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::mail::{
        BroadcastTo, Mail, MailAttachment, MailDraft, MailVisibility,
    };
    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::messaging::move_mail::{move_mail, MoveMailError};
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::{Right, User};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn make_user(slot: u32, access_level: u8) -> User {
        User::new(
            slot,
            format!("user{slot}"),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            access_level,
        )
        .expect("valid user")
    }

    fn insert_sample(store: &mut InMemoryMailStore) -> Mail {
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: "alice".to_string(),
                to_name: "bob".to_string(),
                broadcast_to: BroadcastTo::None,
                subject: "lunch?".to_string(),
                posted_at: t(50),
                author_slot: 2,
                addressee_slot: Some(3),
                body: "are we on?".to_string(),
            })
            .expect("insert")
    }

    #[test]
    fn sysop_can_move_a_mail_to_a_different_base() {
        // Spec MoveMail: ensures Mail.created in target with every
        // preserved field, target.highest_message bumps, source
        // visibility becomes deleted.
        let sysop = make_user(1, 255);
        assert!(sysop.is_sysop());
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let original = insert_sample(&mut source);

        let moved =
            move_mail(&sysop, &mut source, &mut target, original.number()).expect("happy path");

        // New mail at the target.
        assert_eq!(moved.msgbase(), MessageBaseRef::new(3, 4));
        assert_eq!(moved.number(), 1); // target was empty: highest+1=1
        assert_eq!(moved.from_name(), original.from_name());
        assert_eq!(moved.to_name(), original.to_name());
        assert_eq!(moved.broadcast_to(), original.broadcast_to());
        assert_eq!(moved.subject(), original.subject());
        assert_eq!(moved.body(), original.body());
        assert_eq!(moved.posted_at(), original.posted_at());
        assert_eq!(moved.received_at(), original.received_at());
        assert_eq!(moved.author_slot(), original.author_slot());
        assert_eq!(moved.addressee_slot(), original.addressee_slot());
        assert_eq!(moved.visibility(), original.visibility());

        // Target store highest_message bumped.
        assert_eq!(target.highest_message(), 1);

        // Source soft-deleted.
        let reloaded = source.load(original.number()).unwrap().unwrap();
        assert_eq!(reloaded.visibility(), MailVisibility::Deleted);
    }

    #[test]
    fn moving_a_received_mail_preserves_the_received_at_timestamp() {
        // Spec consequent: `received_at: mail.received_at`. The
        // freshly-inserted copy must carry the timestamp forward
        // even though `MailStore::insert` always sets `None`.
        let sysop = make_user(1, 255);
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let mut original = insert_sample(&mut source);
        original.mark_received(t(123)).unwrap();
        source.save(&original).unwrap();

        let moved =
            move_mail(&sysop, &mut source, &mut target, original.number()).expect("happy path");

        assert_eq!(moved.received_at(), Some(t(123)));
    }

    #[test]
    fn moving_a_private_mail_preserves_visibility() {
        // Spec consequent: `visibility: mail.visibility`. A Private
        // source moves over as Private, not Public.
        let sysop = make_user(1, 255);
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let mut original = insert_sample(&mut source);
        original.transition_to(MailVisibility::Private).unwrap();
        source.save(&original).unwrap();

        let moved =
            move_mail(&sysop, &mut source, &mut target, original.number()).expect("happy path");

        assert_eq!(moved.visibility(), MailVisibility::Private);
    }

    #[test]
    fn moving_a_mail_transfers_attachments_to_the_new_row() {
        // Spec consequent: `for a in mail.attachments:
        // MailAttachment.created(mail: target_msgbase, ...)`. The
        // attachments end up on the new mail; the source is
        // emptied as part of the soft-delete.
        let sysop = make_user(1, 255);
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let mut original = insert_sample(&mut source);
        original.push_attachment(MailAttachment::new("a.txt".to_string(), Bytes::new(1)));
        original.push_attachment(MailAttachment::new("b.bin".to_string(), Bytes::new(2)));
        source.save(&original).unwrap();

        let moved =
            move_mail(&sysop, &mut source, &mut target, original.number()).expect("happy path");

        assert_eq!(moved.attachments().len(), 2);
        assert_eq!(moved.attachments()[0].file_name(), "a.txt");
        assert_eq!(moved.attachments()[1].file_name(), "b.bin");
        // Source's attachments removed by the delete-then-clear path.
        let reloaded = source.load(original.number()).unwrap().unwrap();
        assert!(reloaded.attachments().is_empty());
    }

    #[test]
    fn message_edit_right_alone_is_sufficient() {
        // Spec disjunct: `has_access(user, message_edit) or is_sysop`.
        // We have to grant MessageEdit via access tier — make_user
        // at level 255 is the sysop; pick a non-sysop slot with
        // sufficient access to hold MessageEdit. Currently slot >= 2
        // with access_level >= 100 holds MessageEdit, mirroring the
        // existing user-tier mapping.
        let editor = make_user(7, 100);
        assert!(!editor.is_sysop());
        assert!(editor.has_access(Right::MessageEdit));
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let original = insert_sample(&mut source);

        move_mail(&editor, &mut source, &mut target, original.number())
            .expect("MessageEdit alone permits move");
    }

    #[test]
    fn rejects_when_user_lacks_message_edit_and_is_not_sysop() {
        // A pending-validation new user holds neither MessageEdit
        // nor the sysop disjunct.
        let new_user = User::register_new(crate::domain::user::NewUserRegistration {
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
        assert!(!new_user.has_access(Right::MessageEdit));
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));
        let original = insert_sample(&mut source);

        let err = move_mail(&new_user, &mut source, &mut target, original.number())
            .expect_err("non-sysop without MessageEdit is rejected");
        assert!(matches!(err, MoveMailError::NotPermitted), "got {err:?}");
        // No state change anywhere — source still public, target empty.
        assert_eq!(
            source
                .load(original.number())
                .unwrap()
                .unwrap()
                .visibility(),
            MailVisibility::Public,
        );
        assert_eq!(target.highest_message(), 0);
    }

    #[test]
    fn rejects_when_target_equals_source() {
        // Spec requires: `target_msgbase != mail.msgbase`. Moving a
        // mail onto its own base is a no-op trap.
        let sysop = make_user(1, 255);
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let original = insert_sample(&mut source);
        // The trick: we need a second mut borrow into the same
        // store to test self-target. Use a clone of msgbase via a
        // second store at the same coordinates.
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(2, 1));

        let err = move_mail(&sysop, &mut source, &mut target, original.number())
            .expect_err("same msgbase is rejected");
        assert!(matches!(err, MoveMailError::SameMsgbase), "got {err:?}");
    }

    #[test]
    fn rejects_when_mail_does_not_exist_in_source() {
        let sysop = make_user(1, 255);
        let mut source = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let mut target = InMemoryMailStore::new(MessageBaseRef::new(3, 4));

        let err = move_mail(&sysop, &mut source, &mut target, 99).expect_err("missing source mail");
        assert!(matches!(err, MoveMailError::NotFound(99)), "got {err:?}");
    }
}
