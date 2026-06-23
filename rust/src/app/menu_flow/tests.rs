//! Dispatch-loop tests for the menu flow.
//!
//! Covers the logon-scan gate and the plain-`G` flagged-file logoff
//! confirm (slice D5/Ga). The confirm's reference is the genuine
//! internal `internalCommandG` -> `checkFlagged()` -> `yesNo(2)` path
//! (`amiexpress/express.e:25045`, `:12667`, `:2129`), captured live in
//! `comparison/transcripts/ae_tierd_g_confirm.txt`.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::app::menu_command::parse_menu_command;
use crate::app::services::AppServices;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::app::terminal::{
    KeyEvent, KeyRead, Terminal, TerminalEcho, TerminalFuture, TerminalRead,
};
use crate::app::wire_text::{GOODBYE_LINE, IDLE_TIMEOUT_LINE};
use crate::domain::conference::{Conference, ConferenceMembership, MessageBase};
use crate::domain::files::flagged::FlaggedKey;
use crate::domain::password::{PasswordHashKind, PasswordHasher};
use crate::domain::session::typed::MenuSession;
use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
use crate::domain::user::{RatioMode, User};

use super::{DispatchOutcome, MenuFlow};

/// Write-capturing terminal with a scripted key-read queue. The adapter
/// echoes NOTHING in hot-key mode (the caller owns every visible byte),
/// so `output` is the pure server-generated wire — the parity surface.
#[derive(Default)]
struct CaptureTerminal {
    output: Vec<u8>,
    keys: VecDeque<KeyRead>,
}

impl CaptureTerminal {
    fn with_keys(keys: Vec<KeyRead>) -> Self {
        Self {
            output: Vec::new(),
            keys: keys.into(),
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
        Box::pin(async { Ok(TerminalRead::Eof) })
    }

    fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        let key = self.keys.pop_front().unwrap_or(KeyRead::Eof);
        Box::pin(async move { Ok(key) })
    }
}

fn char_key(c: u8) -> KeyRead {
    KeyRead::Key(KeyEvent::Char(c))
}

fn enter_key() -> KeyRead {
    KeyRead::Key(KeyEvent::Enter)
}

fn conference(number: u32) -> Conference {
    Conference::new(
        number,
        format!("Conf {number}"),
        vec![MessageBase::new(number, 1, "main".to_string())],
    )
    .expect("valid conference")
}

fn test_user() -> User {
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

fn menu_session() -> MenuSession {
    let conferences = vec![conference(1)];
    let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
    session.prompt_for_name().expect("prompt");
    session
        .record_identified_user("alice", test_user())
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

/// A menu-phase session carrying one flagged file, so plain `G` reaches
/// the `checkFlagged()` confirm.
fn session_with_flagged_file() -> MenuSession {
    let mut session = menu_session();
    session
        .flagged_files_mut()
        .flag(FlaggedKey::new(1, 1, "MYDEMO.DMS"));
    session
}

/// Builds a menu-phase session with the `quick_logon` flag set.
fn quick_logon_menu_session() -> MenuSession {
    let user = User::new(
        2,
        "alice".to_string(),
        PasswordHashKind::Pbkdf210000,
        "hash".to_string(),
        Some("salt".to_string()),
        SystemTime::UNIX_EPOCH,
        100,
    )
    .expect("valid user");
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
    session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
    session.set_quick_logon(true);
    MenuSession::from_session(session)
}

fn test_services() -> AppServices {
    AppServices {
        user_repo: Arc::new(InMemoryUserRepository::default()),
        hasher: Arc::new(Pbkdf2PasswordHasher::new()),
        caller_log: Arc::new(InMemoryCallerLog::new()),
        screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
        conferences: Arc::new(Vec::new()),
        mail_stores: Arc::new(InMemoryMailStores::new()),
        file_repo: Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
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

/// Drives one menu command through the real `dispatch`, returning the
/// outcome. The command is parsed (not hand-built) so the test is
/// agnostic to the `MenuCommand` shape.
async fn dispatch_line(
    services: &AppServices,
    terminal: &mut CaptureTerminal,
    session: MenuSession,
    line: &str,
) -> DispatchOutcome {
    let mut flow = MenuFlow { terminal, services };
    flow.dispatch(session, parse_menu_command(line))
        .await
        .expect("dispatch")
}

/// The genuine `checkFlagged()` + `yesNo(2)` prompt, server bytes,
/// captured live (`ae_tierd_g_confirm.txt:146`): the `\b\n`->`\r\n`
/// message followed by yesNo's own ANSI `(y/N)? ` suffix.
const LEAVE_FLAGGED_CONFIRM: &[u8] =
    b"\r\nYou have flagged files still not downloaded.\r\nDo you leave without them? \x1b[32m(\x1b[33my\x1b[32m/\x1b[33mN\x1b[32m)\x1b[32m?\x1b[0m ";

#[tokio::test]
async fn plain_g_with_flagged_files_confirms_then_n_stays_at_menu() {
    // Live: ae_tierd_g_confirm.txt:138-156. Plain `G` with files flagged
    // prints the confirm, and a single `N` keypress echoes `No\r\n`,
    // emits the internal `\r\n` (mystat=0 path, express.e:25060), and
    // returns to the menu — it does NOT log the caller off.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(
        matches!(outcome, DispatchOutcome::Continue(_)),
        "answering N must keep the caller in the menu, not log off"
    );
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"No\r\n\r\n");
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn confirm_default_enter_answer_stays_at_menu() {
    // yesNo(2)'s CR default is `N` (express.e:2145): a bare Enter at
    // the confirm echoes `No` and stays, exactly like typing `N`.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![enter_key()]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(matches!(outcome, DispatchOutcome::Continue(_)));
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"No\r\n\r\n");
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn plain_g_with_flagged_files_y_leaves_and_logs_off() {
    // Live: ae_tierd_g_confirm.txt:168-177. A `Y` echoes `Yes\r\n` and
    // proceeds to logoff. (saveFlagged's `** AutoSaving File Flags **`
    // banner is slice D5, not yet emitted — only the `Goodbye!` tail.)
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'Y')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(
        matches!(outcome, DispatchOutcome::LogoffComplete(_)),
        "answering Y must log the caller off"
    );
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"Yes\r\n");
    expected.extend_from_slice(GOODBYE_LINE);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn plain_g_without_flagged_files_logs_off_without_confirming() {
    // checkFlagged() only prompts when `flagFilesList.count()` is
    // non-zero (express.e:12669); an empty flag set logs off straight
    // away, emitting no confirm and reading no key.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "G").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    assert_eq!(
        terminal.output,
        GOODBYE_LINE,
        "an empty flag set must skip the confirm entirely, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn g_y_forces_logoff_past_the_flagged_confirm() {
    // `G Y` sets `auto` (express.e:25049), bypassing checkFlagged()
    // even with files flagged — straight to logoff, no confirm.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    assert_eq!(
        terminal.output,
        GOODBYE_LINE,
        "G Y must force logoff with no confirm, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn confirm_ignores_unrecognised_keys_until_a_yes_or_no() {
    // yesNo loops on any key that is not y/n/CR (express.e:2140): a
    // stray `x` is swallowed and the prompt waits, then `Y` leaves.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'x'), char_key(b'Y')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    // The `x` produced no echo — only the prompt, the `Yes`, the tail.
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"Yes\r\n");
    expected.extend_from_slice(GOODBYE_LINE);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn confirm_disconnect_mid_prompt_logs_off_without_a_goodbye() {
    // A dropped carrier at the confirm (readChar < 0, express.e:2142)
    // ends the session — the confirm was written, but no echo and no
    // Goodbye follow.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(Vec::new()); // read_key -> Eof
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    assert_eq!(
        terminal.output,
        LEAVE_FLAGGED_CONFIRM,
        "a mid-confirm disconnect writes the prompt only, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn confirm_idle_timeout_mid_prompt_logs_off_with_the_timeout_line() {
    // An idle timeout at the confirm logs off with the dedicated
    // timeout notice, distinct from a clean carrier loss.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![KeyRead::IdleTimedOut]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(matches!(outcome, DispatchOutcome::LogoffComplete(_)));
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(IDLE_TIMEOUT_LINE);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn quick_logon_skips_the_logon_conference_scan() {
    // Spec `messaging.allium:ScanConferencesOnLogon` gates on
    // `not quick_logon`; a quick logon must skip the scan entirely —
    // not even the `Scanning conferences for mail...` header is
    // written. (Pins `MenuSession::quick_logon`: a mutant forcing it
    // to `false` would let the scan run and emit the header.)
    let services = test_services();
    let mut terminal = CaptureTerminal::default();
    let mut menu = quick_logon_menu_session();
    {
        let mut flow = MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.run_logon_conference_scan(&mut menu)
            .await
            .expect("scan");
    }
    assert!(
        terminal.output.is_empty(),
        "a quick logon must skip the logon conference scan, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}
