//! [`MailStore`] port (Phase 6, Slice 37).
//!
//! Abstracts persistence of [`Mail`][crate::domain::messaging::mail::Mail] within a
//! single message base. Concrete implementations live in
//! [`crate::adapters`].
//!
//! The store owns the spec's `MessageNumbersUniquePerBase` and
//! `HighestMessageMatchesMaxNumber` invariants: callers post a
//! [`MailDraft`] and the store atomically allocates the next number,
//! persists the mail, and updates its cached high-water mark.

use std::error::Error;

use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail::{Mail, MailDraft};

/// Adapter-originated source error attached to domain-shaped
/// persistence failures.
pub type StoreSourceError = Box<dyn Error + Send + Sync + 'static>;

/// Errors returned by [`MailStore`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum MailStoreError {
    /// A storage backend operation (reading, writing, or enumerating
    /// on-disk state) failed. The concrete cause is type-erased so the
    /// port stays free of any adapter-specific I/O type; the adapter
    /// translates its native error into the boxed [`StoreSourceError`].
    #[error("mail store backend error: {source}")]
    Backend {
        /// Underlying adapter error.
        #[source]
        source: StoreSourceError,
    },
    /// The caller handed [`MailStore::save`] / [`MailStore::insert`] a
    /// mail bound to a different msgbase than this store. Writing it
    /// here would silently break the mail's [`MessageBaseRef`]
    /// coordinate, so the store refuses.
    #[error(
        "mail belongs to msgbase ({payload_conference},{payload_msgbase}) \
         but store is bound to ({store_conference},{store_msgbase})"
    )]
    MsgbaseMismatch {
        /// Conference number the mail is bound to.
        payload_conference: u32,
        /// Msgbase number the mail is bound to.
        payload_msgbase: u32,
        /// Conference number the store was opened against.
        store_conference: u32,
        /// Msgbase number the store was opened against.
        store_msgbase: u32,
    },
    /// A caller attempted to update a message number that is not
    /// currently present in the store. `save` only overwrites existing
    /// messages; new mail must be allocated through [`MailStore::insert`]
    /// so the high-water mark stays authoritative.
    #[error("message {number} is not present in msgbase ({store_conference},{store_msgbase})")]
    MessageMissing {
        /// Message number that was requested.
        number: u32,
        /// Conference number the store was opened against.
        store_conference: u32,
        /// Msgbase number the store was opened against.
        store_msgbase: u32,
    },
}

/// Persistence port for a single [`MessageBaseRef`]'s mail.
pub trait MailStore {
    /// Returns the highest message number currently persisted, or `0`
    /// for an empty store. Spec: `core.allium:MessageBase.highest_message`
    /// reflects this exactly per the `HighestMessageMatchesMaxNumber`
    /// invariant.
    fn highest_message(&self) -> u32;

    /// Returns the parent message base this store is bound to.
    fn msgbase(&self) -> MessageBaseRef;

    /// Atomically allocates the next message number, persists the
    /// resulting [`Mail`] and returns it.
    ///
    /// # Errors
    /// Returns [`MailStoreError::Backend`] when the underlying storage
    /// rejects the write.
    fn insert(&mut self, draft: MailDraft) -> Result<Mail, MailStoreError>;

    /// Loads the message persisted at `number`, or `None` when no
    /// such message exists.
    ///
    /// # Errors
    /// Returns [`MailStoreError::Backend`] for read failures and for
    /// persisted data the adapter diagnoses as corrupt or inconsistent;
    /// the adapter-specific diagnostic travels in the boxed source.
    fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError>;

    /// Overwrites the persisted message at `mail.number()` with the
    /// supplied payload. Used by `messaging.allium:ReadMail` (Slice 39)
    /// to persist `received_at` once the addressee has read the mail,
    /// and by the visibility-transition rules in Phase 8.
    ///
    /// `mail.msgbase()` must equal [`Self::msgbase`] and a message at
    /// `mail.number()` must already exist in the store — `save` is
    /// not an alternative to [`Self::insert`].
    ///
    /// # Errors
    /// Returns [`MailStoreError::MsgbaseMismatch`] when
    /// `mail.msgbase()` disagrees with the store's binding, and
    /// [`MailStoreError::MessageMissing`] when `mail.number()` is not
    /// already present. Returns [`MailStoreError::Backend`] when the
    /// underlying storage rejects the write.
    fn save(&mut self, mail: &Mail) -> Result<(), MailStoreError>;

    /// Returns the lowest message number that is *not*
    /// soft-deleted (spec:
    /// `core.allium:MessageBase.lowest_undeleted_message`). Returns
    /// [`Self::highest_message`] + 1 when every persisted mail is
    /// deleted (or the store is empty) — the conventional
    /// "no more readable mail" sentinel.
    ///
    /// The default impl walks `1..=highest_message()` calling
    /// [`Self::load`] on each entry. Adapters may override with a
    /// cheaper implementation when persistent state allows it.
    ///
    /// # Errors
    /// Propagates any [`MailStoreError`] raised by the underlying
    /// scan.
    fn lowest_undeleted_message(&self) -> Result<u32, MailStoreError> {
        use crate::domain::messaging::mail::MailVisibility;
        let highest = self.highest_message();
        for number in 1..=highest {
            match self.load(number) {
                Ok(Some(mail)) if !matches!(mail.visibility(), MailVisibility::Deleted) => {
                    return Ok(number);
                }
                Ok(_) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(highest.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    struct LoadFailingStore;

    impl LoadFailingStore {
        fn backend_error() -> MailStoreError {
            MailStoreError::Backend {
                source: Box::new(io::Error::other("load failed")),
            }
        }
    }

    impl MailStore for LoadFailingStore {
        fn highest_message(&self) -> u32 {
            2
        }

        fn msgbase(&self) -> MessageBaseRef {
            MessageBaseRef::new(1, 1)
        }

        fn insert(&mut self, _draft: MailDraft) -> Result<Mail, MailStoreError> {
            Err(Self::backend_error())
        }

        fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError> {
            if number == 2 {
                Err(Self::backend_error())
            } else {
                Ok(None)
            }
        }

        fn save(&mut self, _mail: &Mail) -> Result<(), MailStoreError> {
            Err(Self::backend_error())
        }
    }

    #[test]
    fn lowest_undeleted_message_propagates_load_errors() {
        let store = LoadFailingStore;

        let err = store
            .lowest_undeleted_message()
            .expect_err("scan load errors must propagate");

        assert!(
            err.to_string().contains("load failed"),
            "unexpected error: {err}"
        );
    }
}

/// In-memory [`MailStore`] for the messaging-rule tests.
///
/// Lives under `#[cfg(test)]` and is only intended for unit tests of
/// [`crate::domain::post_mail`] and friends — the production
/// adapters in [`crate::adapters`] are the file-backed implementations.
///
/// Mirrors `FileMailStore` semantics: monotonic numbers allocated at
/// insert time, payload stored verbatim, [`MailStore::save`] replaces
/// the matching entry. Three rule families used to copy-paste this
/// type into their own test modules.
#[cfg(test)]
pub(crate) mod test_support {
    use std::time::{Duration, SystemTime};

    use super::{Mail, MailDraft, MailStore, MailStoreError, MessageBaseRef};
    use crate::domain::conference::ConferenceMembership;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::User;

    pub(crate) struct InMemoryMailStore {
        msgbase: MessageBaseRef,
        highest: u32,
        mails: Vec<Mail>,
    }

    impl InMemoryMailStore {
        pub(crate) fn new(msgbase: MessageBaseRef) -> Self {
            Self {
                msgbase,
                highest: 0,
                mails: Vec::new(),
            }
        }
    }

    impl MailStore for InMemoryMailStore {
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

    /// A `SystemTime` `secs` seconds after the Unix epoch.
    pub(crate) fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// A user at access level 100 with a granted membership in
    /// conference 2 — the default fixture for the messaging rules, which
    /// gate on a granted membership for the mail's parent conference.
    pub(crate) fn make_user(slot: u32) -> User {
        make_user_with_handle(slot, &format!("user{slot}"))
    }

    /// [`make_user`] with a caller-chosen handle, for the
    /// addressee-resolution tests (`forward`, `reply`).
    pub(crate) fn make_user_with_handle(slot: u32, handle: &str) -> User {
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

    /// A user at a caller-chosen access level with NO conference
    /// membership. The `move` / `edit-header` tests assert on the access
    /// gate before any membership lookup, so they deliberately omit the
    /// grant.
    pub(crate) fn make_user_with_level(slot: u32, access_level: u8) -> User {
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
}
