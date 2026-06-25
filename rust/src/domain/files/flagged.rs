//! Session-scoped flagged files — slice D2f, the in-memory precursor
//! to slice D5's persisted `FlaggedFile` (`amiexpress/express.e:2757`
//! loadFlagged / `:2798` saveFlagged own persistence later).

use std::collections::BTreeSet;

/// Catalogue identity of a flaggable file. Names compare
/// case-insensitively (stored uppercase) — the DIR catalogue is
/// case-preserving but the legacy flag prompt is not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FlaggedKey {
    conference: u32,
    area: u32,
    name: String,
}

impl FlaggedKey {
    /// Builds a key; `name` is folded to uppercase.
    // Consumed by Task 3.3/3.4 (listing handler builds keys from the
    // scan context); until then only the tests construct keys.
    #[allow(dead_code)]
    pub(crate) fn new(conference: u32, area: u32, name: &str) -> Self {
        Self {
            conference,
            area,
            name: name.to_ascii_uppercase(),
        }
    }

    /// The uppercase-folded file name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// The session's flagged-file set. Slice D5 will persist it; until
/// then it lives and dies with the session.
#[derive(Debug, Clone, Default)]
pub(crate) struct FlaggedFiles {
    set: BTreeSet<FlaggedKey>,
}

impl FlaggedFiles {
    /// Flags `key`. Returns `true` when newly flagged — the repaint
    /// trigger; re-flagging is a no-op.
    // Consumed by Task 3.2/3.3 (session wiring + listing handler);
    // until then only the tests exercise it.
    #[allow(dead_code)]
    pub(crate) fn flag(&mut self, key: FlaggedKey) -> bool {
        self.set.insert(key)
    }

    /// Whether `key` is flagged.
    // Consumed by Task 3.2/3.4 (repaint/contains check); until then
    // only the tests exercise it.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, key: &FlaggedKey) -> bool {
        self.set.contains(key)
    }

    /// Whether nothing is flagged — the `checkFlagged()` gate
    /// (`amiexpress/express.e:12669`, `flagFilesList.count()`): plain
    /// `G` only confirms when the set is non-empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// The flagged file names, upper-cased, in catalogue-key order
    /// (conference, area, name). Backs the `A` listing —
    /// `showFlaggedFiles(-1)` (`amiexpress/express.e:2830`), which the
    /// legacy walks in insertion order; `NextExpress` walks the sorted
    /// `BTreeSet`, a deliberate ordering divergence (slice D6a).
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.set.iter().map(FlaggedKey::name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flagging_is_case_insensitive_and_idempotent() {
        let mut flags = FlaggedFiles::default();
        assert!(flags.flag(FlaggedKey::new(2, 1, "termv48.lha")));
        assert!(
            !flags.flag(FlaggedKey::new(2, 1, "TERMV48.LHA")),
            "same file"
        );
        assert!(flags.contains(&FlaggedKey::new(2, 1, "TermV48.LHA")));
        assert!(
            !flags.contains(&FlaggedKey::new(2, 2, "TERMV48.LHA")),
            "other area"
        );
    }

    #[test]
    fn name_is_uppercase_folded_for_matching() {
        let key = FlaggedKey::new(2, 1, "TermV48.lha");
        assert_eq!(key.name(), "TERMV48.LHA");
    }
}
