//! Port over a user's durable flagged-file set (slice D5-persist).
//!
//! The session's [`FlaggedFiles`] set lives and dies with the session
//! in memory; this port persists it per user slot so the next logon can
//! restore it (the legacy `saveFlagged`/`loadFlagged`,
//! `amiexpress/express.e:2806`/`:2757`). The persisted identity is
//! `(conference, name)` — exactly the domain `FlaggedKey` since the
//! July 2026 identity fix, so persistence is a lossless projection.

use std::error::Error;

use crate::domain::files::flagged::FlaggedFiles;

/// Adapter-originated source error attached to
/// [`FlaggedStoreError`] values.
pub type FlaggedStoreSourceError = Box<dyn Error + Send + Sync + 'static>;

/// Error from a [`FlaggedStore`] backend.
///
/// One opaque variant, per the port-error convention (July 2026
/// review, item 2): the adapter boxes its native error as the
/// `source`, preserving the diagnostic chain the old stringly
/// `Backend(String)` shape discarded.
#[derive(Debug, thiserror::Error)]
pub enum FlaggedStoreError {
    /// The backing store could not be read or written.
    #[error("flagged-file store backend error: {source}")]
    Backend {
        /// Underlying adapter error.
        #[source]
        source: FlaggedStoreSourceError,
    },
}

/// Durable home of a user's flagged-file set, keyed by user slot.
pub trait FlaggedStore {
    /// Loads the flag set saved for `slot`, or an empty set when none is
    /// stored.
    ///
    /// # Errors
    /// Returns [`FlaggedStoreError`] when the backing store cannot be read.
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError>;

    /// Replaces the saved flag set for `slot` with `flags` (an empty set
    /// clears it), persisting the `(conference, name)` projection.
    ///
    /// # Errors
    /// Returns [`FlaggedStoreError`] when the backing store cannot be written.
    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError>;
}
