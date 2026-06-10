//! [`FileRepository`] port: rule-named queries the browse use cases
//! read (spec: `files.allium` browse; `designs/FILES.md` adapter
//! contract).
//!
//! The port is a domain-side abstraction; concrete implementations
//! live in [`crate::adapters`]. It stays narrow — methods are named
//! after the rules that need them, not generic CRUD — and read-only:
//! `Result` plumbing arrives with the first fallible adapter (the
//! SQLite store, slice D2s).

use crate::domain::files::area::FileArea;
use crate::domain::files::file::File;

/// Read access to a board's file areas and their listings.
pub trait FileRepository {
    /// The conference's file areas, ascending by area number. Empty
    /// when the conference has no areas (or is unknown).
    fn areas_in_conference(&self, conference: u32) -> Vec<FileArea>;

    /// Listing-visible files (`FileStatus::is_listing_visible`) of one
    /// area, `uploaded_at` ascending with insertion order as the
    /// tiebreak — the order the legacy DIR file accumulated rows in,
    /// which the captures pin byte-observably (same-date neighbours
    /// list in upload order).
    fn find_in_area(&self, conference: u32, area: u32) -> Vec<File>;

    /// Files awaiting sysop review in this conference, for the hold
    /// listing (`F H`). Same ordering contract as
    /// [`find_in_area`](Self::find_in_area).
    fn list_held(&self, conference: u32) -> Vec<File>;
}
