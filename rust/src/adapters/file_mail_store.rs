//! File-backed [`MailStore`] (Phase 6, Slice 37).
//!
//! One JSON file per message at `<msgbase-dir>/<zero-padded-number>.json`.
//! Each file carries the full message (header + body); the store keeps
//! the cached high-water mark in memory and derives it by scanning the
//! directory at open time.
//!
//! See the slice scope in `slices/phase6.md` for the design discussion.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail::{BroadcastTo, Mail, MailDraft, MailVisibility, NewMail};
use crate::domain::messaging::mail_store::{MailStore, MailStoreError};

/// Width of the zero-padded number used as a message's on-disk
/// filename. Chosen so the lexicographic `ls` order matches numeric
/// order up to ~10^7 messages — comfortably more than any realistic
/// `AmiExpress` message base ever held.
const FILENAME_WIDTH: usize = 7;

/// Extension on every persisted message file.
const FILENAME_EXTENSION: &str = "json";

/// File-backed [`MailStore`] rooted at a per-msgbase directory.
#[derive(Debug)]
pub struct FileMailStore {
    dir: PathBuf,
    msgbase: MessageBaseRef,
    highest_message: u32,
    /// Backs the spec's `lock_msgbase(msgbase)` predicate. A tokio
    /// `Mutex` lets `try_lock` work from sync contexts while keeping
    /// the option of `async lock().await` open for slices that prefer
    /// to wait.
    lock: Arc<Mutex<()>>,
}

/// RAII guard returned by [`FileMailStore::try_lock`].
///
/// Holding the guard reserves the message base for the bearer; dropping
/// it releases the lock. Spec: `messaging.allium`'s `lock_msgbase`
/// predicate (`PostMail` precondition).
pub struct MsgbaseLock(#[allow(dead_code)] OwnedMutexGuard<()>);

impl FileMailStore {
    /// Opens (or creates, if missing) a [`FileMailStore`] rooted at
    /// `dir` for the supplied [`MessageBaseRef`].
    ///
    /// Scans `dir` once at open time to populate the cached
    /// high-water mark, logging progress to stderr so the operator
    /// can see why startup is taking time on a large base.
    ///
    /// # Errors
    /// Returns [`MailStoreError::Io`] when the directory cannot be
    /// created or read, [`MailStoreError::Malformed`] when an entry
    /// is unreadable JSON, and [`MailStoreError::NumberMismatch`] /
    /// [`MailStoreError::MsgbaseMismatch`] when a file's payload
    /// disagrees with its filename or the store's binding.
    pub fn open(dir: PathBuf, msgbase: MessageBaseRef) -> Result<Self, MailStoreError> {
        fs::create_dir_all(&dir)?;
        let highest_message = scan_dir_for_highest(&dir, msgbase)?;
        eprintln!(
            "mail store ({},{}) at {}: scanned, highest_message = {}",
            msgbase.conference_number(),
            msgbase.msgbase_number(),
            dir.display(),
            highest_message,
        );
        Ok(Self {
            dir,
            msgbase,
            highest_message,
            lock: Arc::new(Mutex::new(())),
        })
    }

    /// Attempts to acquire the message-base lock backing the spec's
    /// `lock_msgbase(msgbase)` predicate. Returns `Some(MsgbaseLock)`
    /// when the lock was free, `None` when another holder has it
    /// (mirrors the spec's "another node holds it: rule fails" branch).
    ///
    /// The lock is released when the returned [`MsgbaseLock`] is
    /// dropped.
    #[must_use]
    pub fn try_lock(&self) -> Option<MsgbaseLock> {
        self.lock.clone().try_lock_owned().ok().map(MsgbaseLock)
    }

    fn path_for(&self, number: u32) -> PathBuf {
        let width = FILENAME_WIDTH;
        let ext = FILENAME_EXTENSION;
        self.dir.join(format!("{number:0width$}.{ext}"))
    }

    fn write(&self, mail: &Mail) -> Result<(), MailStoreError> {
        let payload = MailPayload::from(mail);
        let path = self.path_for(mail.number());
        let json =
            serde_json::to_string_pretty(&payload).map_err(|source| MailStoreError::Serialise {
                number: mail.number(),
                source: Box::new(source),
            })?;
        fs::write(path, json).map_err(MailStoreError::Io)
    }
}

impl MailStore for FileMailStore {
    fn highest_message(&self) -> u32 {
        self.highest_message
    }

    fn msgbase(&self) -> MessageBaseRef {
        self.msgbase
    }

    fn insert(&mut self, draft: MailDraft) -> Result<Mail, MailStoreError> {
        let number = self.highest_message + 1;
        let mail = Mail::from_draft(self.msgbase, number, draft);
        self.write(&mail)?;
        self.highest_message = number;
        Ok(mail)
    }

    fn load(&self, number: u32) -> Result<Option<Mail>, MailStoreError> {
        let path = self.path_for(number);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(MailStoreError::Io(e)),
        };
        let payload: MailPayload =
            serde_json::from_str(&text).map_err(|source| MailStoreError::Malformed {
                path: path.display().to_string(),
                source: Box::new(source),
            })?;
        payload
            .into_mail_checked(self.msgbase, number, &path)
            .map(Some)
    }

    fn save(&mut self, mail: &Mail) -> Result<(), MailStoreError> {
        if mail.msgbase() != self.msgbase {
            return Err(MailStoreError::MsgbaseMismatch {
                path: self.path_for(mail.number()).display().to_string(),
                payload_conference: mail.msgbase().conference_number(),
                payload_msgbase: mail.msgbase().msgbase_number(),
                store_conference: self.msgbase.conference_number(),
                store_msgbase: self.msgbase.msgbase_number(),
            });
        }
        self.write(mail)
    }
}

fn scan_dir_for_highest(dir: &Path, msgbase: MessageBaseRef) -> Result<u32, MailStoreError> {
    let mut highest = 0u32;
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(number) = parse_message_filename(name) else {
            continue;
        };
        // Open the file just enough to verify it really belongs to
        // this msgbase before counting it towards the high-water mark.
        // This is what guarantees `HighestMessageMatchesMaxNumber`
        // against a directory that has accidentally received an
        // unrelated message file.
        let path = entry.path();
        let text = fs::read_to_string(&path)?;
        let payload: MailPayload =
            serde_json::from_str(&text).map_err(|source| MailStoreError::Malformed {
                path: path.display().to_string(),
                source: Box::new(source),
            })?;
        payload.check_consistency(msgbase, number, &path)?;
        highest = highest.max(number);
    }
    Ok(highest)
}

fn parse_message_filename(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(&format!(".{FILENAME_EXTENSION}"))?;
    // An empty or non-digit stem fails `stem.parse::<u32>()` below, so
    // the explicit guard would be redundant. The `chars().all` check is
    // kept because it rejects payloads like `"+1.json"` or `"01 .json"`
    // that `parse::<u32>` would otherwise accept silently or with
    // leading/trailing whitespace tolerance via stricter parsers.
    if !stem.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

/// Serde representation of a persisted message. Mirrors the spec's
/// [`Mail`] shape minus the derived flags; the on-disk schema is the
/// authoritative format and is deliberately broken out from the
/// in-memory entity so the latter can grow without forcing a stored-data
/// migration.
///
/// Timestamps are RFC 3339 strings in UTC (the `time::serde::rfc3339`
/// adapter handles offsets at parse time, so a sysop or migration tool
/// hand-writing a file with a `+02:00` suffix still round-trips
/// correctly via the in-memory `OffsetDateTime`).
#[derive(Debug, Serialize, Deserialize)]
struct MailPayload {
    conference_number: u32,
    msgbase_number: u32,
    number: u32,
    visibility: VisibilityWire,
    from_name: String,
    to_name: String,
    broadcast_to: BroadcastWire,
    subject: String,
    #[serde(with = "time::serde::rfc3339")]
    posted_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    received_at: Option<OffsetDateTime>,
    author_slot: u32,
    addressee_slot: Option<u32>,
    body: String,
}

impl MailPayload {
    fn check_consistency(
        &self,
        msgbase: MessageBaseRef,
        filename_number: u32,
        path: &Path,
    ) -> Result<(), MailStoreError> {
        if self.number != filename_number {
            return Err(MailStoreError::NumberMismatch {
                path: path.display().to_string(),
                filename_number,
                payload_number: self.number,
            });
        }
        if self.conference_number != msgbase.conference_number()
            || self.msgbase_number != msgbase.msgbase_number()
        {
            return Err(MailStoreError::MsgbaseMismatch {
                path: path.display().to_string(),
                payload_conference: self.conference_number,
                payload_msgbase: self.msgbase_number,
                store_conference: msgbase.conference_number(),
                store_msgbase: msgbase.msgbase_number(),
            });
        }
        Ok(())
    }

    fn into_mail_checked(
        self,
        msgbase: MessageBaseRef,
        filename_number: u32,
        path: &Path,
    ) -> Result<Mail, MailStoreError> {
        self.check_consistency(msgbase, filename_number, path)?;
        let visibility = MailVisibility::from(self.visibility);
        let broadcast_to = BroadcastTo::from(self.broadcast_to);
        let posted_at = std::time::SystemTime::from(self.posted_at);
        let mut mail = Mail::new(NewMail {
            msgbase,
            number: self.number,
            visibility,
            from_name: self.from_name,
            to_name: self.to_name,
            broadcast_to,
            subject: self.subject,
            posted_at,
            author_slot: self.author_slot,
            addressee_slot: self.addressee_slot,
            body: self.body,
        });
        if let Some(when) = self.received_at {
            let when = std::time::SystemTime::from(when);
            // The DeletedMessagesHaveNoActiveReceived invariant is
            // enforced by `mark_received` refusing to set a deleted
            // message. A corrupt payload that violates the invariant
            // surfaces here as a parse error.
            mail.mark_received(when)
                .map_err(|source| MailStoreError::Malformed {
                    path: path.display().to_string(),
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("deleted mail has live received_at: {source}"),
                    )),
                })?;
        }
        Ok(mail)
    }
}

impl From<&Mail> for MailPayload {
    fn from(mail: &Mail) -> Self {
        Self {
            conference_number: mail.msgbase().conference_number(),
            msgbase_number: mail.msgbase().msgbase_number(),
            number: mail.number(),
            visibility: mail.visibility().into(),
            from_name: mail.from_name().to_string(),
            to_name: mail.to_name().to_string(),
            broadcast_to: mail.broadcast_to().into(),
            subject: mail.subject().to_string(),
            posted_at: OffsetDateTime::from(mail.posted_at()),
            received_at: mail.received_at().map(OffsetDateTime::from),
            author_slot: mail.author_slot(),
            addressee_slot: mail.addressee_slot(),
            body: mail.body().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VisibilityWire {
    Public,
    Private,
    PrivateToSysop,
    Deleted,
}

impl From<MailVisibility> for VisibilityWire {
    fn from(v: MailVisibility) -> Self {
        match v {
            MailVisibility::Public => Self::Public,
            MailVisibility::Private => Self::Private,
            MailVisibility::PrivateToSysop => Self::PrivateToSysop,
            MailVisibility::Deleted => Self::Deleted,
        }
    }
}

impl From<VisibilityWire> for MailVisibility {
    fn from(v: VisibilityWire) -> Self {
        match v {
            VisibilityWire::Public => Self::Public,
            VisibilityWire::Private => Self::Private,
            VisibilityWire::PrivateToSysop => Self::PrivateToSysop,
            VisibilityWire::Deleted => Self::Deleted,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BroadcastWire {
    None,
    All,
    Eall,
}

impl From<BroadcastTo> for BroadcastWire {
    fn from(b: BroadcastTo) -> Self {
        match b {
            BroadcastTo::None => Self::None,
            BroadcastTo::All => Self::All,
            BroadcastTo::Eall => Self::Eall,
        }
    }
}

impl From<BroadcastWire> for BroadcastTo {
    fn from(b: BroadcastWire) -> Self {
        match b {
            BroadcastWire::None => Self::None,
            BroadcastWire::All => Self::All,
            BroadcastWire::Eall => Self::Eall,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::adapters::file_mail_store::FileMailStore;
    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::MailStore;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn sample_draft() -> MailDraft {
        MailDraft {
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Welcome".to_string(),
            posted_at: t(100),
            author_slot: 1,
            addressee_slot: Some(2),
            body: "Hello, alice!".to_string(),
        }
    }

    #[test]
    fn inserting_into_an_empty_store_assigns_number_one() {
        // Spec messaging.allium PostMail: let next_number =
        //   visit.msgbase.highest_message + 1.
        // For an empty base highest_message starts at 0, so the first
        // post is numbered 1.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store =
            FileMailStore::open(dir.path().to_path_buf(), msgbase).expect("open empty dir");

        let mail = store.insert(sample_draft()).expect("insert");

        assert_eq!(mail.number(), 1);
        assert_eq!(mail.msgbase(), msgbase);
        assert_eq!(store.highest_message(), 1);
    }

    #[test]
    fn sequential_inserts_get_strictly_increasing_numbers() {
        // Spec invariant MessageNumbersUniquePerBase plus PostMail's
        // `next_number = highest_message + 1` together force strictly
        // monotonic numbering.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let m1 = store.insert(sample_draft()).unwrap();
        let m2 = store.insert(sample_draft()).unwrap();
        let m3 = store.insert(sample_draft()).unwrap();
        assert_eq!(m1.number(), 1);
        assert_eq!(m2.number(), 2);
        assert_eq!(m3.number(), 3);
        assert_eq!(store.highest_message(), 3);
    }

    #[test]
    fn insert_then_load_round_trips_every_header_field() {
        // Pin the full serialisation contract: every header field and
        // the body must survive a write+read cycle byte-for-byte.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let draft = MailDraft {
            visibility: MailVisibility::Private,
            from_name: "alice".to_string(),
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "lunch?".to_string(),
            posted_at: t(12_345),
            author_slot: 2,
            addressee_slot: Some(3),
            body: "Free at 1?\nMeet at the door.\n".to_string(),
        };
        let posted = store.insert(draft).unwrap();

        let loaded = store
            .load(posted.number())
            .unwrap()
            .expect("just-inserted mail must be loadable");

        assert_eq!(loaded, posted);
    }

    #[test]
    fn load_of_a_missing_number_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let result = store.load(42).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn highest_message_is_zero_on_an_empty_store() {
        // The spec talks about `highest_message` as an integer; for an
        // empty base it's 0 so `highest_message + 1` gives the
        // first-message number (1) on first post.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        assert_eq!(store.highest_message(), 0);
    }

    #[test]
    fn re_opening_a_non_empty_store_recovers_highest_message_and_each_mail() {
        // The HighestMessageMatchesMaxNumber invariant must survive a
        // process restart: scanning the directory at open-time must
        // restore the cached high-water mark.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);

        let m1_handle;
        let m2_handle;
        let m3_handle;
        {
            let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
            m1_handle = store.insert(sample_draft()).unwrap();
            m2_handle = store.insert(sample_draft()).unwrap();
            m3_handle = store.insert(sample_draft()).unwrap();
        }

        let reopened = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        assert_eq!(reopened.highest_message(), 3);
        assert_eq!(reopened.load(1).unwrap().as_ref(), Some(&m1_handle));
        assert_eq!(reopened.load(2).unwrap().as_ref(), Some(&m2_handle));
        assert_eq!(reopened.load(3).unwrap().as_ref(), Some(&m3_handle));
    }

    #[test]
    fn re_opening_then_inserting_continues_numbering_from_highest_plus_one() {
        // A new insert after a re-open must NOT collide with existing
        // numbers — it must consume the cached high-water mark.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        {
            let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
            store.insert(sample_draft()).unwrap();
            store.insert(sample_draft()).unwrap();
        }
        let mut reopened = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let m3 = reopened.insert(sample_draft()).unwrap();
        assert_eq!(m3.number(), 3);
        assert_eq!(reopened.highest_message(), 3);
    }

    #[test]
    fn open_ignores_non_message_files_in_the_directory() {
        // Sysops may stash README, .DS_Store or hand-edited backups in
        // the msgbase directory. The scan must tolerate them.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"notes").unwrap();
        std::fs::write(dir.path().join("backup.bak"), b"junk").unwrap();
        std::fs::write(dir.path().join("abc.json"), b"not a number").unwrap();

        let msgbase = MessageBaseRef::new(2, 1);
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        assert_eq!(store.highest_message(), 0);
    }

    #[test]
    fn open_rejects_a_file_whose_payload_declares_a_different_number_than_its_filename() {
        // Defence against an operator (or a buggy migration) editing a
        // message file's number field. Surfacing the mismatch keeps
        // MessageNumbersUniquePerBase honest.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        // Hand-write a JSON file at 0000001.json that claims number=7.
        let path = dir.path().join("0000001.json");
        std::fs::write(
            &path,
            r#"{
                "conference_number": 2,
                "msgbase_number": 1,
                "number": 7,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T00:00:01Z",
                "received_at": null,
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let err = FileMailStore::open(dir.path().to_path_buf(), msgbase)
            .expect_err("number mismatch must be rejected");
        match err {
            MailStoreError::NumberMismatch {
                filename_number,
                payload_number,
                ..
            } => {
                assert_eq!(filename_number, 1);
                assert_eq!(payload_number, 7);
            }
            other => panic!("expected NumberMismatch, got {other:?}"),
        }
    }

    #[test]
    fn open_rejects_a_file_whose_payload_declares_a_different_msgbase() {
        // Defence against a message file being copied into the wrong
        // msgbase directory.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let path = dir.path().join("0000001.json");
        std::fs::write(
            &path,
            r#"{
                "conference_number": 9,
                "msgbase_number": 4,
                "number": 1,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T00:00:01Z",
                "received_at": null,
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let err = FileMailStore::open(dir.path().to_path_buf(), msgbase)
            .expect_err("msgbase mismatch must be rejected");
        assert!(matches!(err, MailStoreError::MsgbaseMismatch { .. }));
    }

    #[test]
    fn open_rejects_a_file_that_is_not_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        std::fs::write(dir.path().join("0000001.json"), b"not json").unwrap();
        let err = FileMailStore::open(dir.path().to_path_buf(), msgbase)
            .expect_err("malformed JSON must be rejected");
        assert!(matches!(err, MailStoreError::Malformed { .. }));
    }

    #[test]
    fn re_opening_an_empty_directory_succeeds_and_creates_it() {
        // `open` must work for a fresh BBS install where the per-msgbase
        // directory doesn't exist yet. The store auto-creates it.
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("Conf02").join("MsgBase");
        assert!(!nested.exists());
        let msgbase = MessageBaseRef::new(2, 1);
        let _store = FileMailStore::open(nested.clone(), msgbase).expect("open missing dir");
        assert!(nested.is_dir());
    }

    #[test]
    fn parse_message_filename_accepts_zero_padded_decimal_with_json_extension() {
        // Lexicographic and numeric ordering must agree on disk; the
        // zero-padded format guarantees that within the chosen width.
        assert_eq!(super::parse_message_filename("0000001.json"), Some(1));
        assert_eq!(super::parse_message_filename("0000042.json"), Some(42));
        assert_eq!(
            super::parse_message_filename("9999999.json"),
            Some(9_999_999)
        );
    }

    #[test]
    fn parse_message_filename_rejects_unrelated_names() {
        // Anything that isn't `<digits>.json` should be skipped silently
        // — sysops shouldn't be punished for stashing READMEs in here.
        assert_eq!(super::parse_message_filename("README"), None);
        assert_eq!(super::parse_message_filename(".DS_Store"), None);
        assert_eq!(super::parse_message_filename("abc.json"), None);
        assert_eq!(super::parse_message_filename("1.txt"), None);
        assert_eq!(super::parse_message_filename(".json"), None);
        assert_eq!(super::parse_message_filename("1.json.bak"), None);
    }

    // Bring MailStoreError into scope for the negative-path tests.
    use crate::domain::messaging::mail_store::MailStoreError;

    #[test]
    fn load_surfaces_io_errors_other_than_not_found() {
        // The NotFound branch of `load` quietly maps to `Ok(None)`.
        // Other IO errors (e.g. the path being occupied by a directory
        // — `IsADirectory` on Linux, `Other` on macOS) must NOT be
        // silently swallowed: callers need to see them.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        std::fs::create_dir(dir.path().join("0000005.json")).unwrap();
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let err = store.load(5).expect_err("reading a directory must fail");
        assert!(matches!(err, MailStoreError::Io(_)));
    }

    #[test]
    fn loading_a_message_with_a_recorded_received_at_round_trips_the_timestamp() {
        // The on-disk schema serialises `received_at` as an RFC 3339
        // string in UTC. Pin a representative pair so a regression that
        // dropped the conversion, swapped a sign, or used a constant
        // would observe.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let path = dir.path().join("0000001.json");
        std::fs::write(
            &path,
            r#"{
                "conference_number": 2,
                "msgbase_number": 1,
                "number": 1,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T00:01:40Z",
                "received_at": "1970-01-01T03:25:45Z",
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let mail = store.load(1).unwrap().expect("loaded");
        assert_eq!(mail.posted_at(), t(100));
        assert_eq!(mail.received_at(), Some(t(12_345)));
    }

    #[test]
    fn open_rejects_a_file_whose_conference_number_matches_but_msgbase_number_does_not() {
        // The MsgbaseMismatch guard must trip when EITHER field
        // disagrees, not only when both do. Catches a regression where
        // an `&&` instead of `||` would let half-matching coordinates
        // through.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let path = dir.path().join("0000001.json");
        std::fs::write(
            &path,
            r#"{
                "conference_number": 2,
                "msgbase_number": 7,
                "number": 1,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T00:00:01Z",
                "received_at": null,
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let err = FileMailStore::open(dir.path().to_path_buf(), msgbase)
            .expect_err("msgbase number mismatch must be rejected");
        match err {
            MailStoreError::MsgbaseMismatch {
                payload_conference,
                payload_msgbase,
                store_conference,
                store_msgbase,
                ..
            } => {
                assert_eq!(payload_conference, 2);
                assert_eq!(payload_msgbase, 7);
                assert_eq!(store_conference, 2);
                assert_eq!(store_msgbase, 1);
            }
            other => panic!("expected MsgbaseMismatch, got {other:?}"),
        }
    }

    #[test]
    fn open_rejects_a_file_whose_msgbase_number_matches_but_conference_number_does_not() {
        // Mirror of the previous test on the other axis.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let path = dir.path().join("0000001.json");
        std::fs::write(
            &path,
            r#"{
                "conference_number": 9,
                "msgbase_number": 1,
                "number": 1,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T00:00:01Z",
                "received_at": null,
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let err = FileMailStore::open(dir.path().to_path_buf(), msgbase)
            .expect_err("conference number mismatch must be rejected");
        assert!(matches!(err, MailStoreError::MsgbaseMismatch { .. }));
    }

    #[test]
    fn loading_a_message_with_a_non_utc_offset_normalises_to_the_underlying_instant() {
        // A sysop or migration tool may hand-write a file with a local
        // offset (e.g. `+02:00`). The store must accept it and
        // reconstruct the same instant as the corresponding UTC form,
        // honouring the timezone information rather than treating the
        // wall-clock digits as UTC.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let path = dir.path().join("0000001.json");
        // 1970-01-01T02:00:00+02:00 is the same instant as
        // 1970-01-01T00:00:00Z — i.e. UNIX_EPOCH itself.
        std::fs::write(
            &path,
            r#"{
                "conference_number": 2,
                "msgbase_number": 1,
                "number": 1,
                "visibility": "public",
                "from_name": "Sysop",
                "to_name": "alice",
                "broadcast_to": "none",
                "subject": "x",
                "posted_at": "1970-01-01T02:00:00+02:00",
                "received_at": null,
                "author_slot": 1,
                "addressee_slot": 2,
                "body": ""
            }"#,
        )
        .unwrap();
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let mail = store.load(1).unwrap().expect("loaded");
        assert_eq!(mail.posted_at(), t(0));
    }

    #[test]
    fn persisted_files_use_rfc3339_utc_strings_for_timestamps() {
        // Pin the on-disk wire format. A future change to the
        // serialisation strategy would have to update this assertion
        // explicitly and consider migration of existing data.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let draft = MailDraft {
            posted_at: t(86_400), // 1970-01-02T00:00:00Z
            ..sample_draft()
        };
        store.insert(draft).unwrap();
        let written = std::fs::read_to_string(dir.path().join("0000001.json")).unwrap();
        assert!(
            written.contains(r#""posted_at": "1970-01-02T00:00:00Z""#),
            "expected RFC 3339 UTC string for posted_at, got:\n{written}",
        );
        assert!(
            written.contains(r#""received_at": null"#),
            "expected null received_at on a freshly-posted mail, got:\n{written}",
        );
    }

    #[test]
    fn save_persists_received_at_so_reload_returns_it() {
        // Slice 39: ReadMail's `mail.received_at = now` consequent
        // must survive a reload. Without `save`, the timestamp lived
        // only in memory and the rule's effect would not persist.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let mut mail = store.insert(sample_draft()).expect("insert");
        mail.mark_received(t(500)).expect("mark");
        store.save(&mail).expect("save");

        let reopened = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let loaded = reopened.load(mail.number()).unwrap().expect("present");
        assert_eq!(loaded.received_at(), Some(t(500)));
    }

    #[test]
    fn save_rejects_a_mail_whose_msgbase_disagrees_with_the_store() {
        // Defence against a caller handing us a mail loaded from a
        // different msgbase: writing it under our directory would
        // silently break the on-disk MessageBaseRef coordinate.
        use crate::domain::messaging::mail::{Mail, NewMail};
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        store.insert(sample_draft()).expect("insert");

        // Build a Mail whose msgbase claims (9,4). We can do this by
        // constructing it directly rather than via the store.
        let foreign = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(9, 4),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "x".to_string(),
            to_name: "y".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "x".to_string(),
            posted_at: t(0),
            author_slot: 1,
            addressee_slot: Some(2),
            body: String::new(),
        });
        let err = store.save(&foreign).expect_err("msgbase mismatch");
        assert!(matches!(err, MailStoreError::MsgbaseMismatch { .. }));
    }

    #[test]
    fn save_can_be_followed_by_a_subsequent_insert_that_still_increments() {
        // `save` updates an existing record; it must not perturb the
        // cached high-water mark used by `insert`.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let mut store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let mut m1 = store.insert(sample_draft()).expect("insert");
        m1.mark_received(t(500)).unwrap();
        store.save(&m1).expect("save");
        let m2 = store.insert(sample_draft()).expect("insert again");
        assert_eq!(m2.number(), 2);
        assert_eq!(store.highest_message(), 2);
    }

    #[test]
    fn try_lock_succeeds_on_a_fresh_store() {
        // Spec messaging.allium PostMail: `let lock = lock_msgbase(visit.msgbase)`
        // is true when nothing else holds the lock.
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let store = FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap();
        let guard = store.try_lock();
        assert!(guard.is_some(), "lock_msgbase on idle store must succeed");
    }

    #[test]
    fn try_lock_fails_while_another_holder_has_the_lock() {
        // Two sessions on the same node holding Arc<FileMailStore>: the
        // second's `try_lock` must observe `false` per the spec's
        // `requires: lock` precondition on PostMail.
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let msgbase = MessageBaseRef::new(2, 1);
        let store = Arc::new(FileMailStore::open(dir.path().to_path_buf(), msgbase).unwrap());
        let g1 = store.try_lock().expect("first holder acquires");
        assert!(
            store.try_lock().is_none(),
            "second holder must fail while g1 is alive",
        );
        drop(g1);
        let g3 = store.try_lock();
        assert!(g3.is_some(), "after release the lock is available again");
    }
}
