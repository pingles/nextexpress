//! File-backed [`ConferenceRepository`] (Phase 4, Slice 28).
//!
//! The adapter mirrors the legacy `defaultbbs/Conf<NN>/...` directory
//! layout — one directory per conference, named `Conf01`, `Conf02`,
//! and so on — while replacing the Amiga-specific
//! `path`/`paths`/`NDirs`/`Conf.DB` files with a single TOML
//! configuration per conference (per `AGENTS.md`'s "configuration via
//! files rather than a separate program" rule).
//!
//! # On-disk layout
//!
//! ```text
//! <bbs_path>/
//! ├── Conf01/
//! │   ├── conference.toml
//! │   └── menu.txt              # used in Slice 31, ignored here
//! ├── Conf02/
//! │   ├── conference.toml
//! │   └── Menu.txt
//! ```
//!
//! A `conference.toml` describes the conference and its message bases:
//!
//! ```toml
//! number = 1
//! name   = "Lamer Land"
//!
//! [[msgbase]]
//! number = 1
//! name   = "main"
//! ```
//!
//! The legacy seed-file references in `SLICES.md`'s asset inventory
//! map onto this format as follows:
//!
//! - the conference's free-text name (legacy `Conf.DB` tooltype) →
//!   `name` field;
//! - the conference's 1-indexed number (legacy directory ordinal) →
//!   `number` field, and is also encoded in the enclosing
//!   `Conf<NN>` directory name. The two must agree;
//! - the message-base list (legacy `Conf.DB` plus tooltypes) →
//!   `[[msgbase]]` entries.
//!
//! Out-of-scope fields (file-area paths, ratios, free-downloads, etc.)
//! land in the slices that actually consume them; the parser
//! deliberately does not accept them yet so a typo doesn't get
//! silently swallowed.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::domain::conference::{Conference, MessageBase, NameType};
use crate::domain::conference_repository::{ConferenceRepository, ConferenceRepositoryError};
use crate::domain::mail::{AllScanScope, AllowedAddressing};

/// Filename of the per-conference TOML configuration inside each
/// `Conf<NN>` directory.
const CONFERENCE_FILENAME: &str = "conference.toml";

/// File-backed conference repository rooted at a BBS installation
/// path.
#[derive(Debug)]
pub struct FileConferenceRepository {
    bbs_path: PathBuf,
}

impl FileConferenceRepository {
    /// Constructs a repository rooted at `bbs_path`.
    #[must_use]
    pub fn new(bbs_path: PathBuf) -> Self {
        Self { bbs_path }
    }
}

impl ConferenceRepository for FileConferenceRepository {
    fn load_all(&self) -> Result<Vec<Conference>, ConferenceRepositoryError> {
        let entries = match fs::read_dir(&self.bbs_path) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(ConferenceRepositoryError::Io(e)),
        };

        let mut directories: Vec<(u32, PathBuf)> = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(number) = parse_conference_dir_number(&name) {
                directories.push((number, entry.path()));
            }
        }
        directories.sort_by_key(|(number, _)| *number);

        let mut seen: BTreeSet<u32> = BTreeSet::new();
        let mut out = Vec::with_capacity(directories.len());
        for (expected_number, dir) in directories {
            if !seen.insert(expected_number) {
                return Err(ConferenceRepositoryError::DuplicateConferenceNumber {
                    number: expected_number,
                });
            }
            let toml_path = dir.join(CONFERENCE_FILENAME);
            let text = fs::read_to_string(&toml_path)?;
            let parsed: ConferenceToml = toml::from_str(&text).map_err(|source| {
                ConferenceRepositoryError::MalformedConference {
                    path: toml_path.display().to_string(),
                    source: Box::new(source),
                }
            })?;
            if parsed.number != expected_number {
                return Err(ConferenceRepositoryError::ConferenceNumberMismatch {
                    path: toml_path.display().to_string(),
                    declared: parsed.number,
                    expected: expected_number,
                });
            }
            let msgbases: Vec<MessageBase> = parsed
                .msgbases
                .into_iter()
                .map(|m| {
                    MessageBase::with_options(
                        parsed.number,
                        m.number,
                        m.name,
                        m.allowed_addressing.into(),
                        m.all_scan_scope.into(),
                    )
                })
                .collect();
            let conference = Conference::with_name_type(
                parsed.number,
                parsed.name,
                msgbases,
                parsed.accepted_name_type.into(),
            )
            .map_err(|source| ConferenceRepositoryError::InvalidConference {
                path: toml_path.display().to_string(),
                source,
            })?;
            out.push(conference);
        }
        Ok(out)
    }
}

/// TOML schema for a single `conference.toml` file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConferenceToml {
    number: u32,
    name: String,
    /// `core.allium:Conference.accepted_name_type` (Slice 34).
    /// Optional; defaults to [`NameType::Handle`] for the common
    /// case so existing conference files don't need to be touched.
    #[serde(default)]
    accepted_name_type: NameTypeToml,
    #[serde(default, rename = "msgbase")]
    msgbases: Vec<MessageBaseToml>,
}

/// Mirror of [`NameType`] with TOML-friendly snake-case names.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NameTypeToml {
    #[default]
    Handle,
    RealName,
    InternetName,
}

impl From<NameTypeToml> for NameType {
    fn from(value: NameTypeToml) -> Self {
        match value {
            NameTypeToml::Handle => Self::Handle,
            NameTypeToml::RealName => Self::RealName,
            NameTypeToml::InternetName => Self::InternetName,
        }
    }
}

/// TOML schema for a `[[msgbase]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageBaseToml {
    number: u32,
    name: String,
    /// `messaging.allium:MessageBase.allowed_addressing` (Slice 43).
    /// Optional; defaults to [`AllowedAddressing::Any`] so existing
    /// `conference.toml` files don't need to change.
    #[serde(default)]
    allowed_addressing: AllowedAddressingToml,
    /// `messaging.allium:MessageBase.all_scan_scope` (Slice 43).
    /// Optional; defaults to [`AllScanScope::AllUsersInConf`].
    #[serde(default)]
    all_scan_scope: AllScanScopeToml,
}

/// Mirror of [`AllowedAddressing`] with TOML-friendly snake-case names.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AllowedAddressingToml {
    IndividualOnly,
    IndividualOrAll,
    IndividualOrEall,
    #[default]
    Any,
}

impl From<AllowedAddressingToml> for AllowedAddressing {
    fn from(value: AllowedAddressingToml) -> Self {
        match value {
            AllowedAddressingToml::IndividualOnly => Self::IndividualOnly,
            AllowedAddressingToml::IndividualOrAll => Self::IndividualOrAll,
            AllowedAddressingToml::IndividualOrEall => Self::IndividualOrEall,
            AllowedAddressingToml::Any => Self::Any,
        }
    }
}

/// Mirror of [`AllScanScope`] with TOML-friendly snake-case names.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AllScanScopeToml {
    Local,
    #[default]
    AllUsersInConf,
}

impl From<AllScanScopeToml> for AllScanScope {
    fn from(value: AllScanScopeToml) -> Self {
        match value {
            AllScanScopeToml::Local => Self::Local,
            AllScanScopeToml::AllUsersInConf => Self::AllUsersInConf,
        }
    }
}

/// Parses a `Conf<NN>` directory name (e.g. `Conf01`, `Conf02`) and
/// returns the embedded conference number, or `None` for any other
/// name. The numeric portion must be all digits, mirroring the
/// legacy directory naming. The bare `"Conf"` prefix on its own
/// (empty suffix) is rejected by `parse()` itself returning `Err`.
fn parse_conference_dir_number(dir_name: &str) -> Option<u32> {
    let suffix = dir_name.strip_prefix("Conf")?;
    if !suffix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn write_conf(root: &Path, number: u32, body: &str) -> PathBuf {
        let dir = root.join(format!("Conf{number:02}"));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFERENCE_FILENAME);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn parse_conference_dir_number_accepts_two_digit_padded_numbers() {
        assert_eq!(parse_conference_dir_number("Conf01"), Some(1));
        assert_eq!(parse_conference_dir_number("Conf02"), Some(2));
        assert_eq!(parse_conference_dir_number("Conf99"), Some(99));
    }

    #[test]
    fn parse_conference_dir_number_accepts_unpadded_numbers() {
        // A sysop hand-creating a directory may type "Conf3" rather
        // than "Conf03"; both should resolve to 3.
        assert_eq!(parse_conference_dir_number("Conf3"), Some(3));
        assert_eq!(parse_conference_dir_number("Conf123"), Some(123));
    }

    #[test]
    fn parse_conference_dir_number_rejects_non_conference_directories() {
        assert_eq!(parse_conference_dir_number("Screens"), None);
        assert_eq!(parse_conference_dir_number("Conf"), None);
        assert_eq!(parse_conference_dir_number("Confxx"), None);
        assert_eq!(parse_conference_dir_number("Conf01a"), None);
        assert_eq!(parse_conference_dir_number("conf01"), None);
        assert_eq!(parse_conference_dir_number(""), None);
    }

    #[test]
    fn missing_bbs_path_yields_empty_catalogue() {
        // The composition root may construct the repository before any
        // bbs directory exists (e.g. fresh install scenarios). Treat a
        // missing path as "no conferences yet" rather than an error.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-yet-installed");
        let repo = FileConferenceRepository::new(missing);
        let confs = repo
            .load_all()
            .expect("missing path is empty, not an error");
        assert!(confs.is_empty());
    }

    #[test]
    fn non_directory_bbs_path_returns_an_io_error() {
        // Only `NotFound` is treated as "no conferences yet". Anything
        // else — for example a misconfiguration that points the BBS
        // root at a regular file — must surface so the sysop notices.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        fs::write(&file, b"oops").unwrap();
        let repo = FileConferenceRepository::new(file);
        let err = repo.load_all().expect_err("file path should error");
        assert!(matches!(err, ConferenceRepositoryError::Io(_)));
    }

    #[test]
    fn empty_bbs_path_yields_empty_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("empty");
        assert!(confs.is_empty());
    }

    #[test]
    fn loads_a_single_conference_with_one_message_base() {
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name   = "Lamer Land"

                [[msgbase]]
                number = 1
                name   = "main"
            "#,
        );

        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid layout");

        assert_eq!(confs.len(), 1);
        assert_eq!(confs[0].number(), 1);
        assert_eq!(confs[0].name(), "Lamer Land");
        assert_eq!(confs[0].msgbases().len(), 1);
        assert_eq!(confs[0].msgbases()[0].name(), "main");
        assert_eq!(confs[0].msgbases()[0].number(), 1);
        assert_eq!(confs[0].msgbases()[0].conference_number(), 1);
    }

    #[test]
    fn returns_conferences_sorted_by_number() {
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            5,
            r#"
                number = 5
                name = "Five"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        write_conf(
            dir.path(),
            2,
            r#"
                number = 2
                name = "Two"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        write_conf(
            dir.path(),
            10,
            r#"
                number = 10
                name = "Ten"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );

        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        let numbers: Vec<u32> = confs.iter().map(Conference::number).collect();
        assert_eq!(numbers, vec![2, 5, 10]);
    }

    #[test]
    fn loads_a_conference_with_multiple_message_bases_in_declared_order() {
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            3,
            r#"
                number = 3
                name = "Three"
                [[msgbase]]
                number = 1
                name = "main"
                [[msgbase]]
                number = 2
                name = "tech"
                [[msgbase]]
                number = 3
                name = "off-topic"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        let names: Vec<&str> = confs[0].msgbases().iter().map(MessageBase::name).collect();
        assert_eq!(names, vec!["main", "tech", "off-topic"]);
    }

    #[test]
    fn ignores_directories_that_are_not_conferences() {
        let dir = tempfile::tempdir().unwrap();
        // Sibling directories that the BBS uses for other purposes
        // — Screens for screen files, Logs for caller logs, etc. —
        // must be tolerated, not parsed as conferences.
        fs::create_dir_all(dir.path().join("Screens")).unwrap();
        fs::create_dir_all(dir.path().join("Logs")).unwrap();
        fs::create_dir_all(dir.path().join("Confxx")).unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Solo"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        assert_eq!(confs.len(), 1);
        assert_eq!(confs[0].number(), 1);
    }

    #[test]
    fn ignores_files_living_at_the_bbs_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("README"), b"not a conference").unwrap();
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        assert!(confs.is_empty());
    }

    #[test]
    fn malformed_toml_returns_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_conf(dir.path(), 1, "not = valid = toml");
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let err = repo.load_all().expect_err("malformed");
        match err {
            ConferenceRepositoryError::MalformedConference { path: reported, .. } => {
                assert_eq!(reported, path.display().to_string());
            }
            other => panic!("expected MalformedConference, got {other:?}"),
        }
    }

    #[test]
    fn dir_name_number_must_match_toml_number() {
        let dir = tempfile::tempdir().unwrap();
        // Directory name encodes 1, but the file says 7. Bail out
        // rather than silently accepting the inconsistency — a typo
        // here would otherwise let two conferences claim the same
        // user-visible number.
        write_conf(
            dir.path(),
            1,
            r#"
                number = 7
                name = "Mismatch"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let err = repo.load_all().expect_err("number mismatch");
        match err {
            ConferenceRepositoryError::ConferenceNumberMismatch {
                declared, expected, ..
            } => {
                assert_eq!(declared, 7);
                assert_eq!(expected, 1);
            }
            other => panic!("expected ConferenceNumberMismatch, got {other:?}"),
        }
    }

    #[test]
    fn missing_conference_toml_in_a_conf_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // Empty Conf01/ directory: the loader sees a conference dir
        // but no payload. Surface this as a real error so a sysop
        // notices the half-set-up conference.
        fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let err = repo.load_all().expect_err("missing conference.toml");
        assert!(matches!(err, ConferenceRepositoryError::Io(_)));
    }

    #[test]
    fn empty_msgbase_list_violates_at_least_one_message_base_invariant() {
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "No bases"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let err = repo.load_all().expect_err("empty msgbases");
        assert!(matches!(
            err,
            ConferenceRepositoryError::InvalidConference { .. }
        ));
    }

    #[test]
    fn unknown_top_level_keys_are_rejected() {
        // Catching typos at load-time prevents silently misconfigured
        // conferences. As later slices add fields (free-downloads,
        // ratios, etc.) this set grows.
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Typo'd"
                # 'banner' isn't a known key (yet)
                banner = "welcome"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        assert!(matches!(
            repo.load_all(),
            Err(ConferenceRepositoryError::MalformedConference { .. })
        ));
    }

    #[test]
    fn accepted_name_type_defaults_to_handle() {
        // Slice 34: omitting `accepted_name_type` is a valid layout
        // and means the conference renders posters as `Handle`.
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Plain"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        assert_eq!(confs[0].accepted_name_type(), NameType::Handle);
    }

    #[test]
    fn loads_real_name_and_internet_name_conferences() {
        // Slice 34: the loader recognises snake-case TOML names for
        // each variant.
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Authors"
                accepted_name_type = "real_name"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        write_conf(
            dir.path(),
            2,
            r#"
                number = 2
                name = "Internet"
                accepted_name_type = "internet_name"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        assert_eq!(confs[0].accepted_name_type(), NameType::RealName);
        assert_eq!(confs[1].accepted_name_type(), NameType::InternetName);
    }

    #[test]
    fn msgbase_broadcast_policy_defaults_when_omitted() {
        // Slice 43: existing `conference.toml` files don't carry the
        // broadcast policy fields — they must continue to load with
        // the spec defaults (Any / AllUsersInConf).
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Main"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        let msgbase = &confs[0].msgbases()[0];
        assert_eq!(msgbase.allowed_addressing(), AllowedAddressing::Any);
        assert_eq!(msgbase.all_scan_scope(), AllScanScope::AllUsersInConf);
    }

    #[test]
    fn msgbase_broadcast_policy_round_trips_explicit_values() {
        // Slice 43: a sysop narrows the policy via TOML to forbid EALL
        // (mirroring an external-bridge base that cannot fan out).
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Bridged"
                [[msgbase]]
                number = 1
                name = "main"
                allowed_addressing = "individual_or_all"
                all_scan_scope = "local"
            "#,
        );
        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("valid");
        let msgbase = &confs[0].msgbases()[0];
        assert_eq!(
            msgbase.allowed_addressing(),
            AllowedAddressing::IndividualOrAll
        );
        assert_eq!(msgbase.all_scan_scope(), AllScanScope::Local);
    }

    #[test]
    fn fixture_mirrors_legacy_defaultbbs_layout() {
        // Phase 4 / Slice 28: "Reference the legacy seed files in test
        // fixtures so the loader's layout assumptions are explicit."
        // The legacy `defaultbbs/Conf01/...` ships:
        //   - Conf01/  (empty Conf.DB plus a NDirs / paths metadata
        //               file structure that is replaced here by TOML)
        //   - Conf02/  (with a default 4-command Menu.txt; menu.txt
        //               loading lands in Slice 31)
        // The loader for Slice 28 has to handle that exact pair, and
        // hand the rest of the Phase 4 work two valid Conferences.
        let dir = tempfile::tempdir().unwrap();
        write_conf(
            dir.path(),
            1,
            r#"
                number = 1
                name = "Lamer Land"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );
        write_conf(
            dir.path(),
            2,
            r#"
                number = 2
                name = "Programming"
                [[msgbase]]
                number = 1
                name = "main"
            "#,
        );

        let repo = FileConferenceRepository::new(dir.path().to_path_buf());
        let confs = repo.load_all().expect("legacy-shaped layout");
        assert_eq!(confs.len(), 2);
        assert_eq!(confs[0].number(), 1);
        assert_eq!(confs[0].name(), "Lamer Land");
        assert_eq!(confs[1].number(), 2);
        assert_eq!(confs[1].name(), "Programming");
    }
}
