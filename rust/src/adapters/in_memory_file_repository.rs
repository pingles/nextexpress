//! In-memory [`FileRepository`] adapter.
//!
//! Phase-1 storage for the file-browse unit (slices D1+D2): the
//! composition root seeds it with the demo catalogue when no real
//! file data is configured, so a fresh boot always has something to
//! list. The `SQLite` metadata store (`designs/FILES.md`) lands in slice
//! D2s. Read-only port — the adapter holds plain owned `Vec`s with no
//! interior mutability.

use crate::domain::files::area::{FileArea, FileAreaRef};
use crate::domain::files::file::File;
use crate::domain::files::repository::{FileRepository, FileRepositoryError};

/// An owned, immutable file catalogue serving the [`FileRepository`]
/// port from memory.
#[derive(Debug)]
pub struct InMemoryFileRepository {
    areas: Vec<FileArea>,
    /// `(conference, area, file)` placements, in insertion order.
    files: Vec<(u32, u32, File)>,
}

impl InMemoryFileRepository {
    /// Builds a repository over `areas` and `files`, where each file
    /// entry places a [`File`] in `(conference, area)`. Insertion
    /// order of `files` is the tiebreak order
    /// [`find_in_area`](FileRepository::find_in_area) preserves.
    #[must_use]
    pub fn new(areas: Vec<FileArea>, files: Vec<(u32, u32, File)>) -> Self {
        Self { areas, files }
    }

    /// Files matching `keep` over `(conference, area, file)`,
    /// `uploaded_at` ascending with insertion order preserved between
    /// equal timestamps (stable sort).
    fn select(&self, keep: impl Fn(u32, u32, &File) -> bool) -> Vec<File> {
        let mut selected: Vec<&File> = self
            .files
            .iter()
            .filter(|(conf, area, file)| keep(*conf, *area, file))
            .map(|(_, _, file)| file)
            .collect();
        selected.sort_by_key(|file| file.uploaded_at());
        selected.into_iter().cloned().collect()
    }
}

impl FileRepository for InMemoryFileRepository {
    fn areas_in_conference(&self, conference: u32) -> Result<Vec<FileArea>, FileRepositoryError> {
        let mut areas: Vec<FileArea> = self
            .areas
            .iter()
            .filter(|area| area.conference() == conference)
            .cloned()
            .collect();
        areas.sort_by_key(FileArea::number);
        Ok(areas)
    }

    fn find_in_area(&self, area: FileAreaRef) -> Result<Vec<File>, FileRepositoryError> {
        Ok(self.select(|conf, file_area, file| {
            conf == area.conference() && file_area == area.area() && {
                file.status().is_listing_visible()
            }
        }))
    }

    fn list_held(&self, conference: u32) -> Result<Vec<File>, FileRepositoryError> {
        use crate::domain::files::file::FileStatus;
        Ok(self.select(|conf, _, file| {
            conf == conference && file.status() == FileStatus::HeldForReview
        }))
    }

    fn list_new_since(
        &self,
        area: FileAreaRef,
        since: std::time::SystemTime,
    ) -> Result<Vec<File>, FileRepositoryError> {
        Ok(self.select(|conf, file_area, file| {
            conf == area.conference()
                && file_area == area.area()
                && file.status().is_listing_visible()
                && file.uploaded_at() >= since
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};
    use crate::domain::files::repository::FileRepository;
    use std::time::{Duration, SystemTime};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn file(name: &str, status: FileStatus, uploaded_secs: u64) -> File {
        File::new(
            name.to_string(),
            Bytes::new(1_000),
            status,
            Some(b'P'),
            format!("{name} description"),
            t(uploaded_secs),
        )
    }

    fn repo() -> InMemoryFileRepository {
        InMemoryFileRepository::new(
            vec![
                FileArea::new(2, 2, "Uploads".to_string()),
                FileArea::new(2, 1, "Main".to_string()),
                FileArea::new(1, 1, "General".to_string()),
            ],
            vec![
                (2, 1, file("NEWEST.LHA", FileStatus::Available, 300)),
                (2, 1, file("HELDFILE.LHA", FileStatus::HeldForReview, 150)),
                (2, 1, file("OLDEST.LHA", FileStatus::Lcfiles, 100)),
                (2, 1, file("MIDDLE.LHA", FileStatus::Available, 200)),
                (2, 2, file("OTHERAREA.LHA", FileStatus::Available, 50)),
                (1, 1, file("OTHERCONF.LHA", FileStatus::HeldForReview, 60)),
            ],
        )
    }

    #[test]
    fn find_in_area_returns_only_listing_visible_files_oldest_first() {
        let names: Vec<String> = repo()
            .find_in_area(FileAreaRef::new(2, 1))
            .expect("files")
            .into_iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(names, vec!["OLDEST.LHA", "MIDDLE.LHA", "NEWEST.LHA"]);
    }

    #[test]
    fn find_in_area_preserves_insertion_order_for_equal_uploaded_at() {
        let repo = InMemoryFileRepository::new(
            vec![FileArea::new(2, 1, "Main".to_string())],
            vec![
                (2, 1, file("FIRST.LHA", FileStatus::Available, 500)),
                (2, 1, file("SECOND.LHA", FileStatus::Available, 500)),
            ],
        );
        let names: Vec<String> = repo
            .find_in_area(FileAreaRef::new(2, 1))
            .expect("files")
            .into_iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(names, vec!["FIRST.LHA", "SECOND.LHA"]);
    }

    #[test]
    fn find_in_area_unknown_conference_or_area_is_empty() {
        assert!(repo()
            .find_in_area(FileAreaRef::new(9, 1))
            .expect("files")
            .is_empty());
        assert!(repo()
            .find_in_area(FileAreaRef::new(2, 9))
            .expect("files")
            .is_empty());
    }

    #[test]
    fn list_held_returns_only_held_files_for_the_conference() {
        let names: Vec<String> = repo()
            .list_held(2)
            .expect("files")
            .into_iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(names, vec!["HELDFILE.LHA"]);
    }

    #[test]
    fn list_new_since_is_inclusive_of_the_boundary_instant() {
        // The N filter is inclusive (`express.e:27976-27986` `ddt>=day`,
        // confirmed by the SCAN sibling capture): a file uploaded
        // exactly at the cutoff is listed.
        let repo = InMemoryFileRepository::new(
            vec![FileArea::new(1, 1, "Main".to_string())],
            vec![
                (1, 1, file("OLDER.LHA", FileStatus::Available, 100)),
                (1, 1, file("ONCUT.LHA", FileStatus::Available, 200)),
                (1, 1, file("NEWER.LHA", FileStatus::Available, 300)),
            ],
        );
        let names = |since: u64| -> Vec<String> {
            repo.list_new_since(FileAreaRef::new(1, 1), t(since))
                .expect("files")
                .into_iter()
                .map(|f| f.name().to_string())
                .collect()
        };
        assert_eq!(names(200), vec!["ONCUT.LHA", "NEWER.LHA"]);
        assert_eq!(names(201), vec!["NEWER.LHA"]);
        assert_eq!(names(301), Vec::<String>::new());
        // Ordering contract: uploaded_at ascending, like find_in_area.
        assert_eq!(names(0), vec!["OLDER.LHA", "ONCUT.LHA", "NEWER.LHA"]);
    }

    #[test]
    fn list_new_since_keeps_the_listing_visibility_contract() {
        // Same visibility filter as find_in_area: HeldForReview rows
        // never list; a failed-check Available row (BADUPLD's shape —
        // check char 'F', status Available, seed.rs) DOES list, exactly
        // as the live door listed it (ae_tierd_newfiles.txt N2 pass 2,
        // File #19).
        let badupld = File::new(
            "BADUPLD.LHA".to_string(),
            Bytes::new(11_111),
            FileStatus::Available,
            Some(b'F'),
            "Upload aborted at 80 percent".to_string(),
            t(150),
        );
        let repo = InMemoryFileRepository::new(
            vec![FileArea::new(1, 1, "Main".to_string())],
            vec![
                (1, 1, badupld),
                (1, 1, file("HELD.LHA", FileStatus::HeldForReview, 160)),
                (1, 1, file("VISIBLE.LHA", FileStatus::Available, 170)),
            ],
        );
        let names: Vec<String> = repo
            .list_new_since(FileAreaRef::new(1, 1), t(0))
            .expect("files")
            .into_iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(names, vec!["BADUPLD.LHA", "VISIBLE.LHA"]);
    }

    #[test]
    fn list_new_since_scopes_to_the_requested_area() {
        let names: Vec<String> = repo()
            .list_new_since(FileAreaRef::new(2, 1), t(0))
            .expect("files")
            .into_iter()
            .map(|f| f.name().to_string())
            .collect();
        assert_eq!(names, vec!["OLDEST.LHA", "MIDDLE.LHA", "NEWEST.LHA"]);
        assert!(repo()
            .list_new_since(FileAreaRef::new(2, 9), t(0))
            .expect("files")
            .is_empty());
    }

    #[test]
    fn areas_in_conference_sorts_ascending_by_number() {
        let numbers: Vec<u32> = repo()
            .areas_in_conference(2)
            .expect("areas")
            .into_iter()
            .map(|a| a.number())
            .collect();
        assert_eq!(numbers, vec![1, 2]);
    }

    #[test]
    fn areas_in_conference_unknown_conference_is_empty() {
        assert!(repo().areas_in_conference(9).expect("areas").is_empty());
    }
}
