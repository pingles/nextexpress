//! Shared `#[cfg(test)]` fixtures for the `menu_flow` test modules.
//!
//! The default [`AppServices`] fixture was copy-pasted verbatim across a
//! dozen test modules; this is the single source of truth. Tests that
//! need a non-default port build on top of it:
//! `let mut s = test_services(); s.field = ...; s`.
//!
//! The write-capturing [`CaptureTerminal`] and the menu-phase
//! session/user fixtures live here too (slice D9 consolidation): the
//! `file_list` and `menu_flow` test modules carried near-duplicate
//! copies of each.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::adapters::system_clock::SystemClock;
use crate::app::seed;
use crate::app::services::AppServices;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::app::terminal::{
    KeyEvent, KeyRead, Terminal, TerminalEcho, TerminalFuture, TerminalRead,
};
use crate::domain::conference::{Conference, ConferenceMembership, MessageBase};
use crate::domain::password::{PasswordHashKind, PasswordHasher};
use crate::domain::session::typed::MenuSession;
use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
use crate::domain::user::{RatioMode, User};

/// The default [`AppServices`] test fixture: in-memory ports, an empty
/// conference catalogue, empty file/mail/flag stores, new users allowed,
/// and ratio accounting disabled. Override individual fields on the
/// returned value for tests that need a non-default port.
pub(crate) fn test_services() -> AppServices {
    AppServices {
        user_repo: Arc::new(InMemoryUserRepository::default()),
        hasher: Arc::new(Pbkdf2PasswordHasher::new()),
        caller_log: Arc::new(InMemoryCallerLog::new()),
        screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
        conferences: Arc::new(Vec::new()),
        mail_stores: Arc::new(InMemoryMailStores::new()),
        file_repo: Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
        flagged_store: Arc::new(InMemoryFlaggedStore::new()),
        clock: Arc::new(SystemClock),
        session_policy: SessionPolicy::default(),
        default_ratio: DefaultRatio {
            mode: RatioMode::Disabled,
            value: 0,
        },
        new_user_gate: Arc::new(NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        }),
        bbs_name: Arc::from("Test BBS"),
    }
}

/// Write-capturing terminal with scripted line- and key-read queues.
/// Like the real adapter in hot-key parity, it echoes NOTHING, so
/// `output` is the pure server-generated wire — the parity surface.
/// Drained queues read as `Eof` (carrier loss).
pub(crate) struct CaptureTerminal {
    /// Every byte the flow under test wrote.
    pub(crate) output: Vec<u8>,
    /// Scripted `read_line` (`lineInput`) results.
    pub(crate) lines: VecDeque<TerminalRead>,
    /// Scripted `read_key` (hot-key) results.
    pub(crate) keys: VecDeque<KeyRead>,
    /// Live ANSI colour mode, surfaced through `ansi_colour()` so the
    /// repaint gate (slice D2f) can be exercised both ways.
    pub(crate) ansi: bool,
}

impl Default for CaptureTerminal {
    fn default() -> Self {
        Self {
            output: Vec::new(),
            lines: VecDeque::new(),
            keys: VecDeque::new(),
            ansi: true,
        }
    }
}

impl CaptureTerminal {
    /// Scripts the `read_line` queue only.
    pub(crate) fn with_lines(lines: Vec<TerminalRead>) -> Self {
        Self {
            lines: lines.into(),
            ..Self::default()
        }
    }

    /// Scripts the `read_key` queue only.
    pub(crate) fn with_keys(keys: Vec<KeyRead>) -> Self {
        Self {
            keys: keys.into(),
            ..Self::default()
        }
    }

    /// Scripts both queues, for flows that mix line prompts with the
    /// hot-key pager.
    pub(crate) fn with_lines_and_keys(lines: Vec<TerminalRead>, keys: Vec<KeyRead>) -> Self {
        Self {
            lines: lines.into(),
            keys: keys.into(),
            ..Self::default()
        }
    }
}

impl Terminal for CaptureTerminal {
    type Error = Infallible;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move {
            self.output.extend_from_slice(bytes);
            Ok(())
        })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn read_line(
        &mut self,
        _echo: TerminalEcho,
        _timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        let read = self.lines.pop_front().unwrap_or(TerminalRead::Eof);
        Box::pin(async move { Ok(read) })
    }

    fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        let key = self.keys.pop_front().unwrap_or(KeyRead::Eof);
        Box::pin(async move { Ok(key) })
    }

    fn ansi_colour(&self) -> bool {
        self.ansi
    }
}

/// One scripted printable keypress for the hot-key pager (D2b).
pub(crate) fn key(c: u8) -> KeyRead {
    KeyRead::Key(KeyEvent::Char(c))
}

/// A terminal scripted with pager keys only — no line reads.
pub(crate) fn keyed_terminal(keys: Vec<KeyRead>) -> CaptureTerminal {
    CaptureTerminal::with_keys(keys)
}

/// A key-scripted terminal with ANSI colour off — for the repaint
/// gate (slice D2f): the cursor CSI must be suppressed.
pub(crate) fn keyed_terminal_no_ansi(keys: Vec<KeyRead>) -> CaptureTerminal {
    let mut terminal = CaptureTerminal::with_keys(keys);
    terminal.ansi = false;
    terminal
}

/// A one-msgbase conference fixture.
pub(crate) fn conference(number: u32) -> Conference {
    Conference::new(
        number,
        format!("Conf {number}"),
        vec![MessageBase::new(number, 1, "main".to_string())],
    )
    .expect("valid conference")
}

/// The default menu-phase test user: slot 2 `alice`, access 255, a
/// membership in conference 1, no prior call.
pub(crate) fn test_user() -> User {
    let hasher = Pbkdf2PasswordHasher::new();
    let computed = hasher
        .compute_password_hash("pw", PasswordHashKind::Pbkdf210000)
        .expect("hash");
    let mut user = User::new(
        2,
        "alice".to_string(),
        PasswordHashKind::Pbkdf210000,
        computed.hash,
        computed.salt,
        SystemTime::UNIX_EPOCH,
        255,
    )
    .expect("valid user");
    user.upsert_membership(ConferenceMembership::new(1, true));
    user
}

/// A menu-phase session for [`test_user`], joined to conference 1.
pub(crate) fn menu_session() -> MenuSession {
    menu_session_with_user(test_user())
}

/// A menu-phase session for `user`, joined to conference 1 — for tests
/// that need a tuned user (e.g. a recorded `last_call`).
pub(crate) fn menu_session_with_user(user: User) -> MenuSession {
    let conferences = vec![conference(1)];
    let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
    session.prompt_for_name().expect("prompt");
    session
        .record_identified_user("alice", user)
        .expect("identify");
    apply_password_match(
        &mut session,
        SessionPolicy::default(),
        SystemTime::UNIX_EPOCH,
    )
    .expect("password match");
    session
        .auto_rejoin_conference(&conferences, SystemTime::UNIX_EPOCH)
        .expect("rejoin");
    session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
    MenuSession::from_session(session)
}

/// [`test_services`] over `file_repo`, with conference 1 loaded.
pub(crate) fn services_with(file_repo: InMemoryFileRepository) -> AppServices {
    let mut services = test_services();
    services.conferences = Arc::new(vec![conference(1)]);
    services.file_repo = Arc::new(file_repo);
    services
}

/// Services whose file catalogue is the seeded demo corpus in
/// conference 1 (area 1: 27 files, area 2: 3 files).
pub(crate) fn services_with_demo_catalogue() -> AppServices {
    let conferences = vec![conference(1)];
    let (areas, files) = seed::demo_file_catalogue(&conferences);
    services_with(InMemoryFileRepository::new(areas, files))
}

/// The two-reset tail every listing-shaped exit emits
/// (`ae_tierd_aquascan3.txt:163`).
pub(crate) const EXIT_TAIL: &[u8] = b"\x1b[0m\r\n\x1b[0m\r\n";

/// Joins `lines` with `\r\n` terminators — the byte stream a sequence
/// of emitted listing lines produces.
pub(crate) fn joined(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in lines {
        bytes.extend_from_slice(line);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes
}

/// `\r` + 69 spaces + `\r` — the captured `More?`/ns-confirm
/// overprint clear (counted programmatically from the transcripts).
pub(crate) fn more_clear() -> Vec<u8> {
    let mut bytes = vec![b'\r'];
    bytes.extend(std::iter::repeat_n(b' ', 69));
    bytes.push(b'\r');
    bytes
}

/// `\r` + 79 spaces + `\r` — the wider overprint after a flag entry
/// (counted from `ae_tierd_aquascan3.txt` S4).
pub(crate) fn flag_clear() -> Vec<u8> {
    let mut bytes = vec![b'\r'];
    bytes.extend(std::iter::repeat_n(b' ', 79));
    bytes.push(b'\r');
    bytes
}
