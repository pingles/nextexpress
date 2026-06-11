//! Default seed data used when the configured user repository is empty
//! (Slice 13a).
//!
//! Phase 1 ships only the in-memory [`UserRepository`][repo] adapter, so
//! a fresh boot has no users to log in as. To make the binary usable
//! out of the box, [`default_sysop`] returns a slot-1 sysop with handle
//! `sysop` and password `sysop`, hashed with the spec's default
//! algorithm. This is explicitly a development seed; later slices that
//! introduce a persistent user store will replace it with proper
//! enrolment.
//!
//! [repo]: crate::domain::user_repository::UserRepository

use std::time::SystemTime;

use time::macros::datetime;
use time::OffsetDateTime;

use crate::domain::bytes::Bytes;
use crate::domain::conference::{Conference, ConferenceMembership};
use crate::domain::files::area::FileArea;
use crate::domain::files::file::{File, FileStatus};
use crate::domain::password::{PasswordError, PasswordHashKind, PasswordHasher};
use crate::domain::user::{User, UserError};

/// Errors returned by [`default_sysop`].
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The hasher couldn't compute a hash for the seed credential.
    #[error("couldn't hash seed credential: {0}")]
    Hash(#[source] PasswordError),
    /// The freshly hashed credential triple failed [`User::new`]'s
    /// invariants. This should never happen for the spec-default
    /// PBKDF2 hash, but is propagated rather than panicking.
    #[error("couldn't construct seeded user: {0}")]
    User(#[source] UserError),
}

/// Builds the default sysop seed user.
///
/// Returns a slot-`1` [`User`] with handle `sysop`, access level `255`
/// and a password hash for the literal string `sysop` produced by
/// `hasher`. `password_last_updated` is set to [`SystemTime::UNIX_EPOCH`]
/// so the seed sorts before any real account.
///
/// # Errors
/// Returns [`SeedError::Hash`] if the hasher rejects the spec-default
/// [`PasswordHashKind::Pbkdf210000`], or [`SeedError::User`] if the
/// freshly hashed triple fails [`User::new`]'s invariants.
pub fn default_sysop(hasher: &dyn PasswordHasher) -> Result<User, SeedError> {
    let kind = PasswordHashKind::Pbkdf210000;
    let computed = hasher
        .compute_password_hash("sysop", kind)
        .map_err(SeedError::Hash)?;
    User::new(
        1,
        "sysop".to_string(),
        kind,
        computed.hash,
        computed.salt,
        SystemTime::UNIX_EPOCH,
        255,
    )
    .map_err(SeedError::User)
}

/// One demo-catalogue row: name, size, upload-writer check byte,
/// upload timestamp, listing text (first DIR line + `\n`-separated
/// continuations).
type DemoRow = (&'static str, u64, Option<u8>, OffsetDateTime, &'static str);

/// Mirror of `comparison/evidence-tierD/fixtures/Dir1` — the corpus
/// seeded onto the live FS-UAE reference board, so dev-boot listings
/// are directly byte-comparable to the captured transcripts. Includes
/// the deliberate edge rows: an 8-digit size (column drift), a 13-char
/// and an over-long filename (no check byte, plain-row fallback), a
/// failed-upload `F` check byte, and `Sent by:` / multi-line
/// continuations. Timestamps are fixed mid-day UTC.
const DEMO_DIR1: [DemoRow; 27] = [
    ("ANSIPACK.LHA", 234_567, Some(b'P'), datetime!(2026-01-15 12:00 UTC), "Collection of 40 ANSI screens from the\nMirage art crew, January release.\nIncludes viewer."),
    ("TERMV48.LHA", 456_123, Some(b'P'), datetime!(2026-01-22 12:00 UTC), "Term v4.8 - the best Amiga comms package"),
    ("LHA_138.RUN", 123_456, Some(b'P'), datetime!(2026-02-01 12:00 UTC), "LhA 1.38 evaluation - archiver"),
    ("MODRIPPR.LZH", 98_304, Some(b'P'), datetime!(2026-02-03 12:00 UTC), "Rip protracker mods from memory"),
    ("SYSINFO4.LHA", 187_233, Some(b'P'), datetime!(2026-02-10 12:00 UTC), "SysInfo 4.0 - system information tool"),
    ("PTREPLAY.LHA", 45_678, Some(b'P'), datetime!(2026-02-14 12:00 UTC), "Protracker replay routine, asm source\nSent by: SYSOP"),
    ("AMIGANSI.TXT", 4_521, Some(b'P'), datetime!(2026-02-20 12:00 UTC), "ANSI escape code reference for Amiga"),
    ("BBSDOORS.LHA", 345_987, Some(b'P'), datetime!(2026-03-01 12:00 UTC), "Collection of 12 AmiExpress doors\nTested on /X 4.x and 5.x."),
    ("WB31PTCH.LHA", 78_222, Some(b'P'), datetime!(2026-03-05 12:00 UTC), "Workbench 3.1 unofficial patches"),
    ("ZOOMOD12.MOD", 267_890, Some(b'P'), datetime!(2026-03-12 12:00 UTC), "4-channel mod - 'Zoom' by Stinger"),
    ("PICPACK1.LHA", 876_543, Some(b'P'), datetime!(2026-03-18 12:00 UTC), "IFF picture pack vol 1 - 25 images"),
    ("CRUNCHM2.LHA", 34_567, Some(b'P'), datetime!(2026-03-25 12:00 UTC), "CrunchMania 2.0 - file cruncher"),
    ("ASMTUT05.TXT", 23_456, Some(b'P'), datetime!(2026-04-02 12:00 UTC), "68k assembler tutorial part 5: copper\nlists and display interrupts."),
    ("VIRUSZ31.LHA", 145_678, Some(b'P'), datetime!(2026-04-08 12:00 UTC), "VirusZ 3.1 - virus checker update"),
    ("FCOMPARE.LHA", 12_345, Some(b'P'), datetime!(2026-04-15 12:00 UTC), "Compare two files, show differences"),
    ("DISKMAST.LHA", 234_511, Some(b'P'), datetime!(2026-04-22 12:00 UTC), "DiskMaster 2.1 - directory utility"),
    ("MEGADEMO.DMS", 12_345_678, Some(b'P'), datetime!(2026-05-01 12:00 UTC), "Spaceballs mega demo disk 1 of 2"),
    ("XPRZMODM.LHA", 56_789, Some(b'P'), datetime!(2026-05-06 12:00 UTC), "XPR Zmodem library 2.10"),
    ("GUIDE2AE.TXT", 8_765, Some(b'P'), datetime!(2026-05-12 12:00 UTC), "Beginners guide to AmiExpress BBSing"),
    ("BADUPLD.LHA", 11_111, Some(b'F'), datetime!(2026-05-15 12:00 UTC), "Upload aborted at 80 percent"),
    ("THIRTEENCH.LZ", 66_666, None, datetime!(2026-05-20 12:00 UTC), "Exactly thirteen character filename"),
    ("ALONGFILENAME.LHA", 77_777, None, datetime!(2026-05-22 12:00 UTC), "Long filename breaks the columns"),
    ("STARVIEW.LHA", 198_765, Some(b'P'), datetime!(2026-05-28 12:00 UTC), "StarView 2.4 - astronomy program\nPlots 9000 stars, needs FPU."),
    ("ICONPACK.LHA", 54_321, Some(b'P'), datetime!(2026-06-02 12:00 UTC), "MagicWB style icons, 200 drawers"),
    ("PROTRACK.LHA", 321_098, Some(b'P'), datetime!(2026-06-05 12:00 UTC), "Protracker 3.15 final release"),
    ("NEWSREAD.LHA", 87_654, Some(b'P'), datetime!(2026-06-08 12:00 UTC), "Offline news reader for UUCP feeds"),
    ("README1ST.TXT", 1_024, None, datetime!(2026-06-09 12:00 UTC), "How the file areas on this board work"),
];

/// Mirror of `comparison/evidence-tierD/fixtures/Dir2` — the upload
/// directory's fresh arrivals, including the same-date pair that pins
/// the renderer's butt-join rule.
const DEMO_DIR2: [DemoRow; 3] = [
    (
        "FRESHUPL.LHA",
        43_210,
        Some(b'P'),
        datetime!(2026-06-09 12:00 UTC),
        "Uploaded last night, awaiting sort",
    ),
    (
        "MYDEMO.DMS",
        567_890,
        Some(b'P'),
        datetime!(2026-06-10 12:00 UTC),
        "My first demo - feedback welcome!\nGreets to everyone on node 1.",
    ),
    (
        "TOOLPACK.LHA",
        234_567,
        Some(b'P'),
        datetime!(2026-06-10 12:00 UTC),
        "Misc CLI tools collection",
    ),
];

fn demo_file(row: &DemoRow) -> File {
    let (name, size, check, uploaded, description) = row;
    File::new(
        (*name).to_string(),
        Bytes::new(*size),
        // Mirrors SysopUploadFile (files.allium:431-449): sysop-
        // imported records are created directly `available`.
        FileStatus::Available,
        *check,
        (*description).to_string(),
        SystemTime::from(*uploaded),
    )
}

/// Builds the demo file catalogue used when no real file data is
/// configured (the file-side analogue of [`default_sysop`]).
///
/// The first conference in `conferences` — the landing conference —
/// receives areas 1 ("Main") and 2 ("Uploads") seeded with the
/// fixture corpus, so a fresh boot's `F` listing is never empty and
/// matches the live captures byte-for-byte; every other conference
/// receives one empty "Main" area. Returns the `(areas, placements)`
/// pair [`crate::adapters::in_memory_file_repository::InMemoryFileRepository::new`]
/// accepts. Empty when `conferences` is empty.
#[must_use]
pub fn demo_file_catalogue(conferences: &[Conference]) -> (Vec<FileArea>, Vec<(u32, u32, File)>) {
    let Some(first) = conferences.first() else {
        return (Vec::new(), Vec::new());
    };
    let landing = first.number();

    let mut areas = vec![
        FileArea::new(landing, 1, "Main".to_string()),
        FileArea::new(landing, 2, "Uploads".to_string()),
    ];
    areas.extend(
        conferences[1..]
            .iter()
            .map(|conference| FileArea::new(conference.number(), 1, "Main".to_string())),
    );

    let files = DEMO_DIR1
        .iter()
        .map(|row| (landing, 1, demo_file(row)))
        .chain(DEMO_DIR2.iter().map(|row| (landing, 2, demo_file(row))))
        .collect();

    (areas, files)
}

/// Grants `user` a `granted = true` [`ConferenceMembership`] for every
/// conference in `conferences` (Slice 34a). Used by the composition
/// root so the seeded sysop can auto-rejoin into a freshly bootstrapped
/// catalogue without a separate admin step. Pre-existing rows for the
/// same conference are upserted to `granted = true`.
pub fn grant_all_memberships(user: &mut User, conferences: &[Conference]) {
    for conference in conferences {
        user.upsert_membership(ConferenceMembership::new(conference.number(), true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::domain::conference::MessageBase;

    fn make_conf(number: u32) -> Conference {
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    #[test]
    fn grant_all_memberships_adds_a_granted_row_per_conference() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        grant_all_memberships(&mut user, &confs);
        for conf in &confs {
            assert!(
                user.has_membership(conf),
                "missing grant for {}",
                conf.number()
            );
        }
    }

    #[test]
    fn grant_all_memberships_upserts_existing_rows_to_granted_true() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        // Pre-existing revoked row for conf 1.
        user.upsert_membership(ConferenceMembership::new(1, false));
        let confs = vec![make_conf(1)];
        grant_all_memberships(&mut user, &confs);
        assert!(user.has_membership(&confs[0]));
    }

    #[test]
    fn grant_all_memberships_with_empty_catalogue_is_a_noop() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        grant_all_memberships(&mut user, &[]);
        assert!(user.memberships().is_empty());
    }

    #[test]
    fn default_sysop_uses_slot_1_and_handle_sysop() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(user.is_sysop());
        assert_eq!(user.handle(), "sysop");
    }

    #[test]
    fn default_sysop_authenticates_against_correct_password() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(hasher.verify_password(&user, "sysop").expect("verify"));
    }

    #[test]
    fn default_sysop_rejects_other_passwords() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(!hasher.verify_password(&user, "wrong").expect("verify"));
        assert!(!hasher.verify_password(&user, "").expect("verify"));
        assert!(!hasher.verify_password(&user, "Sysop").expect("verify"));
    }

    #[test]
    fn default_sysop_uses_pbkdf2_hash_kind() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert_eq!(user.password_hash_kind(), PasswordHashKind::Pbkdf210000);
        assert!(user.password_salt().is_some(), "PBKDF2 user has a salt");
    }

    #[test]
    fn demo_catalogue_seeds_the_first_conference_with_the_fixture_corpus() {
        let confs = vec![make_conf(1), make_conf(2)];
        let (areas, files) = demo_file_catalogue(&confs);

        // First conference: two areas; every other conference: one
        // empty area.
        let conf1_areas: Vec<u32> = areas
            .iter()
            .filter(|a| a.conference() == 1)
            .map(FileArea::number)
            .collect();
        let conf2_areas: Vec<u32> = areas
            .iter()
            .filter(|a| a.conference() == 2)
            .map(FileArea::number)
            .collect();
        assert_eq!(conf1_areas, vec![1, 2]);
        assert_eq!(conf2_areas, vec![1]);

        let in_area = |conf: u32, area: u32| -> Vec<&crate::domain::files::file::File> {
            files
                .iter()
                .filter(|(c, a, _)| *c == conf && *a == area)
                .map(|(_, _, f)| f)
                .collect()
        };
        let dir1 = in_area(1, 1);
        let dir2 = in_area(1, 2);
        assert_eq!(dir1.len(), 27, "fixtures/Dir1 has 27 file rows");
        assert_eq!(dir2.len(), 3, "fixtures/Dir2 has 3 file rows");
        assert!(in_area(2, 1).is_empty(), "other conferences seed empty");

        let by_name = |name: &str| -> &crate::domain::files::file::File {
            files
                .iter()
                .map(|(_, _, f)| f)
                .find(|f| f.name() == name)
                .unwrap_or_else(|| panic!("seed record {name} missing"))
        };

        // The deliberate edge-case rows from the captured fixtures.
        let megademo = by_name("MEGADEMO.DMS");
        assert_eq!(megademo.size().count(), 12_345_678);
        assert_eq!(megademo.check_char(), Some(b'P'));
        assert_eq!(by_name("THIRTEENCH.LZ").check_char(), None);
        assert_eq!(by_name("ALONGFILENAME.LHA").check_char(), None);
        assert_eq!(by_name("README1ST.TXT").check_char(), None);
        assert_eq!(by_name("BADUPLD.LHA").check_char(), Some(b'F'));

        // The SENTBY_FILES-style continuation survives in the
        // description text.
        let ptreplay_lines: Vec<&str> = by_name("PTREPLAY.LHA").description_lines().collect();
        assert_eq!(
            ptreplay_lines,
            vec!["Protracker replay routine, asm source", "Sent by: SYSOP"]
        );

        // Date anchor: 2026-01-15T12:00:00Z, independently computed.
        assert_eq!(
            by_name("ANSIPACK.LHA").uploaded_at(),
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_768_478_400)
        );
        // The same-date pair that pins the renderer's butt-join rule.
        assert_eq!(
            by_name("MYDEMO.DMS").uploaded_at(),
            by_name("TOOLPACK.LHA").uploaded_at()
        );

        // Legacy entry bound: every description line fits the 44-char
        // upload editor width (files.allium:558-560).
        for (_, _, file) in &files {
            for line in file.description_lines() {
                assert!(
                    line.len() <= 44,
                    "{}: line {line:?} exceeds 44 chars",
                    file.name()
                );
            }
        }
    }

    #[test]
    fn demo_catalogue_with_no_conferences_is_empty() {
        let (areas, files) = demo_file_catalogue(&[]);
        assert!(areas.is_empty());
        assert!(files.is_empty());
    }
}
