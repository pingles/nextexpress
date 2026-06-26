//! [`edit_mail_header`] rule (spec:
//! `messaging.allium:EditMailHeader`).
//!
//! Phase 8, Slice 49. Sysop-only (or `access_level >= 210`)
//! rewrite of an existing mail's subject and/or addressee. The
//! addressee resolution itself is a black box; the caller supplies
//! the pre-resolved `(to_name, addressee_slot)` pair derived from
//! `lookup_user_by_name(new_to_name, conference.accepted_name_type)`.

use crate::domain::messaging::mail_store::{MailStore, MailStoreError};
use crate::domain::user::User;

/// Access threshold (spec: `session.user.access_level >= 210`).
const EDIT_HEADER_ACCESS_LEVEL: u8 = 210;

/// Errors raised by [`edit_mail_header`].
#[derive(Debug, thiserror::Error)]
pub enum EditMailHeaderError {
    /// No mail at the supplied number exists in the store.
    #[error("no mail at number {0}")]
    NotFound(u32),
    /// Neither the sysop disjunct nor the `access_level >= 210`
    /// disjunct holds for `user`.
    #[error("user lacks edit-header permission")]
    NotPermitted,
    /// The underlying [`MailStore`] rejected the read/write.
    #[error(transparent)]
    Store(#[from] MailStoreError),
}

/// Applies `messaging.allium:EditMailHeader` to `(user, store,
/// mail_number, new_subject, new_to)`.
///
/// `new_subject` and `new_to` are both optional; the spec's `if
/// X != null: ...` consequents are honoured independently — a
/// caller may rewrite just the subject, just the addressee, or
/// both at once. `new_to`'s `(to_name, addressee_slot)` pair is
/// the caller's pre-resolved equivalent of the spec's
/// `lookup_user_by_name`. The `addressee_slot` is `None` when the
/// new `to_name` is a broadcast keyword (`"ALL"` / `"EALL"`); the
/// rule does not re-validate the broadcast/addressee invariant in
/// either direction — sysop edits are expected to be deliberate.
///
/// # Errors
/// Returns [`EditMailHeaderError::NotFound`] when no mail at the
/// supplied number exists; [`EditMailHeaderError::NotPermitted`]
/// when the access disjunct fails; [`EditMailHeaderError::Store`]
/// for storage failures. No state is mutated on error.
pub fn edit_mail_header(
    user: &User,
    store: &mut dyn MailStore,
    mail_number: u32,
    new_subject: Option<String>,
    new_to: Option<(String, Option<u32>)>,
) -> Result<(), EditMailHeaderError> {
    if !can_edit_header(user) {
        return Err(EditMailHeaderError::NotPermitted);
    }
    let Some(mut mail) = store.load(mail_number)? else {
        return Err(EditMailHeaderError::NotFound(mail_number));
    };
    if let Some(subject) = new_subject {
        mail.set_subject(subject);
    }
    if let Some((to_name, slot)) = new_to {
        mail.set_addressee(to_name, slot);
    }
    store.save(&mail)?;
    Ok(())
}

/// True when `user` may edit a message header — the spec disjunct
/// `session.user.is_sysop or session.user.access_level >= 210`. The
/// legacy `readMSG` sub-prompt uses this to gate the `EH` option (the
/// `ACS_MESSAGE_EDIT` check at `express.e:12179`, expressed in
/// `NextExpress`'s access-tier model).
#[must_use]
pub fn can_edit_header(user: &User) -> bool {
    user.is_sysop() || user.access_level() >= EDIT_HEADER_ACCESS_LEVEL
}

#[cfg(test)]
mod tests {

    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::edit_mail_header::{edit_mail_header, EditMailHeaderError};
    use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::test_support::{
        make_user_with_level, t, InMemoryMailStore,
    };
    use crate::domain::messaging::mail_store::MailStore;

    fn insert_sample(store: &mut InMemoryMailStore) -> u32 {
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: "alice".to_string(),
                to_name: "bob".to_string(),
                broadcast_to: BroadcastTo::None,
                subject: "old subject".to_string(),
                posted_at: t(50),
                author_slot: 2,
                addressee_slot: Some(3),
                body: "body".to_string(),
            })
            .expect("insert")
            .number()
    }

    #[test]
    fn sysop_can_rewrite_subject() {
        // Spec EditMailHeader: `if new_subject != null: mail.subject
        // = new_subject`. Sysop is allowed by the access disjunct.
        let sysop = make_user_with_level(1, 255);
        assert!(sysop.is_sysop());
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let number = insert_sample(&mut store);

        edit_mail_header(
            &sysop,
            &mut store,
            number,
            Some("new subject".to_string()),
            None,
        )
        .expect("happy path");

        let reloaded = store.load(number).unwrap().unwrap();
        assert_eq!(reloaded.subject(), "new subject");
        // The other fields are untouched.
        assert_eq!(reloaded.to_name(), "bob");
        assert_eq!(reloaded.addressee_slot(), Some(3));
    }

    #[test]
    fn can_rewrite_addressee_and_slot() {
        // Spec consequent: `mail.to_name = new_to_name; mail.addressee
        // = lookup_user_by_name(new_to_name, ...)`. The caller pre-
        // resolves the slot.
        let sysop = make_user_with_level(1, 255);
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let number = insert_sample(&mut store);

        edit_mail_header(
            &sysop,
            &mut store,
            number,
            None,
            Some(("dave".to_string(), Some(7))),
        )
        .expect("happy path");

        let reloaded = store.load(number).unwrap().unwrap();
        assert_eq!(reloaded.to_name(), "dave");
        assert_eq!(reloaded.addressee_slot(), Some(7));
        assert_eq!(reloaded.subject(), "old subject"); // untouched
    }

    #[test]
    fn null_subject_and_null_to_leaves_everything_unchanged() {
        // Both consequents are gated on `!= null`. Passing None for
        // both is a no-op (other than the save) — the row remains
        // bit-identical.
        let sysop = make_user_with_level(1, 255);
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let number = insert_sample(&mut store);
        let before = store.load(number).unwrap().unwrap();

        edit_mail_header(&sysop, &mut store, number, None, None).expect("no-op happy path");

        let after = store.load(number).unwrap().unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn access_level_210_can_edit_header() {
        // Spec access disjunct: `session.user.access_level >= 210`.
        let co_sysop = make_user_with_level(7, 210);
        assert!(!co_sysop.is_sysop());
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let number = insert_sample(&mut store);

        edit_mail_header(
            &co_sysop,
            &mut store,
            number,
            Some("changed".to_string()),
            None,
        )
        .expect("210 bypasses sysop check");

        assert_eq!(store.load(number).unwrap().unwrap().subject(), "changed");
    }

    #[test]
    fn ordinary_user_cannot_edit_header() {
        // Neither the sysop nor the access-210 disjunct holds — a
        // regular user at access_level 100 is refused.
        let ordinary = make_user_with_level(5, 100);
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));
        let number = insert_sample(&mut store);

        let err = edit_mail_header(
            &ordinary,
            &mut store,
            number,
            Some("hack".to_string()),
            None,
        )
        .expect_err("ordinary user is refused");
        assert!(
            matches!(err, EditMailHeaderError::NotPermitted),
            "got {err:?}"
        );
        // No mutation — subject is still the original.
        assert_eq!(
            store.load(number).unwrap().unwrap().subject(),
            "old subject"
        );
    }

    #[test]
    fn rejects_when_mail_does_not_exist() {
        let sysop = make_user_with_level(1, 255);
        let mut store = InMemoryMailStore::new(MessageBaseRef::new(2, 1));

        let err = edit_mail_header(&sysop, &mut store, 99, Some("anything".to_string()), None)
            .expect_err("missing mail");
        assert!(
            matches!(err, EditMailHeaderError::NotFound(99)),
            "got {err:?}"
        );
    }
}
