//! [`FileRepository`] port: rule-named queries the browse use cases
//! read (spec: `files.allium` browse; `designs/FILES.md` adapter
//! contract).
//!
//! The port is a domain-side abstraction; concrete implementations
//! live in [`crate::adapters`]. It stays narrow — methods are named
//! after the rules that need them, not generic CRUD. Reads are
//! fallible so a real store (the `SQLite` metadata store, slice D2s)
//! can report backend failures without reshaping every signature at
//! adapter-swap time; write methods land rule-named with their
//! consuming slices (`list_new_since` with the `N` scan,
//! `record_download` with D-T2, `begin`/`complete_upload` with
//! D-T4a). File *content* is deliberately not this port's concern —
//! a separate `FileContentStore` port arrives with the first transfer
//! slice (D-T1).

use std::error::Error;

use crate::domain::files::area::{FileArea, FileAreaRef};
use crate::domain::files::file::File;

/// Failure reported by a [`FileRepository`] backend.
///
/// One opaque variant, per the port-error convention (July 2026
/// review, item 2): adapters box their native error as the `source`
/// and keep rich diagnostics adapter-private. Callers render the
/// catalogue as empty and log — the wire the legacy shows for an
/// unreadable DIR file is the empty listing.
#[derive(Debug, thiserror::Error)]
pub enum FileRepositoryError {
    /// The backing store failed; `source` is the adapter's boxed
    /// native error.
    #[error("file repository backend error: {source}")]
    Backend {
        /// The adapter's type-erased native error.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Read access to a board's file areas and their listings.
pub trait FileRepository {
    /// The conference's file areas, ascending by area number. Empty
    /// when the conference has no areas (or is unknown).
    ///
    /// # Errors
    /// [`FileRepositoryError::Backend`] when the backing store fails.
    fn areas_in_conference(&self, conference: u32) -> Result<Vec<FileArea>, FileRepositoryError>;

    /// Listing-visible files (`FileStatus::is_listing_visible`) of one
    /// area, `uploaded_at` ascending with insertion order as the
    /// tiebreak — the order the legacy DIR file accumulated rows in,
    /// which the captures pin byte-observably (same-date neighbours
    /// list in upload order).
    ///
    /// # Errors
    /// [`FileRepositoryError::Backend`] when the backing store fails.
    fn find_in_area(&self, area: FileAreaRef) -> Result<Vec<File>, FileRepositoryError>;

    /// Files awaiting sysop review in this conference, for the hold
    /// listing (`F H`). Same ordering contract as
    /// [`find_in_area`](Self::find_in_area).
    ///
    /// # Errors
    /// [`FileRepositoryError::Backend`] when the backing store fails.
    fn list_held(&self, conference: u32) -> Result<Vec<File>, FileRepositoryError>;
}
