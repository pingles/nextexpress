//! SQLite-backed [`UserRepository`] (see [`designs/USERS.md`]).
//!
//! Production adapter — the binary picks this when
//! `config.user_storage` names a file path. Tests stick with
//! [`crate::adapters::in_memory_user_repository::InMemoryUserRepository`]
//! so they don't need the file system or `rusqlite` to set up a
//! fixture.
//!
//! v1 implements the existing [`UserRepository`] port shape
//! (`save(User)`, `create_user`, lookups). The
//! design's eventual command-style writes (deltas, patches,
//! reservations) are out of scope here — they enter the picture when
//! the port itself grows them.
//!
//! [`designs/USERS.md`]: ../../../../designs/USERS.md
//! [`UserRepository`]: crate::domain::user_repository::UserRepository

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::conference::{ConferenceMembership, MessageBaseRef, ScanFlag};
use crate::domain::messaging::read_pointers::ReadPointers;
use crate::domain::password::PasswordHashKind;
use crate::domain::user::{
    AuthOutcome, DailyBudgetOutcome, MembershipPatch, NewUserDraft, PasswordChange, PersistedUser,
    RatioMode, User, UserError, UserFlag, UserPatch,
};
use crate::domain::user_repository::{
    NameLookupResult, UserCreationError, UserRepository, UserRepositoryError,
};

/// Errors returned when opening a [`SqliteUserRepository`].
#[derive(Debug, thiserror::Error)]
pub enum SqliteUserRepositoryError {
    /// The underlying [`rusqlite::Connection::open`] call failed
    /// (missing parent directory, permissions, locked file, etc.).
    #[error("couldn't open user database {}: {error}", path.display())]
    Open {
        /// The path that was attempted.
        path: PathBuf,
        /// The underlying [`rusqlite::Error`].
        #[source]
        error: rusqlite::Error,
    },
    /// An initial pragma or schema-creation statement failed.
    #[error("couldn't initialise user database: {0}")]
    Schema(#[source] rusqlite::Error),
}

/// `rusqlite`-backed [`UserRepository`].
///
/// Owns a single connection behind a [`Mutex`]; the BBS workload is
/// dominated by sub-second telnet round-trips so contention here is
/// not the bottleneck. WAL mode is enabled for crash safety; foreign
/// keys are on so the `ON DELETE CASCADE` on memberships and read
/// pointers does the right thing.
pub struct SqliteUserRepository {
    conn: Mutex<Connection>,
}

impl SqliteUserRepository {
    /// Opens a `SQLite` database at `path`, creating the file and the
    /// schema if necessary.
    ///
    /// # Errors
    /// Returns [`SqliteUserRepositoryError::Open`] if the connection
    /// can't be created, or [`SqliteUserRepositoryError::Schema`] if
    /// pragma setup or schema creation fails.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, SqliteUserRepositoryError> {
        let path = path.as_ref();
        let conn = Connection::open(path).map_err(|error| SqliteUserRepositoryError::Open {
            path: path.to_path_buf(),
            error,
        })?;
        Self::init_schema(&conn).map_err(SqliteUserRepositoryError::Schema)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Opens an in-memory `SQLite` database, used by the adapter's own
    /// tests so they don't touch the file system.
    ///
    /// # Errors
    /// Returns [`SqliteUserRepositoryError::Schema`] if schema setup
    /// fails (should never happen for an in-memory connection).
    pub fn in_memory() -> Result<Self, SqliteUserRepositoryError> {
        let conn = Connection::open_in_memory().map_err(SqliteUserRepositoryError::Schema)?;
        Self::init_schema(&conn).map_err(SqliteUserRepositoryError::Schema)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS users (
                 slot_number              INTEGER PRIMARY KEY,
                 handle                   TEXT    NOT NULL,
                 handle_folded            TEXT    NOT NULL UNIQUE,
                 password_hash_kind       TEXT    NOT NULL,
                 password_hash            TEXT    NOT NULL,
                 password_salt            TEXT,
                 password_last_updated    INTEGER NOT NULL,
                 force_password_reset     INTEGER NOT NULL DEFAULT 0,
                 access_level             INTEGER NOT NULL,
                 invalid_attempts         INTEGER NOT NULL DEFAULT 0,
                 account_locked           INTEGER NOT NULL DEFAULT 0,
                 is_new_user              INTEGER NOT NULL DEFAULT 0,
                 censored                 INTEGER NOT NULL DEFAULT 0,
                 times_called             INTEGER NOT NULL DEFAULT 0,
                 times_called_today       INTEGER NOT NULL DEFAULT 0,
                 last_call                INTEGER,
                 time_limit_per_call_secs INTEGER NOT NULL DEFAULT 0,
                 time_limit_per_day_secs  INTEGER NOT NULL DEFAULT 0,
                 time_used_today_secs     INTEGER NOT NULL DEFAULT 0,
                 location                 TEXT,
                 phone_number             TEXT,
                 email                    TEXT,
                 line_length              INTEGER NOT NULL DEFAULT 0,
                 ansi_colour              INTEGER NOT NULL DEFAULT 0,
                 expert_mode              INTEGER NOT NULL DEFAULT 0,
                 account_created          INTEGER NOT NULL,
                 flags                    INTEGER NOT NULL DEFAULT 0,
                 ratio_mode               TEXT    NOT NULL,
                 ratio_value              INTEGER NOT NULL DEFAULT 0,
                 messages_posted          INTEGER NOT NULL DEFAULT 0,
                 last_joined_conference   INTEGER,
                 last_joined_msgbase      INTEGER,
                 CHECK ((last_joined_conference IS NULL) = (last_joined_msgbase IS NULL))
             );
             CREATE TABLE IF NOT EXISTS conference_memberships (
                 slot_number       INTEGER NOT NULL REFERENCES users(slot_number) ON DELETE CASCADE,
                 conference_number INTEGER NOT NULL,
                 granted           INTEGER NOT NULL,
                 messages_posted   INTEGER NOT NULL DEFAULT 0,
                 mail_scan         INTEGER NOT NULL DEFAULT 1,
                 mailscan_all      INTEGER NOT NULL DEFAULT 0,
                 file_scan         INTEGER NOT NULL DEFAULT 1,
                 zoom_scan         INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (slot_number, conference_number)
             );
             CREATE TABLE IF NOT EXISTS read_pointers (
                 slot_number       INTEGER NOT NULL,
                 conference_number INTEGER NOT NULL,
                 msgbase_number    INTEGER NOT NULL,
                 last_read         INTEGER NOT NULL DEFAULT 0,
                 last_scanned      INTEGER NOT NULL DEFAULT 0,
                 new_since         INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (slot_number, conference_number, msgbase_number),
                 FOREIGN KEY (slot_number, conference_number)
                     REFERENCES conference_memberships(slot_number, conference_number)
                     ON DELETE CASCADE,
                 CHECK (last_read <= last_scanned)
             );",
        )
    }

    /// Returns true when no `users` row exists.
    ///
    /// Used by the composition root to decide whether to write the
    /// default sysop seed on first boot.
    ///
    /// # Errors
    /// Returns the underlying [`rusqlite::Error`] if the count query
    /// fails.
    ///
    /// # Panics
    /// Panics if the connection mutex has been poisoned by an earlier
    /// panicking writer.
    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().expect("user db mutex");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(count == 0)
    }

    /// Inserts `user` directly, bypassing the slot-allocation logic.
    ///
    /// Intended only for one-time bootstrap of seed data (e.g. the
    /// default sysop on first boot). Regular user creation must go
    /// through [`UserRepository::create_user`] so the
    /// repository owns slot allocation.
    ///
    /// # Errors
    /// Returns the underlying [`rusqlite::Error`] if the insert fails.
    ///
    /// # Panics
    /// Panics if the connection mutex has been poisoned by an earlier
    /// panicking writer.
    pub fn insert_seed(&self, user: &User) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().expect("user db mutex");
        let tx = conn.transaction()?;
        Self::upsert_user(&tx, user)?;
        tx.commit()
    }

    /// Maps "UPDATE matched zero rows" to the port's `UserNotFound`.
    fn require_row(changed: usize, slot: u32) -> Result<(), UserRepositoryError> {
        if changed == 0 {
            return Err(UserRepositoryError::UserNotFound {
                handle: format!("slot {slot}"),
            });
        }
        Ok(())
    }

    /// Applies one [`MembershipPatch`] inside an open patch
    /// transaction: optional row creation, field patch, then pointer
    /// `MAX`-upserts (in that order — the pointer rows' composite
    /// foreign key requires the membership row to exist first).
    fn apply_membership_patch(
        tx: &Connection,
        slot: u32,
        membership: &MembershipPatch,
    ) -> rusqlite::Result<()> {
        if membership.create_if_missing {
            tx.execute(
                "INSERT INTO conference_memberships (
                     slot_number, conference_number, granted, messages_posted,
                     mail_scan, mailscan_all, file_scan, zoom_scan
                 ) VALUES (?1, ?2, ?3, 0, 1, 0, 1, 0)
                 ON CONFLICT(slot_number, conference_number) DO NOTHING",
                params![
                    slot,
                    membership.conference_number,
                    i64::from(membership.granted.unwrap_or(true)),
                ],
            )?;
        }
        let flags = membership.scan_flags;
        tx.execute(
            "UPDATE conference_memberships SET
                 granted = COALESCE(?3, granted),
                 messages_posted = messages_posted + ?4,
                 mail_scan = COALESCE(?5, mail_scan),
                 mailscan_all = COALESCE(?6, mailscan_all),
                 file_scan = COALESCE(?7, file_scan),
                 zoom_scan = COALESCE(?8, zoom_scan)
             WHERE slot_number = ?1 AND conference_number = ?2",
            params![
                slot,
                membership.conference_number,
                membership.granted.map(i64::from),
                membership.messages_posted_delta,
                flags.map(|f| i64::from(f.mail_scan)),
                flags.map(|f| i64::from(f.mailscan_all)),
                flags.map(|f| i64::from(f.file_scan)),
                flags.map(|f| i64::from(f.zoom_scan)),
            ],
        )?;
        for pointer in &membership.pointers {
            // `new_since` is deliberately absent from the DO UPDATE:
            // an existing row keeps its own.
            tx.execute(
                "INSERT INTO read_pointers (
                     slot_number, conference_number, msgbase_number,
                     last_read, last_scanned, new_since
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(slot_number, conference_number, msgbase_number) DO UPDATE SET
                     last_read = MAX(last_read, excluded.last_read),
                     last_scanned = MAX(last_scanned, excluded.last_scanned)",
                params![
                    slot,
                    membership.conference_number,
                    pointer.msgbase_number,
                    pointer.last_read,
                    pointer.last_scanned,
                    system_time_to_secs(pointer.new_since),
                ],
            )?;
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "single SQL upsert spans many columns"
    )]
    fn upsert_user(conn: &Connection, user: &User) -> rusqlite::Result<()> {
        let snapshot = user.to_persisted();
        let handle_folded = snapshot.handle.to_ascii_lowercase();
        conn.execute(
            "INSERT INTO users (
                 slot_number, handle, handle_folded,
                 password_hash_kind, password_hash, password_salt,
                 password_last_updated, force_password_reset,
                 access_level, invalid_attempts, account_locked,
                 is_new_user, censored,
                 times_called, times_called_today, last_call,
                 time_limit_per_call_secs, time_limit_per_day_secs,
                 time_used_today_secs,
                 location, phone_number, email,
                 line_length, ansi_colour, account_created, flags,
                 ratio_mode, ratio_value, messages_posted,
                 last_joined_conference, last_joined_msgbase, expert_mode
             ) VALUES (
                 ?1, ?2, ?3,
                 ?4, ?5, ?6,
                 ?7, ?8,
                 ?9, ?10, ?11,
                 ?12, ?13,
                 ?14, ?15, ?16,
                 ?17, ?18,
                 ?19,
                 ?20, ?21, ?22,
                 ?23, ?24, ?25, ?26,
                 ?27, ?28, ?29,
                 ?30, ?31, ?32
             )
             ON CONFLICT(slot_number) DO UPDATE SET
                 handle = excluded.handle,
                 handle_folded = excluded.handle_folded,
                 password_hash_kind = excluded.password_hash_kind,
                 password_hash = excluded.password_hash,
                 password_salt = excluded.password_salt,
                 password_last_updated = excluded.password_last_updated,
                 force_password_reset = excluded.force_password_reset,
                 access_level = excluded.access_level,
                 invalid_attempts = excluded.invalid_attempts,
                 account_locked = excluded.account_locked,
                 is_new_user = excluded.is_new_user,
                 censored = excluded.censored,
                 times_called = excluded.times_called,
                 times_called_today = excluded.times_called_today,
                 last_call = excluded.last_call,
                 time_limit_per_call_secs = excluded.time_limit_per_call_secs,
                 time_limit_per_day_secs = excluded.time_limit_per_day_secs,
                 time_used_today_secs = excluded.time_used_today_secs,
                 location = excluded.location,
                 phone_number = excluded.phone_number,
                 email = excluded.email,
                 line_length = excluded.line_length,
                 ansi_colour = excluded.ansi_colour,
                 account_created = excluded.account_created,
                 flags = excluded.flags,
                 ratio_mode = excluded.ratio_mode,
                 ratio_value = excluded.ratio_value,
                 messages_posted = excluded.messages_posted,
                 last_joined_conference = excluded.last_joined_conference,
                 last_joined_msgbase = excluded.last_joined_msgbase,
                 expert_mode = excluded.expert_mode",
            params![
                snapshot.slot_number,
                snapshot.handle,
                handle_folded,
                hash_kind_to_str(snapshot.password_hash_kind),
                snapshot.password_hash,
                snapshot.password_salt,
                system_time_to_secs(snapshot.password_last_updated),
                i64::from(snapshot.force_password_reset),
                i64::from(snapshot.access_level),
                snapshot.invalid_attempts,
                i64::from(snapshot.account_locked),
                i64::from(snapshot.is_new_user),
                i64::from(snapshot.censored),
                snapshot.times_called,
                snapshot.times_called_today,
                snapshot.last_call.map(system_time_to_secs),
                duration_to_secs(snapshot.time_limit_per_call),
                duration_to_secs(snapshot.time_limit_per_day),
                duration_to_secs(snapshot.time_used_today),
                snapshot.location,
                snapshot.phone_number,
                snapshot.email,
                snapshot.line_length,
                i64::from(snapshot.ansi_colour),
                system_time_to_secs(snapshot.account_created),
                flags_to_bitmask(&snapshot.flags),
                ratio_mode_to_str(snapshot.ratio_mode),
                snapshot.ratio_value,
                snapshot.messages_posted,
                snapshot.last_joined.map(|r| r.conference_number()),
                snapshot.last_joined.map(|r| r.msgbase_number()),
                i64::from(snapshot.expert_mode),
            ],
        )?;

        // Replace membership and pointer rows from scratch. v1 takes the
        // simple approach — `save(User)` is the only entry point and
        // hands us the full record, so a clean overwrite mirrors what
        // the in-memory adapter does without trying to diff.
        conn.execute(
            "DELETE FROM conference_memberships WHERE slot_number = ?1",
            params![snapshot.slot_number],
        )?;
        for membership in &snapshot.memberships {
            conn.execute(
                "INSERT INTO conference_memberships (
                     slot_number, conference_number, granted, messages_posted,
                     mail_scan, mailscan_all, file_scan, zoom_scan
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    snapshot.slot_number,
                    membership.conference_number(),
                    i64::from(membership.is_granted()),
                    membership.messages_posted(),
                    i64::from(membership.scan_flag(ScanFlag::MailScan)),
                    i64::from(membership.scan_flag(ScanFlag::MailScanAll)),
                    i64::from(membership.scan_flag(ScanFlag::FileScan)),
                    i64::from(membership.scan_flag(ScanFlag::Zoom)),
                ],
            )?;
            for pointer in membership.pointers() {
                conn.execute(
                    "INSERT INTO read_pointers (
                         slot_number, conference_number, msgbase_number,
                         last_read, last_scanned, new_since
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        snapshot.slot_number,
                        membership.conference_number(),
                        pointer.msgbase_number(),
                        pointer.last_read(),
                        pointer.last_scanned(),
                        system_time_to_secs(pointer.new_since()),
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn load_user(conn: &Connection, slot: u32) -> rusqlite::Result<Option<User>> {
        let snapshot = conn
            .query_row(
                "SELECT slot_number, handle, password_hash_kind, password_hash,
                        password_salt, password_last_updated, force_password_reset,
                        access_level, invalid_attempts, account_locked,
                        is_new_user, censored,
                        times_called, times_called_today, last_call,
                        time_limit_per_call_secs, time_limit_per_day_secs,
                        time_used_today_secs,
                        location, phone_number, email,
                        line_length, ansi_colour, account_created, flags,
                        ratio_mode, ratio_value, messages_posted,
                        last_joined_conference, last_joined_msgbase, expert_mode
                 FROM users WHERE slot_number = ?1",
                params![slot],
                row_to_partial_snapshot,
            )
            .optional()?;
        let Some(mut snapshot) = snapshot else {
            return Ok(None);
        };
        snapshot.memberships = Self::load_memberships(conn, snapshot.slot_number)?;
        let user = User::from_persisted(snapshot)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(UserBuildError(e))))?;
        Ok(Some(user))
    }

    fn load_memberships(
        conn: &Connection,
        slot_number: u32,
    ) -> rusqlite::Result<Vec<ConferenceMembership>> {
        #[allow(clippy::struct_excessive_bools)] // mirrors the row's flag columns
        struct MembershipRow {
            conference_number: u32,
            granted: bool,
            messages_posted: u32,
            mail_scan: bool,
            mailscan_all: bool,
            file_scan: bool,
            zoom_scan: bool,
        }
        let mut stmt = conn.prepare(
            "SELECT conference_number, granted, messages_posted,
                    mail_scan, mailscan_all, file_scan, zoom_scan
             FROM conference_memberships
             WHERE slot_number = ?1
             ORDER BY conference_number",
        )?;
        let raw_rows = stmt
            .query_map(params![slot_number], |row| {
                Ok(MembershipRow {
                    conference_number: row.get::<_, u32>(0)?,
                    granted: row.get::<_, i64>(1)? != 0,
                    messages_posted: row.get::<_, u32>(2)?,
                    mail_scan: row.get::<_, i64>(3)? != 0,
                    mailscan_all: row.get::<_, i64>(4)? != 0,
                    file_scan: row.get::<_, i64>(5)? != 0,
                    zoom_scan: row.get::<_, i64>(6)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut memberships = Vec::with_capacity(raw_rows.len());
        for row in raw_rows {
            let mut membership = ConferenceMembership::new(row.conference_number, row.granted);
            for _ in 0..row.messages_posted {
                membership.bump_messages_posted();
            }
            membership.set_scan_flag(ScanFlag::MailScan, row.mail_scan);
            membership.set_scan_flag(ScanFlag::MailScanAll, row.mailscan_all);
            membership.set_scan_flag(ScanFlag::FileScan, row.file_scan);
            membership.set_scan_flag(ScanFlag::Zoom, row.zoom_scan);
            for pointer in Self::load_pointers(conn, slot_number, row.conference_number)? {
                membership.upsert_pointers(pointer);
            }
            memberships.push(membership);
        }
        Ok(memberships)
    }

    fn load_pointers(
        conn: &Connection,
        slot_number: u32,
        conference_number: u32,
    ) -> rusqlite::Result<Vec<ReadPointers>> {
        let mut stmt = conn.prepare(
            "SELECT msgbase_number, last_read, last_scanned, new_since
             FROM read_pointers
             WHERE slot_number = ?1 AND conference_number = ?2
             ORDER BY msgbase_number",
        )?;
        let pointers = stmt
            .query_map(params![slot_number, conference_number], |row| {
                let msgbase_number: u32 = row.get(0)?;
                let last_read: u32 = row.get(1)?;
                let last_scanned: u32 = row.get(2)?;
                let new_since_secs: i64 = row.get(3)?;
                ReadPointers::new(
                    msgbase_number,
                    last_read,
                    last_scanned,
                    secs_to_system_time(new_since_secs),
                )
                .map_err(|e| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(PointerBuildError(e)))
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pointers)
    }

    fn find_by_handle_inner(conn: &Connection, typed: &str) -> rusqlite::Result<Option<User>> {
        let folded = typed.to_ascii_lowercase();
        let slot: Option<u32> = conn
            .query_row(
                "SELECT slot_number FROM users WHERE handle_folded = ?1",
                params![folded],
                |row| row.get(0),
            )
            .optional()?;
        match slot {
            Some(slot) => Self::load_user(conn, slot),
            None => Ok(None),
        }
    }
}

impl UserRepository for SqliteUserRepository {
    fn find_by_handle(&self, typed: &str) -> Result<NameLookupResult, UserRepositoryError> {
        let conn = self.conn.lock().expect("user db mutex");
        match Self::find_by_handle_inner(&conn, typed) {
            Ok(Some(user)) => Ok(NameLookupResult::Found(Box::new(user))),
            Ok(None) => Ok(NameLookupResult::NotFound),
            Err(error) => Err(UserRepositoryError::storage("lookup", error)),
        }
    }

    fn find_sysop(&self) -> Result<NameLookupResult, UserRepositoryError> {
        let conn = self.conn.lock().expect("user db mutex");
        match Self::load_user(&conn, 1) {
            Ok(Some(user)) => Ok(NameLookupResult::Found(Box::new(user))),
            Ok(None) => Ok(NameLookupResult::NotFound),
            Err(error) => Err(UserRepositoryError::storage("lookup sysop", error)),
        }
    }

    fn save(&self, user: User) -> Result<(), UserRepositoryError> {
        let conn = self.conn.lock().expect("user db mutex");
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM users WHERE slot_number = ?1",
                params![user.slot_number()],
                |_| Ok(true),
            )
            .optional()
            .map_err(|error| UserRepositoryError::storage("save lookup", error))?
            .unwrap_or(false);
        if !exists {
            return Err(UserRepositoryError::UserNotFound {
                handle: user.handle().to_string(),
            });
        }
        Self::upsert_user(&conn, &user).map_err(|error| UserRepositoryError::storage("save", error))
    }

    fn record_auth_outcome(
        &self,
        slot: u32,
        outcome: &AuthOutcome,
    ) -> Result<(), UserRepositoryError> {
        let conn = self.conn.lock().expect("user db mutex");
        let changed = match outcome {
            AuthOutcome::Matched {
                daily,
                force_password_reset,
            } => {
                let daily_sql = match daily {
                    Some(DailyBudgetOutcome::NewDay) => {
                        "times_called_today = 0, time_used_today_secs = 0,"
                    }
                    Some(DailyBudgetOutcome::SameDay) => {
                        "times_called_today = times_called_today + 1,"
                    }
                    None => "",
                };
                let sql = format!(
                    "UPDATE users SET {daily_sql}
                         invalid_attempts = 0,
                         force_password_reset = MAX(force_password_reset, ?2)
                     WHERE slot_number = ?1"
                );
                conn.execute(&sql, params![slot, i64::from(*force_password_reset)])
            }
            AuthOutcome::Mismatched { lock_account } => conn.execute(
                "UPDATE users SET
                     invalid_attempts = invalid_attempts + 1,
                     account_locked = MAX(account_locked, ?2)
                 WHERE slot_number = ?1",
                params![slot, i64::from(*lock_account)],
            ),
        }
        .map_err(|error| UserRepositoryError::storage("record auth outcome", error))?;
        Self::require_row(changed, slot)
    }

    fn record_password_change(
        &self,
        slot: u32,
        change: &PasswordChange,
    ) -> Result<(), UserRepositoryError> {
        let conn = self.conn.lock().expect("user db mutex");
        let changed = conn
            .execute(
                "UPDATE users SET
                     password_hash = ?2,
                     password_salt = ?3,
                     password_hash_kind = ?4,
                     password_last_updated = ?5,
                     force_password_reset = 0
                 WHERE slot_number = ?1",
                params![
                    slot,
                    change.hash,
                    change.salt,
                    hash_kind_to_str(change.kind),
                    system_time_to_secs(change.changed_at),
                ],
            )
            .map_err(|error| UserRepositoryError::storage("record password change", error))?;
        Self::require_row(changed, slot)
    }

    fn apply_user_patch(&self, slot: u32, patch: &UserPatch) -> Result<(), UserRepositoryError> {
        let mut conn = self.conn.lock().expect("user db mutex");
        let tx = conn
            .transaction()
            .map_err(|error| UserRepositoryError::storage("apply user patch", error))?;
        // Always runs, even for all-zero deltas: it doubles as the
        // existence check.
        let changed = tx
            .execute(
                "UPDATE users SET
                     times_called = times_called + ?2,
                     times_called_today = times_called_today + ?3,
                     time_used_today_secs = time_used_today_secs + ?4,
                     messages_posted = messages_posted + ?5,
                     last_call = CASE
                         WHEN ?6 IS NULL THEN last_call
                         WHEN last_call IS NULL THEN ?6
                         ELSE MAX(last_call, ?6) END,
                     expert_mode = COALESCE(?7, expert_mode),
                     flags = COALESCE(?8, flags),
                     last_joined_conference = COALESCE(?9, last_joined_conference),
                     last_joined_msgbase = COALESCE(?10, last_joined_msgbase)
                 WHERE slot_number = ?1",
                params![
                    slot,
                    patch.times_called_delta,
                    patch.times_called_today_delta,
                    duration_to_secs(patch.time_used_today_delta),
                    patch.messages_posted_delta,
                    patch.last_call.map(system_time_to_secs),
                    patch.expert_mode.map(i64::from),
                    patch.flags.as_ref().map(flags_to_bitmask),
                    patch.last_joined.map(|r| r.conference_number()),
                    patch.last_joined.map(|r| r.msgbase_number()),
                ],
            )
            .map_err(|error| UserRepositoryError::storage("apply user patch", error))?;
        Self::require_row(changed, slot)?;
        for membership in &patch.memberships {
            Self::apply_membership_patch(&tx, slot, membership)
                .map_err(|error| UserRepositoryError::storage("apply membership patch", error))?;
        }
        tx.commit()
            .map_err(|error| UserRepositoryError::storage("apply user patch", error))
    }

    fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError> {
        let mut conn = self.conn.lock().expect("user db mutex");
        let tx = conn
            .transaction()
            .map_err(|error| UserCreationError::storage("begin transaction", error))?;
        let folded_handle = draft.handle.to_ascii_lowercase();
        let dup_handle: bool = tx
            .query_row(
                "SELECT 1 FROM users WHERE handle_folded = ?1",
                params![folded_handle],
                |_| Ok(true),
            )
            .optional()
            .map_err(|error| UserCreationError::storage("duplicate check", error))?
            .unwrap_or(false);
        if dup_handle {
            return Err(UserCreationError::DuplicateUser {
                handle: draft.handle.clone(),
            });
        }
        let next_slot: u32 = tx
            .query_row(
                "SELECT COALESCE(MAX(slot_number), 0) + 1 FROM users",
                [],
                |row| row.get(0),
            )
            .map_err(|error| UserCreationError::storage("allocate slot", error))?;
        let user = User::register_new(next_slot, draft)?;
        Self::upsert_user(&tx, &user)
            .map_err(|error| UserCreationError::storage("insert user", error))?;
        tx.commit()
            .map_err(|error| UserCreationError::storage("commit user", error))?;
        Ok(user)
    }
}

// ─── Adapters between domain types and SQLite primitive values ───

fn hash_kind_to_str(kind: PasswordHashKind) -> &'static str {
    match kind {
        PasswordHashKind::Pbkdf210000 => "pbkdf2_10000",
    }
}

fn str_to_hash_kind(s: &str) -> rusqlite::Result<PasswordHashKind> {
    match s {
        "pbkdf2_10000" => Ok(PasswordHashKind::Pbkdf210000),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(InvalidHashKind(other.to_string())),
        )),
    }
}

fn ratio_mode_to_str(mode: RatioMode) -> &'static str {
    match mode {
        RatioMode::Disabled => "disabled",
        RatioMode::ByFiles => "by_files",
        RatioMode::ByBytes => "by_bytes",
    }
}

fn str_to_ratio_mode(s: &str) -> rusqlite::Result<RatioMode> {
    match s {
        "disabled" => Ok(RatioMode::Disabled),
        "by_files" => Ok(RatioMode::ByFiles),
        "by_bytes" => Ok(RatioMode::ByBytes),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(InvalidRatioMode(other.to_string())),
        )),
    }
}

fn system_time_to_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        // Times before the UNIX epoch are stored as negative seconds.
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

fn secs_to_system_time(secs: i64) -> SystemTime {
    if secs >= 0 {
        UNIX_EPOCH + Duration::from_secs(secs.unsigned_abs())
    } else {
        UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs())
    }
}

fn duration_to_secs(d: Duration) -> i64 {
    i64::try_from(d.as_secs()).unwrap_or(i64::MAX)
}

fn secs_to_duration(secs: i64) -> Duration {
    Duration::from_secs(u64::try_from(secs.max(0)).unwrap_or(0))
}

fn flag_bit(flag: UserFlag) -> u32 {
    match flag {
        UserFlag::ShowNewUserMessage => 1 << 0,
        UserFlag::AutoJoinFirstConf => 1 << 1,
        UserFlag::ShowOneTimeMessages => 1 << 2,
        UserFlag::ScreenClearAfterMessage => 1 << 3,
        UserFlag::IsDonor => 1 << 4,
        UserFlag::EditorFullScreen => 1 << 5,
        UserFlag::EditorPrompts => 1 << 6,
        UserFlag::BackgroundFileCheck => 1 << 7,
    }
}

const ALL_USER_FLAGS: [UserFlag; 8] = [
    UserFlag::ShowNewUserMessage,
    UserFlag::AutoJoinFirstConf,
    UserFlag::ShowOneTimeMessages,
    UserFlag::ScreenClearAfterMessage,
    UserFlag::IsDonor,
    UserFlag::EditorFullScreen,
    UserFlag::EditorPrompts,
    UserFlag::BackgroundFileCheck,
];

fn flags_to_bitmask(flags: &BTreeSet<UserFlag>) -> u32 {
    flags
        .iter()
        .copied()
        .map(flag_bit)
        .fold(0, |acc, b| acc | b)
}

fn bitmask_to_flags(bits: u32) -> BTreeSet<UserFlag> {
    ALL_USER_FLAGS
        .into_iter()
        .filter(|f| bits & flag_bit(*f) != 0)
        .collect()
}

fn row_to_partial_snapshot(row: &Row<'_>) -> rusqlite::Result<PersistedUser> {
    let slot_number: u32 = row.get(0)?;
    let handle: String = row.get(1)?;
    let password_hash_kind: String = row.get(2)?;
    let password_hash: String = row.get(3)?;
    let password_salt: Option<String> = row.get(4)?;
    let password_last_updated: i64 = row.get(5)?;
    let force_password_reset: i64 = row.get(6)?;
    let access_level: i64 = row.get(7)?;
    let invalid_attempts: u32 = row.get(8)?;
    let account_locked: i64 = row.get(9)?;
    let is_new_user: i64 = row.get(10)?;
    let censored: i64 = row.get(11)?;
    let times_called: u32 = row.get(12)?;
    let times_called_today: u32 = row.get(13)?;
    let last_call: Option<i64> = row.get(14)?;
    let time_limit_per_call_secs: i64 = row.get(15)?;
    let time_limit_per_day_secs: i64 = row.get(16)?;
    let time_used_today_secs: i64 = row.get(17)?;
    let location: Option<String> = row.get(18)?;
    let phone_number: Option<String> = row.get(19)?;
    let email: Option<String> = row.get(20)?;
    let line_length: u32 = row.get(21)?;
    let ansi_colour: i64 = row.get(22)?;
    let account_created: i64 = row.get(23)?;
    let flags_bits: u32 = row.get(24)?;
    let ratio_mode: String = row.get(25)?;
    let ratio_value: u32 = row.get(26)?;
    let messages_posted: u32 = row.get(27)?;
    let last_joined_conference: Option<u32> = row.get(28)?;
    let last_joined_msgbase: Option<u32> = row.get(29)?;
    let expert_mode: i64 = row.get(30)?;

    let last_joined = match (last_joined_conference, last_joined_msgbase) {
        (Some(c), Some(m)) => Some(MessageBaseRef::new(c, m)),
        _ => None,
    };
    let access_level_u8 = u8::try_from(access_level).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Integer,
            Box::new(OutOfRange("access_level")),
        )
    })?;
    Ok(PersistedUser {
        slot_number,
        handle,
        password_hash_kind: str_to_hash_kind(&password_hash_kind)?,
        password_hash,
        password_salt,
        password_last_updated: secs_to_system_time(password_last_updated),
        force_password_reset: force_password_reset != 0,
        access_level: access_level_u8,
        invalid_attempts,
        account_locked: account_locked != 0,
        is_new_user: is_new_user != 0,
        censored: censored != 0,
        times_called,
        times_called_today,
        last_call: last_call.map(secs_to_system_time),
        time_limit_per_call: secs_to_duration(time_limit_per_call_secs),
        time_limit_per_day: secs_to_duration(time_limit_per_day_secs),
        time_used_today: secs_to_duration(time_used_today_secs),
        location,
        phone_number,
        email,
        line_length,
        ansi_colour: ansi_colour != 0,
        expert_mode: expert_mode != 0,
        account_created: secs_to_system_time(account_created),
        flags: bitmask_to_flags(flags_bits),
        ratio_mode: str_to_ratio_mode(&ratio_mode)?,
        ratio_value,
        // Memberships and pointers are filled in by `load_user` after
        // the base row has been read.
        memberships: Vec::new(),
        last_joined,
        messages_posted,
    })
}

// ─── Boxed error wrappers for SQLite's conversion-error vocabulary ──

#[derive(Debug, thiserror::Error)]
#[error("unknown password_hash_kind: {0}")]
struct InvalidHashKind(String);

#[derive(Debug, thiserror::Error)]
#[error("unknown ratio_mode: {0}")]
struct InvalidRatioMode(String);

#[derive(Debug, thiserror::Error)]
#[error("{0} value out of range for u8")]
struct OutOfRange(&'static str);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct UserBuildError(UserError);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct PointerBuildError(crate::domain::messaging::read_pointers::ReadPointersError);

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::domain::conference::ConferenceMembership;
    use crate::domain::user::{NewUserDraft, UserFlag};

    fn draft_with(handle: &str) -> NewUserDraft {
        NewUserDraft {
            handle: handle.to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555-0123".to_string()),
            email: Some("u@example.com".to_string()),
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 80,
            ansi_colour: true,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::ByFiles,
            ratio_value: 3,
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1000),
        }
    }

    mod command_write_contract {
        use super::*;
        use crate::adapters::user_repository_contract as contract;
        use crate::domain::user::{MembershipPatch, PointerPatch, UserPatch};

        fn make(users: Vec<User>) -> SqliteUserRepository {
            let repo = SqliteUserRepository::in_memory().expect("open in-memory db");
            for user in users {
                repo.insert_seed(&user).expect("seed user");
            }
            repo
        }

        #[test]
        fn mismatch_bumps_additively() {
            contract::mismatch_bumps_additively(make);
        }

        #[test]
        fn mismatch_lock_is_one_way() {
            contract::mismatch_lock_is_one_way(make);
        }

        #[test]
        fn matched_clears_attempts() {
            contract::matched_clears_attempts(make);
        }

        #[test]
        fn matched_new_day_resets_counters() {
            contract::matched_new_day_resets_counters(make);
        }

        #[test]
        fn matched_same_day_bumps_today() {
            contract::matched_same_day_bumps_today(make);
        }

        #[test]
        fn matched_rejected_path_leaves_daily_counters() {
            contract::matched_rejected_path_leaves_daily_counters(make);
        }

        #[test]
        fn matched_does_not_unset_force_reset() {
            contract::matched_does_not_unset_force_reset(make);
        }

        #[test]
        fn password_change_replaces_credentials_and_clears_flag() {
            contract::password_change_replaces_credentials_and_clears_flag(make);
        }

        #[test]
        fn patch_counters_are_additive() {
            contract::patch_counters_are_additive(make);
        }

        #[test]
        fn patch_last_call_is_monotonic() {
            contract::patch_last_call_is_monotonic(make);
        }

        #[test]
        fn patch_pointer_rows_max_merge_and_keep_new_since() {
            contract::patch_pointer_rows_max_merge_and_keep_new_since(make);
        }

        #[test]
        fn patch_creates_missing_membership_with_pointer_rows() {
            contract::patch_creates_missing_membership_with_pointer_rows(make);
        }

        #[test]
        fn patch_preferences_are_last_writer_wins() {
            contract::patch_preferences_are_last_writer_wins(make);
        }

        #[test]
        fn interleaved_sessions_do_not_lose_updates() {
            contract::interleaved_sessions_do_not_lose_updates(make);
        }

        #[test]
        fn unknown_slot_is_user_not_found() {
            contract::unknown_slot_is_user_not_found(make);
        }

        #[test]
        fn apply_user_patch_rolls_back_wholesale_on_mid_patch_failure() {
            // A pointer row for a conference with no membership row
            // violates the composite FK. The users-table counter bump
            // in the same patch must roll back with it — a partial
            // (torn) application was exactly the defect of the old
            // bare-connection save().
            let repo = make(vec![contract::seeded_user(7, "alice")]);
            let patch = UserPatch {
                times_called_delta: 1,
                memberships: vec![MembershipPatch {
                    conference_number: 99,
                    create_if_missing: false, // no membership row -> FK failure
                    granted: None,
                    messages_posted_delta: 0,
                    scan_flags: None,
                    pointers: vec![PointerPatch {
                        msgbase_number: 1,
                        last_read: 1,
                        last_scanned: 1,
                        new_since: SystemTime::UNIX_EPOCH,
                    }],
                }],
                ..UserPatch::default()
            };
            repo.apply_user_patch(7, &patch)
                .expect_err("pointer row without membership must fail the FK check");
            let NameLookupResult::Found(user) = repo.find_by_handle("alice").expect("lookup")
            else {
                panic!("seeded user must exist");
            };
            assert_eq!(
                user.to_persisted().times_called,
                10,
                "counter bump must roll back with the failed pointer insert"
            );
        }
    }

    #[test]
    fn create_user_starts_at_one_in_a_fresh_database() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        let user = repo.create_user(draft_with("alice")).expect("create");
        assert_eq!(user.slot_number(), 1);
        assert!(matches!(
            repo.find_by_handle("alice"),
            Ok(NameLookupResult::Found(_))
        ));
    }

    #[test]
    fn find_by_handle_is_case_insensitive_on_stored_handle() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("Alice")).expect("create");
        assert!(matches!(
            repo.find_by_handle("alice"),
            Ok(NameLookupResult::Found(_))
        ));
        assert!(matches!(
            repo.find_by_handle("ALICE"),
            Ok(NameLookupResult::Found(_))
        ));
    }

    #[test]
    fn save_round_trips_a_full_user_record() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        let mut alice = repo.create_user(draft_with("alice")).expect("create");
        alice.bump_invalid_attempts();
        alice.bump_invalid_attempts();
        alice.bump_times_called();
        alice.bump_times_called_today();
        alice.add_time_used_today(Duration::from_secs(45));
        alice.bump_messages_posted();
        alice.set_censored(true);
        alice.set_force_password_reset(true);
        alice.set_expert_mode(true);
        alice.upsert_membership(ConferenceMembership::new(1, true));
        alice.upsert_read_pointers(
            ReadPointers::new(1, 3, 5, SystemTime::UNIX_EPOCH + Duration::from_secs(200))
                .expect("valid"),
            1,
        );
        repo.save(alice.clone()).expect("save");

        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(loaded) => {
                assert_eq!(loaded.handle(), alice.handle());
                assert_eq!(loaded.slot_number(), alice.slot_number());
                assert_eq!(loaded.invalid_attempts(), 2);
                assert_eq!(loaded.times_called(), 1);
                assert_eq!(loaded.times_called_today(), 1);
                assert_eq!(loaded.time_used_today(), Duration::from_secs(45));
                assert_eq!(loaded.messages_posted(), 1);
                assert!(loaded.is_censored());
                assert!(loaded.force_password_reset());
                assert!(
                    loaded.is_new_user(),
                    "register_new starts the account in the awaiting-validation tier; \
                     the bool must round-trip through SQLite"
                );
                assert!(
                    loaded.ansi_colour(),
                    "the registration record opted into ANSI; the bool must survive a round-trip"
                );
                assert!(
                    loaded.expert_mode(),
                    "the X toggle set expert mode; the bool must survive a round-trip"
                );
                assert_eq!(loaded.memberships().len(), 1);
                let pointer = loaded
                    .read_pointers_for(MessageBaseRef::new(1, 1))
                    .expect("pointer row");
                assert_eq!(pointer.last_read(), 3);
                assert_eq!(pointer.last_scanned(), 5);
            }
            NameLookupResult::NotFound => panic!("expected to find alice"),
        }
    }

    #[test]
    fn scan_flags_round_trip_through_sqlite() {
        // C5: the per-conference M/A/F/Z scan flags must survive the
        // on-logoff save (legacy "** AutoSaving File Flags **"). Every flag
        // is inverted away from its default (mail/file on, all/zoom off) so
        // a missing column surfaces as the wrong value on reload, not a
        // coincidental match.
        use crate::domain::conference::ScanFlag;
        let repo = SqliteUserRepository::in_memory().expect("open");
        let mut alice = repo.create_user(draft_with("alice")).expect("create");
        let mut membership = ConferenceMembership::new(1, true);
        membership.set_scan_flag(ScanFlag::MailScan, false);
        membership.set_scan_flag(ScanFlag::FileScan, false);
        membership.set_scan_flag(ScanFlag::MailScanAll, true);
        membership.set_scan_flag(ScanFlag::Zoom, true);
        alice.upsert_membership(membership);
        repo.save(alice.clone()).expect("save");

        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(loaded) => {
                let m = loaded
                    .memberships()
                    .iter()
                    .find(|m| m.conference_number() == 1)
                    .expect("membership row");
                assert!(!m.scan_flag(ScanFlag::MailScan), "mail_scan persisted off");
                assert!(!m.scan_flag(ScanFlag::FileScan), "file_scan persisted off");
                assert!(
                    m.scan_flag(ScanFlag::MailScanAll),
                    "mailscan_all persisted on"
                );
                assert!(m.scan_flag(ScanFlag::Zoom), "zoom_scan persisted on");
            }
            NameLookupResult::NotFound => panic!("expected to find alice"),
        }
    }

    #[test]
    fn save_unknown_user_returns_user_not_found() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        let alien =
            User::register_new(999, draft_with("unknown")).expect("valid registration record");
        let err = repo.save(alien).expect_err("save should fail");
        assert_eq!(
            err,
            UserRepositoryError::UserNotFound {
                handle: "unknown".to_string()
            }
        );
    }

    #[test]
    fn create_user_rejects_duplicate_handle() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("alice")).expect("create first");
        let err = repo
            .create_user(draft_with("alice"))
            .expect_err("duplicate should error");
        assert_eq!(
            err,
            UserCreationError::DuplicateUser {
                handle: "alice".to_string()
            }
        );
    }

    #[test]
    fn create_user_returns_one_above_max_used() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("alice")).expect("alice");
        repo.create_user(draft_with("bob")).expect("bob");
        let next = repo.create_user(draft_with("carol")).expect("carol");
        assert_eq!(next.slot_number(), 3);
    }

    #[test]
    fn find_sysop_returns_slot_one() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("sysop")).expect("sysop");
        match repo.find_sysop().expect("lookup sysop") {
            NameLookupResult::Found(user) => assert!(user.is_sysop()),
            NameLookupResult::NotFound => panic!("sysop should have been created"),
        }
    }

    #[test]
    fn find_sysop_returns_not_found_when_empty() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        assert!(matches!(repo.find_sysop(), Ok(NameLookupResult::NotFound)));
    }

    #[test]
    fn find_by_handle_returns_storage_error_when_row_cannot_decode() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("alice")).expect("create");
        {
            let conn = repo.conn.lock().expect("db mutex");
            conn.execute(
                "UPDATE users SET password_hash_kind = 'unknown' WHERE handle_folded = 'alice'",
                [],
            )
            .expect("corrupt row");
        }

        let err = repo
            .find_by_handle("alice")
            .expect_err("decode failure must not become not-found");
        assert!(
            matches!(
                err,
                UserRepositoryError::Storage {
                    context: "lookup",
                    ref message
                } if message.contains("unknown password_hash_kind")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn find_sysop_returns_storage_error_when_row_cannot_decode() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        repo.create_user(draft_with("sysop")).expect("create");
        {
            let conn = repo.conn.lock().expect("db mutex");
            conn.execute(
                "UPDATE users SET password_hash_kind = 'unknown' WHERE slot_number = 1",
                [],
            )
            .expect("corrupt row");
        }

        let err = repo
            .find_sysop()
            .expect_err("decode failure must not become no sysop");
        assert!(
            matches!(
                err,
                UserRepositoryError::Storage {
                    context: "lookup sysop",
                    ref message
                } if message.contains("unknown password_hash_kind")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn save_returns_storage_error_when_lookup_query_fails() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        let alice = repo.create_user(draft_with("alice")).expect("create");
        {
            let conn = repo.conn.lock().expect("db mutex");
            conn.execute_batch("DROP TABLE users").expect("drop users");
        }

        let err = repo
            .save(alice)
            .expect_err("storage failure must not become user-not-found");
        assert!(
            matches!(
                err,
                UserRepositoryError::Storage {
                    context: "save lookup",
                    ref message
                } if message.contains("no such table")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn create_user_returns_storage_error_when_repository_query_fails() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        {
            let conn = repo.conn.lock().expect("db mutex");
            conn.execute_batch("DROP TABLE users").expect("drop users");
        }

        let err = repo
            .create_user(draft_with("alice"))
            .expect_err("storage failure must not become a build error");
        assert!(
            matches!(
                err,
                UserCreationError::Storage {
                    context: "duplicate check",
                    ref message
                } if message.contains("no such table")
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn flags_bitmask_round_trips_every_known_flag() {
        // Each flag is exercised individually so an off-by-one in the
        // bit shift for any single variant is observable. A bulk
        // round-trip alone misses single-bit mutations on flags that
        // happen to be absent from the set.
        for flag in ALL_USER_FLAGS {
            let mut set = BTreeSet::new();
            set.insert(flag);
            let bits = flags_to_bitmask(&set);
            assert_ne!(
                bits, 0,
                "flag {flag:?} must occupy at least one bit (a >> shift would collapse it)"
            );
            assert_eq!(bitmask_to_flags(bits), set, "round trip for {flag:?}");
        }
        // And as a combined sanity check.
        let combined: BTreeSet<UserFlag> = ALL_USER_FLAGS.into_iter().collect();
        let combined_bits = flags_to_bitmask(&combined);
        assert_eq!(bitmask_to_flags(combined_bits), combined);
    }

    #[test]
    fn is_empty_returns_true_for_a_fresh_database_and_false_once_a_user_is_inserted() {
        let repo = SqliteUserRepository::in_memory().expect("open");
        assert!(repo.is_empty().expect("count"));
        repo.create_user(draft_with("alice")).expect("alice");
        assert!(!repo.is_empty().expect("count after insert"));
    }

    #[test]
    fn save_round_trips_account_locked_and_specific_timestamps() {
        // The round-trip test above asserts boolean / counter
        // fields. This test pins specific SystemTime values so that
        // an encoder/decoder that always returned 0 (or shifted the
        // epoch) is observable on read-back.
        let repo = SqliteUserRepository::in_memory().expect("open");
        let mut alice = repo.create_user(draft_with("alice")).expect("create");
        alice.lock_account();
        alice.record_last_call(SystemTime::UNIX_EPOCH + Duration::from_secs(123_456));
        repo.save(alice.clone()).expect("save");

        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(loaded) => {
                assert!(loaded.is_account_locked());
                assert_eq!(
                    loaded.last_call(),
                    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(123_456))
                );
                assert_eq!(
                    loaded.account_created(),
                    alice.account_created(),
                    "account_created must survive the SystemTime round-trip"
                );
            }
            NameLookupResult::NotFound => panic!("expected to find alice"),
        }
    }

    #[test]
    fn save_round_trips_last_joined_with_both_coordinates() {
        // The row-decoder's match arm on `(Some, Some)` is the only
        // path that produces a populated last_joined value. Without
        // a positive test, deleting that arm passes mutation testing.
        use crate::domain::conference::{Conference, MessageBase};

        let repo = SqliteUserRepository::in_memory().expect("open");
        let mut alice = repo.create_user(draft_with("alice")).expect("create");
        let conf = Conference::new(
            7,
            "Seven".to_string(),
            vec![MessageBase::new(7, 3, "third".to_string())],
        )
        .expect("conf");
        alice.upsert_membership(ConferenceMembership::new(7, true));
        alice.record_join(&conf, &conf.msgbases()[0]);
        repo.save(alice).expect("save");

        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(loaded) => {
                let joined = loaded.last_joined().expect("last_joined preserved");
                assert_eq!(joined.conference_number(), 7);
                assert_eq!(joined.msgbase_number(), 3);
            }
            NameLookupResult::NotFound => panic!("expected to find alice"),
        }
    }

    #[test]
    fn negative_system_time_round_trips_through_secs_helpers() {
        // SystemTime values predating the UNIX epoch are stored as
        // negative seconds; the round-trip must preserve sign.
        let before_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(42);
        let secs = system_time_to_secs(before_epoch);
        assert!(secs < 0, "pre-epoch timestamps encode as negative seconds");
        assert_eq!(secs_to_system_time(secs), before_epoch);

        let after_epoch = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        let secs2 = system_time_to_secs(after_epoch);
        assert!(
            secs2 > 0,
            "post-epoch timestamps encode as positive seconds"
        );
        assert_eq!(secs_to_system_time(secs2), after_epoch);
    }

    #[test]
    fn opens_file_backed_database_and_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("users.db");
        let repo = SqliteUserRepository::open(&db_path).expect("open file db");
        repo.create_user(draft_with("alice")).expect("create alice");
        drop(repo);
        assert!(db_path.exists());

        // Re-opening reads the existing row, proving the file is the
        // source of truth (not just an in-memory artefact).
        let repo2 = SqliteUserRepository::open(&db_path).expect("re-open");
        assert!(matches!(
            repo2.find_by_handle("alice"),
            Ok(NameLookupResult::Found(_))
        ));
    }
}
