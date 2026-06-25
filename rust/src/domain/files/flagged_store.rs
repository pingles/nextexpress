//! Port over a user's durable flagged-file set (slice D5-persist).
//!
//! The session's [`FlaggedFiles`] set lives and dies with the session
//! in memory; this port persists it per user slot so the next logon can
//! restore it (the legacy `saveFlagged`/`loadFlagged`,
//! `amiexpress/express.e:2806`/`:2757`). The persisted identity is
//! `(conference, name)` — `area` is normalised to `0` on load, uniformly
//! across adapters (see the design's Decision ①).

use crate::domain::files::flagged::FlaggedFiles;

/// Error from a [`FlaggedStore`] backend.
#[derive(Debug, thiserror::Error)]
pub enum FlaggedStoreError {
    /// The backing store could not be read or written.
    #[error("flagged-file store backend error: {0}")]
    Backend(String),
}

/// Durable home of a user's flagged-file set, keyed by user slot.
pub trait FlaggedStore {
    /// Loads the flag set saved for `slot`, or an empty set when none is
    /// stored. Restored keys carry `area = 0`.
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
