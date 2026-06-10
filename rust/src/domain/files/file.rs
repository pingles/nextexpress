//! [`File`] entity and its status lifecycle (spec: `files.allium:File`).

use std::time::SystemTime;

use crate::domain::bytes::Bytes;

/// Lifecycle status of a [`File`] (spec: `files.allium:FileStatus`).
///
/// Browse (the D1+D2 unit) carries only the variants its rules read:
/// the listing-visible pair plus `held_for_review`, which the `F H`
/// hold listing shows. The spec's remaining variants (`in_playpen`,
/// `quarantined`, `removed`) and the transition table arrive with
/// their first writers — the upload, background-check and maintenance
/// slices (schema-growth principle, `SLICES.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Visible in listings, downloadable (spec: `files.allium:52`).
    Available,
    /// Visible in listings; downloads count more strongly against
    /// ratios (spec: `files.allium:53`).
    Lcfiles,
    /// Awaiting sysop review; shown only by the hold listing
    /// (spec: `files.allium:51`).
    HeldForReview,
}

impl FileStatus {
    /// Whether files with this status appear in the normal directory
    /// listing.
    ///
    /// The spec pins the visible set as `{available, lcfiles}`
    /// (`files.allium:52-53`; `FlagFile`'s requires at `:165` and the
    /// `FlaggedFilesAreDownloadable` invariant at `:492-495` use the
    /// same set).
    #[must_use]
    pub const fn is_listing_visible(self) -> bool {
        matches!(self, Self::Available | Self::Lcfiles)
    }
}

/// A file held in a conference's file area (spec: `files.allium:File`).
///
/// Carries only the fields the browse rules read (schema-growth,
/// `SLICES.md`): `description_source`, `file_id_diz`, `uploaded_by`,
/// `last_downloaded_at` and `download_count` arrive with the upload /
/// DIZ / transfer slices that first read them. The conference/area
/// association lives in the
/// [`FileRepository`](crate::domain::files::repository) keying rather
/// than on the entity — no browse rule reads `file.area` directly.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    name: String,
    size: Bytes,
    status: FileStatus,
    check_char: Option<u8>,
    description: String,
    uploaded_at: SystemTime,
}

impl File {
    /// Constructs a [`File`].
    ///
    /// `check_char` is the raw status byte the legacy upload writer
    /// pokes at column 13 of a DIR row (`P`assed / `F`ailed /
    /// `N`ot-allowed / `D`upe — `amiexpress/express.e:19458-19470`);
    /// `None` for rows that never carried one. `description` is the
    /// whole listing text — first DIR line plus `\n`-separated
    /// continuation lines — in one field (spec: `files.allium:94`).
    #[must_use]
    pub fn new(
        name: String,
        size: Bytes,
        status: FileStatus,
        check_char: Option<u8>,
        description: String,
        uploaded_at: SystemTime,
    ) -> Self {
        Self {
            name,
            size,
            status,
            check_char,
            description,
            uploaded_at,
        }
    }

    /// The stored filename ("12-char filename in the legacy code",
    /// spec: `files.allium:90`; longer names occur and are never
    /// truncated).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// File size in octets.
    #[must_use]
    pub const fn size(&self) -> Bytes {
        self.size
    }

    /// Lifecycle status.
    #[must_use]
    pub const fn status(&self) -> FileStatus {
        self.status
    }

    /// The legacy upload-writer status byte, if the row carries one.
    #[must_use]
    pub const fn check_char(&self) -> Option<u8> {
        self.check_char
    }

    /// When the file arrived (drives listing order and the date shown
    /// in the rendered row).
    #[must_use]
    pub const fn uploaded_at(&self) -> SystemTime {
        self.uploaded_at
    }

    /// The listing text split into its lines: the first DIR line
    /// followed by any continuation lines.
    pub fn description_lines(&self) -> impl Iterator<Item = &str> {
        self.description.split('\n')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn available_and_lcfiles_are_listing_visible_held_is_not() {
        assert!(FileStatus::Available.is_listing_visible());
        assert!(FileStatus::Lcfiles.is_listing_visible());
        assert!(!FileStatus::HeldForReview.is_listing_visible());
    }

    #[test]
    fn file_round_trips_the_browse_fields() {
        let uploaded = SystemTime::UNIX_EPOCH + Duration::from_secs(1_768_996_800);
        let file = File::new(
            "ANSIPACK.LHA".to_string(),
            crate::domain::bytes::Bytes::new(234_567),
            FileStatus::Available,
            Some(b'P'),
            "Collection of 40 ANSI screens".to_string(),
            uploaded,
        );
        assert_eq!(file.name(), "ANSIPACK.LHA");
        assert_eq!(file.size().count(), 234_567);
        assert_eq!(file.status(), FileStatus::Available);
        assert_eq!(file.check_char(), Some(b'P'));
        assert_eq!(file.uploaded_at(), uploaded);
    }

    #[test]
    fn check_char_is_absent_for_unchecked_rows() {
        let file = File::new(
            "THIRTEENCH.LZ".to_string(),
            crate::domain::bytes::Bytes::new(66_666),
            FileStatus::Available,
            None,
            "Exactly thirteen character filename".to_string(),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(file.check_char(), None);
    }

    #[test]
    fn description_lines_splits_the_embedded_listing_text() {
        let file = File::new(
            "STARVIEW.LHA".to_string(),
            crate::domain::bytes::Bytes::new(198_765),
            FileStatus::Available,
            Some(b'P'),
            "StarView 2.4 - astronomy program\nPlots 9000 stars, needs FPU.".to_string(),
            SystemTime::UNIX_EPOCH,
        );
        let lines: Vec<&str> = file.description_lines().collect();
        assert_eq!(
            lines,
            vec![
                "StarView 2.4 - astronomy program",
                "Plots 9000 stars, needs FPU.",
            ]
        );
    }

    #[test]
    fn single_line_description_yields_one_line() {
        let file = File::new(
            "LHA_138.RUN".to_string(),
            crate::domain::bytes::Bytes::new(123_456),
            FileStatus::Available,
            Some(b'P'),
            "LhA 1.38 evaluation - archiver".to_string(),
            SystemTime::UNIX_EPOCH,
        );
        let lines: Vec<&str> = file.description_lines().collect();
        assert_eq!(lines, vec!["LhA 1.38 evaluation - archiver"]);
    }
}
