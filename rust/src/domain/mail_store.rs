//! [`MailStore`] port (Phase 6, Slice 37).
//!
//! Abstracts persistence of [`Mail`][crate::domain::mail::Mail] within a
//! single message base. Concrete implementations live in
//! [`crate::adapters`].
//!
//! The store owns the spec's `MessageNumbersUniquePerBase` and
//! `HighestMessageMatchesMaxNumber` invariants: callers post a
//! [`MailDraft`] and the store atomically allocates the next number,
//! persists the mail, and updates its cached high-water mark.

use crate::domain::conference::MessageBaseRef;
use crate::domain::mail::{Mail, MailDraft};

/// Errors returned by [`MailStore`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum MailStoreError {
    /// I/O failure while reading or writing on-disk state.
    #[error("mail store I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A persisted message could not be parsed.
    #[error("malformed mail at {path}: {source}")]
    Malformed {
        /// Path of the offending message file.
        path: String,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
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
        source: serde_json::Error,
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
    /// Returns [`MailStoreError::Io`] when the underlying storage
    /// rejects the write.
    fn insert(&mut self, draft: MailDraft) -> Result<Mail, MailStoreError>;

    /// Loads the message persisted at `number`, or `None` when no
    /// such message exists.
    ///
    /// # Errors
    /// Returns [`MailStoreError::Io`] for read failures and
    /// [`MailStoreError::Malformed`] / [`MailStoreError::NumberMismatch`]
    /// / [`MailStoreError::MsgbaseMismatch`] for corrupted data.
    fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError>;
}
