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
    /// A persisted message could not be parsed.
    #[error("malformed mail at {path}: {source}")]
    Malformed {
        /// Path of the offending message file.
        path: String,
        /// Underlying parse error.
        #[source]
        source: StoreSourceError,
    },
    /// A mail could not be serialised to JSON. The in-memory writers
    /// used by [`MailStore`] implementations cannot themselves fail, so
    /// reaching this variant indicates a bug in the encoder rather than
    /// a deployment problem — it's modelled explicitly anyway so the
    /// adapter doesn't have to panic.
    #[error("failed to serialise mail at number {number}: {source}")]
    Serialise {
        /// Message number that failed to serialise.
        number: u32,
        /// Underlying serde error.
        #[source]
        source: StoreSourceError,
    },
    /// A persisted message's recorded number disagrees with the number
    /// encoded in its filename. Catches manual edits that would
    /// otherwise let `MessageNumbersUniquePerBase` silently drift.
    #[error(
        "mail file {path} encodes number {filename_number} but its \
         payload declares number {payload_number}"
    )]
    NumberMismatch {
        /// Path of the offending message file.
        path: String,
        /// Number derived from the filename.
        filename_number: u32,
        /// Number declared in the JSON payload.
        payload_number: u32,
    },
    /// A persisted message's recorded `msgbase` disagrees with the
    /// store's configured [`MessageBaseRef`]. Catches a message that
    /// has been copied into the wrong msgbase directory.
    #[error(
        "mail file {path} belongs to msgbase \
         ({payload_conference},{payload_msgbase}) but store is bound to \
         ({store_conference},{store_msgbase})"
    )]
    MsgbaseMismatch {
        /// Path of the offending message file.
        path: String,
        /// Conference number declared in the JSON payload.
        payload_conference: u32,
        /// Msgbase number declared in the JSON payload.
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
    /// Returns [`MailStoreError::Backend`] for read failures and
    /// [`MailStoreError::Malformed`] / [`MailStoreError::NumberMismatch`]
    /// / [`MailStoreError::MsgbaseMismatch`] for corrupted data.
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
    use super::{Mail, MailDraft, MailStore, MailStoreError, MessageBaseRef};

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
}
