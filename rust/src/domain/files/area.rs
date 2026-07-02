//! [`FileArea`] entity (spec: `core.allium:FileArea`).

/// A numbered file directory within a conference
/// (spec: `core.allium:FileArea`).
///
/// Browse reads `conference`, `number` and `name` only; `upload_path`
/// and `free_downloads` are transfer-side and arrive with the slices
/// that read them. The legacy upload-dir convention (the
/// highest-numbered area is the upload dir, `displayFileList`'s
/// `fLLoop = maxDirs` branch) is resolved by the listing use case, not
/// stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileArea {
    conference: u32,
    number: u32,
    name: String,
}

impl FileArea {
    /// Constructs a [`FileArea`] for conference `conference`, 1-indexed
    /// `number` within it, with the display `name`.
    #[must_use]
    pub fn new(conference: u32, number: u32, name: String) -> Self {
        Self {
            conference,
            number,
            name,
        }
    }

    /// Number of the conference this area belongs to.
    #[must_use]
    pub const fn conference(&self) -> u32 {
        self.conference
    }

    /// 1-indexed area number within the conference.
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// Display name (e.g. "Utilities").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// This area's `(conference, number)` address.
    #[must_use]
    pub const fn area_ref(&self) -> FileAreaRef {
        FileAreaRef::new(self.conference, self.number)
    }
}

/// Address of one file area: `(conference, area number)` — the file
/// world's analogue of `MessageBaseRef`, and the prefix of the natural
/// file key `(conference, area, name)` (`designs/FILES.md`,
/// `UNIQUE(area_id, name)`). The [`FileRepository`] port takes this
/// instead of raw `(u32, u32)` pairs so conference/area transpositions
/// are unrepresentable at call sites.
///
/// [`FileRepository`]: crate::domain::files::repository::FileRepository
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileAreaRef {
    conference: u32,
    area: u32,
}

impl FileAreaRef {
    /// Constructs an address from raw conference and area numbers.
    /// Prefer [`FileArea::area_ref`] when a [`FileArea`] is in hand.
    #[must_use]
    pub const fn new(conference: u32, area: u32) -> Self {
        Self { conference, area }
    }

    /// The conference's 1-indexed number.
    #[must_use]
    pub const fn conference(&self) -> u32 {
        self.conference
    }

    /// The area's 1-indexed number within its conference.
    #[must_use]
    pub const fn area(&self) -> u32 {
        self.area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_area_round_trips_the_browse_fields() {
        let area = FileArea::new(2, 1, "Utilities".to_string());
        assert_eq!(area.conference(), 2);
        assert_eq!(area.number(), 1);
        assert_eq!(area.name(), "Utilities");
    }
}
