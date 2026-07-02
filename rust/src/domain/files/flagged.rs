//! Session-scoped flagged files — slice D2f, the in-memory precursor
//! to slice D5's persisted `FlaggedFile` (`amiexpress/express.e:2757`
//! loadFlagged / `:2798` saveFlagged own persistence later).

use std::collections::BTreeSet;

/// Catalogue identity of a flaggable file: `(conference, name)` — the
/// legacy `(confNum, fileName)` key (`isInFlaggedList`,
/// `amiexpress/express.e:12534`). Deliberately carries no catalogue
/// area: a flag set from an `F` listing (which knows its dir number)
/// and the same name typed at the `A` prompt or restored on logon
/// (which do not) must be the SAME flag. Names compare
/// case-insensitively (stored uppercase) — the DIR catalogue is
/// case-preserving but the legacy flag prompt is not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlaggedKey {
    conference: u32,
    name: String,
}

impl FlaggedKey {
    /// Builds a key; `name` is folded to uppercase.
    pub(crate) fn new(conference: u32, name: &str) -> Self {
        Self {
            conference,
            name: name.to_ascii_uppercase(),
        }
    }

    /// The uppercase-folded file name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The conference this flag belongs to — the legacy `confNum`
    /// half of the persisted `(confNum, fileName)` key (slice D5-persist).
    pub(crate) fn conference(&self) -> u32 {
        self.conference
    }
}

/// The session's flagged-file set. Slice D5 will persist it; until
/// then it lives and dies with the session.
#[derive(Debug, Clone, Default)]
pub struct FlaggedFiles {
    set: BTreeSet<FlaggedKey>,
}

impl FlaggedFiles {
    /// Flags `key`. Returns `true` when newly flagged — the repaint
    /// trigger; re-flagging is a no-op.
    pub(crate) fn flag(&mut self, key: FlaggedKey) -> bool {
        self.set.insert(key)
    }

    /// Whether `key` is flagged.
    pub(crate) fn contains(&self, key: &FlaggedKey) -> bool {
        self.set.contains(key)
    }

    /// Whether nothing is flagged — the `checkFlagged()` gate
    /// (`amiexpress/express.e:12669`, `flagFilesList.count()`): plain
    /// `G` only confirms when the set is non-empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Drops every flag — the legacy `clearFlagItems(flagFilesList)`,
    /// reached by `A`'s `C` -> `*` (=All) clear path (slice D6b,
    /// `amiexpress/express.e:12622`).
    pub(crate) fn clear(&mut self) {
        self.set.clear();
    }

    /// The flagged file names, upper-cased, in key order
    /// (conference, name). Backs the `A` listing —
    /// `showFlaggedFiles(-1)` (`amiexpress/express.e:2830`), which the
    /// legacy walks in insertion order; `NextExpress` walks the sorted
    /// `BTreeSet`, a deliberate ordering divergence (slice D6a).
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.set.iter().map(FlaggedKey::name)
    }

    /// The flags as `(conference, name)` pairs in key order — exactly
    /// what `FlaggedStore::save` persists (slice D5-persist), matching
    /// the legacy `flagged` file's conf + name rows
    /// (`amiexpress/express.e:2822`). Since the key IS
    /// `(conference, name)`, the pairs are unique by construction.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (u32, &str)> {
        self.set.iter().map(|k| (k.conference(), k.name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flagging_is_case_insensitive_and_idempotent() {
        let mut flags = FlaggedFiles::default();
        assert!(flags.flag(FlaggedKey::new(2, "termv48.lha")));
        assert!(!flags.flag(FlaggedKey::new(2, "TERMV48.LHA")), "same file");
        assert!(flags.contains(&FlaggedKey::new(2, "TermV48.LHA")));
        assert!(
            !flags.contains(&FlaggedKey::new(3, "TERMV48.LHA")),
            "other conference"
        );
    }

    #[test]
    fn same_conference_and_name_is_one_flag_regardless_of_source_area() {
        // The legacy flag identity is (confNum, fileName) with no area
        // (`isInFlaggedList`, amiexpress/express.e:12534): a file flagged
        // from an F listing (which knows its dir number) and the same
        // name typed at the A prompt (which does not) are the SAME flag.
        let mut flags = FlaggedFiles::default();
        assert!(flags.flag(FlaggedKey::new(2, "termv48.lha")));
        assert!(
            !flags.flag(FlaggedKey::new(2, "TERMV48.LHA")),
            "same conference + name = same file, wherever it was flagged from"
        );
        assert_eq!(flags.names().count(), 1, "the A listing shows it once");
        assert_eq!(flags.entries().count(), 1, "save persists one row");
    }

    #[test]
    fn name_is_uppercase_folded_for_matching() {
        let key = FlaggedKey::new(2, "TermV48.lha");
        assert_eq!(key.name(), "TERMV48.LHA");
    }

    #[test]
    fn entries_yield_conference_and_name_pairs_in_key_order() {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(2, "termv48.lha"));
        flags.flag(FlaggedKey::new(1, "mydemo.dms"));
        let entries: Vec<(u32, &str)> = flags.entries().collect();
        assert_eq!(
            entries,
            vec![(1, "MYDEMO.DMS"), (2, "TERMV48.LHA")],
            "entries are (conference, upper-name) in BTreeSet key order"
        );
    }
}
