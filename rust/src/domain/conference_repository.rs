//! [`ConferenceRepository`] port for loading conferences from
//! persistent storage.
//!
//! The port is a domain-side abstraction; concrete implementations
//! live in [`crate::adapters`]. The Phase 4 loader (Slice 28) reads a
//! TOML layout that mirrors the legacy `defaultbbs/Conf<NN>/...`
//! directory schema while replacing the Amiga-specific binary files
//! with TOML, per `AGENTS.md`.

use std::error::Error;

use crate::domain::conference::{Conference, ConferenceError};

/// Adapter-originated source error attached to
/// [`ConferenceRepositoryError`] values.
pub type ConferenceRepositorySourceError = Box<dyn Error + Send + Sync + 'static>;

/// Errors returned by [`ConferenceRepository::load_all`]
/// implementations.
#[derive(Debug, thiserror::Error)]
pub enum ConferenceRepositoryError {
    /// A storage backend operation (enumerating or reading the on-disk
    /// layout) failed. The concrete cause is type-erased so the port
    /// stays free of any adapter-specific I/O type; the adapter
    /// translates its native error into the boxed
    /// [`ConferenceRepositorySourceError`].
    #[error("conference repository backend error: {source}")]
    Backend {
        /// Underlying adapter error.
        #[source]
        source: ConferenceRepositorySourceError,
    },
    /// A conference TOML payload could not be parsed.
    #[error("malformed conference file at {path}: {source}")]
    MalformedConference {
        /// Path of the offending TOML file.
        path: String,
        /// Underlying TOML parser error.
        #[source]
        source: ConferenceRepositorySourceError,
    },
    /// A conference's TOML data violates a domain invariant from
    /// [`ConferenceError`] (e.g. `AtLeastOneMessageBase`).
    #[error("conference {path} is invalid: {source}")]
    InvalidConference {
        /// Path of the offending TOML file.
        path: String,
        /// Underlying domain error.
        #[source]
        source: ConferenceError,
    },
    /// A conference directory carries a `number` field that disagrees
    /// with the number embedded in its directory name.
    #[error(
        "conference at {path} declares number {declared} but its \
         directory name encodes {expected}"
    )]
    ConferenceNumberMismatch {
        /// Path of the offending TOML file.
        path: String,
        /// Number recorded inside the file.
        declared: u32,
        /// Number derived from the enclosing directory name.
        expected: u32,
    },
    /// Two conferences share the same `number`.
    #[error("duplicate conference number {number}")]
    DuplicateConferenceNumber {
        /// The clashing conference number.
        number: u32,
    },
}

/// Port over the conference catalogue.
///
/// Implementations return the catalogue in ascending conference-number
/// order so callers can rely on
/// `result[i].number() < result[i + 1].number()` without re-sorting.
pub trait ConferenceRepository {
    /// Loads every conference known to the repository.
    ///
    /// # Errors
    /// Returns [`ConferenceRepositoryError`] when storage cannot be
    /// read or the on-disk data violates a domain invariant.
    fn load_all(&self) -> Result<Vec<Conference>, ConferenceRepositoryError>;
}
