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
