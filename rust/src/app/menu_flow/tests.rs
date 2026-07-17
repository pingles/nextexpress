//! Dispatch-loop tests for the menu flow.
//!
//! Covers the logon-scan gate and the plain-`G` flagged-file logoff
//! confirm (slice D5/Ga). The confirm's reference is the genuine
//! internal `internalCommandG` -> `checkFlagged()` -> `yesNo(2)` path
//! (`amiexpress/express.e:25045`, `:12667`, `:2129`), captured live in
//! `comparison/transcripts/ae_tierd_g_confirm.txt`.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime};

use crate::app::menu_command::parse_menu_command;
use crate::app::menu_flow::test_support::{menu_session, test_services, CaptureTerminal};
use crate::app::services::AppServices;
use crate::app::terminal::{KeyEvent, KeyRead, TerminalRead};
use crate::domain::files::flagged::FlaggedFiles;
use crate::domain::files::flagged::FlaggedKey;
use crate::domain::files::flagged_store::{FlaggedStore, FlaggedStoreError};
use crate::domain::password::PasswordHashKind;
use crate::domain::session::typed::MenuSession;
use crate::domain::session::{apply_password_match, CallId, LogonChannel, Session, SessionPolicy};
use crate::domain::user::User;

use super::{
    DispatchOutcome, MenuExit, MenuFlow, MenuFlowError, AUTOSAVING_FILE_FLAGS, CLEAR_PROMPT,
    FLAGGED_FILES_EXIST, FLAG_PROMPT,
};

#[derive(Default)]
struct SpyFlaggedStore {
    /// (slot, sorted names) recorded on each save.
    saved: StdMutex<Vec<(u32, Vec<String>)>>,
    /// Pre-seeded sets returned by `load`, keyed by slot.
    seeded: StdMutex<std::collections::HashMap<u32, FlaggedFiles>>,
    /// When true, `save`/`load` return an error.
    fail: bool,
}

impl FlaggedStore for SpyFlaggedStore {
    fn load(&self, slot: u32) -> Result<FlaggedFiles, FlaggedStoreError> {
        if self.fail {
            return Err(FlaggedStoreError::Backend {
                source: "boom".into(),
            });
        }
        Ok(self
            .seeded
            .lock()
            .unwrap()
            .get(&slot)
            .cloned()
            .unwrap_or_default())
    }

    fn save(&self, slot: u32, flags: &FlaggedFiles) -> Result<(), FlaggedStoreError> {
        if self.fail {
            return Err(FlaggedStoreError::Backend {
                source: "boom".into(),
            });
        }
        let names: Vec<String> = flags.names().map(str::to_owned).collect();
        self.saved.lock().unwrap().push((slot, names));
        Ok(())
    }
}

fn services_with_flagged_store(store: Arc<dyn FlaggedStore + Send + Sync>) -> AppServices {
    let mut services = test_services();
    services.flagged_store = store;
    services
}

fn char_key(c: u8) -> KeyRead {
    KeyRead::Key(KeyEvent::Char(c))
}

fn enter_key() -> KeyRead {
    KeyRead::Key(KeyEvent::Enter)
}

/// A scripted `read_line` result carrying `text` (the legacy `lineInput`
/// returning a typed line). An empty `text` is the `<CR>`/none answer.
fn line(text: &str) -> TerminalRead {
    TerminalRead::Line(text.to_string())
}

/// A menu-phase session carrying one flagged file, so plain `G` reaches
/// the `checkFlagged()` confirm.
fn session_with_flagged_file() -> MenuSession {
    let mut session = menu_session();
    session
        .flagged_files_mut()
        .flag(FlaggedKey::new(1, "MYDEMO.DMS"));
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
        CallId::new(1),
    )
    .expect("password match");
    session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
    session.set_quick_logon(true);
    MenuSession::from_session(session)
}

enum TestDispatchOutcome {
    Continue(Box<MenuSession>),
    UserRequestedLogoff,
    Exit(MenuExit),
}

/// Drives one menu command through the real borrowed-session `dispatch`.
/// The command is parsed (not hand-built) so the test is agnostic to the
/// `MenuCommand` shape.
async fn dispatch_line(
    services: &AppServices,
    terminal: &mut CaptureTerminal,
    mut session: MenuSession,
    line: &str,
) -> TestDispatchOutcome {
    let mut flow = MenuFlow { terminal, services };
    match flow.dispatch(&mut session, parse_menu_command(line)).await {
        Ok(DispatchOutcome::Continue) => TestDispatchOutcome::Continue(Box::new(session)),
        Ok(DispatchOutcome::UserRequestedLogoff) => TestDispatchOutcome::UserRequestedLogoff,
        Err(MenuFlowError::Exit(exit)) => TestDispatchOutcome::Exit(exit),
        Err(MenuFlowError::Terminal(error)) => match error {},
    }
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
        matches!(outcome, TestDispatchOutcome::Continue(_)),
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

    assert!(matches!(outcome, TestDispatchOutcome::Continue(_)));
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"No\r\n\r\n");
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn plain_g_with_flagged_files_y_leaves_and_logs_off() {
    // Live: ae_tierd_g_confirm.txt:168-177. A `Y` echoes `Yes\r\n`, then
    // saveFlagged prints the `** AutoSaving File Flags **` banner + BEL
    // (express.e:25064 -> :2803) before the `Goodbye!` tail (slice D5).
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'Y')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(
        matches!(outcome, TestDispatchOutcome::UserRequestedLogoff),
        "answering Y must log the caller off"
    );
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"Yes\r\n");
    expected.extend_from_slice(AUTOSAVING_FILE_FLAGS);
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
    // away, emitting no confirm and reading no key. saveFlagged still
    // runs unconditionally on the logoff path (express.e:25064 ->
    // :2803), so the autosave banner appears even with nothing flagged
    // — live-confirmed by `comparison/transcripts/ae_tierd_g_empty.txt`
    // (`G\r\n\r\n** AutoSaving File Flags **\r\n\x07\r\n...`).
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "G").await;

    assert!(matches!(outcome, TestDispatchOutcome::UserRequestedLogoff));
    let expected = AUTOSAVING_FILE_FLAGS.to_vec();
    assert_eq!(
        terminal.output,
        expected,
        "an empty flag set skips the confirm but still autosave-banners, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn g_y_forces_logoff_past_the_flagged_confirm() {
    // `G Y` sets `auto` (express.e:25049), bypassing checkFlagged()'s
    // confirm even with files flagged — but saveFlagged still runs
    // (express.e:25064), so a non-empty flag set still prints the
    // autosave banner before goodbye. No confirm prompt, no key read.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;

    assert!(matches!(outcome, TestDispatchOutcome::UserRequestedLogoff));
    let expected = AUTOSAVING_FILE_FLAGS.to_vec();
    assert_eq!(
        terminal.output,
        expected,
        "G Y must force logoff (no confirm) but still autosave-banner with flags, got {:?}",
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

    assert!(matches!(outcome, TestDispatchOutcome::UserRequestedLogoff));
    // The `x` produced no echo — only the prompt, the `Yes`, the
    // autosave banner (flag set non-empty), then the goodbye tail.
    let mut expected = LEAVE_FLAGGED_CONFIRM.to_vec();
    expected.extend_from_slice(b"Yes\r\n");
    expected.extend_from_slice(AUTOSAVING_FILE_FLAGS);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn fs_denies_unconditionally_with_the_higher_access_line() {
    // Slice D8 (`FS`, FAITHFUL DENY): internalCommandFS gates on
    // ACS_CONFERENCE_ACCOUNTING (`amiexpress/express.e:24872`) and on
    // the shipped board no account holds the right, so every `FS`
    // returns RESULT_NOT_ALLOWED and the dispatcher tail (`:28400`)
    // prints higherAccess() (`:3038`). Live:
    // `comparison/transcripts/ae_tierd_fs.txt` (all probes, both
    // conferences, sysop sec 255 included). NextExpress writes the deny
    // unconditionally — no gate, no granted branch (design §7) — and
    // the session continues at the menu.
    let services = test_services();
    let mut terminal = CaptureTerminal::default();
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "FS").await;

    assert!(
        matches!(outcome, TestDispatchOutcome::Continue(_)),
        "the deny must keep the caller at the menu, not log off"
    );
    // Restated single-line literal, independent of HIGHER_ACCESS_LINE.
    assert_eq!(
        terminal.output,
        b"\r\nCommand requires higher access.\r\n".to_vec(),
        "FS must write exactly the higherAccess() line and nothing else, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_with_no_flags_lists_no_file_flags() {
    // Slice D6a/D6b. `A` -> alterFlags -> showFlags (express.e:12486): an
    // empty set prints `No file flags`, framed by alterFlags's leading
    // `\b\n`, then the flag prompt. Live: ae_tierd_alterflags.txt
    // `* -> cleared, empty listing`. Here the prompt's lineInput hits
    // EOF (carrier loss, stat<0), so alterFlags returns with no trailing
    // blank line and the lifecycle exit propagates immediately.
    let services = test_services();
    let mut terminal = CaptureTerminal::default(); // read_line -> Eof
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "A").await;

    assert!(matches!(
        outcome,
        TestDispatchOutcome::Exit(MenuExit::CarrierLost)
    ));
    let mut expected = b"\r\nNo file flags\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_lists_flagged_names_uppercased_and_space_joined() {
    // Slice D6a/D6b. showFlaggedFiles(-1) (express.e:2830) space-joins
    // the upper-cased flagged names, then the flag prompt follows. Live:
    // ae_tierd_alterflags.txt `A again -> clean ... listing`. Names are
    // upper-cased on flagging and ordered by the catalogue key; the
    // prompt's lineInput hits EOF here (no trailing line).
    let services = test_services();
    let mut session = menu_session();
    session
        .flagged_files_mut()
        .flag(FlaggedKey::new(1, "termv48.lha"));
    session
        .flagged_files_mut()
        .flag(FlaggedKey::new(1, "mydemo.dms"));
    let mut terminal = CaptureTerminal::default();
    let outcome = dispatch_line(&services, &mut terminal, session, "A").await;

    assert!(matches!(
        outcome,
        TestDispatchOutcome::Exit(MenuExit::CarrierLost)
    ));
    let mut expected = b"\r\nMYDEMO.DMS TERMV48.LHA\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_flag_prompt_then_enter_returns_to_menu() {
    // Slice D6b. `A` -> alterFlags (express.e:12648): the leading `\b\n`,
    // showFlags's `No file flags\r\n`, then the `flagFiles` main prompt
    // (express.e:12601). A bare `<CR>` (=none) falls through to
    // RESULT_SUCCESS, ending the REPEAT loop, so alterFlags emits its
    // trailing `\b\n` and returns to the menu. Live:
    // ae_tierd_alterflags.txt `Enter (=none) -> back to menu`.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("")]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "A").await;

    assert!(matches!(outcome, TestDispatchOutcome::Continue(_)));
    let mut expected = b"\r\nNo file flags\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT);
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_flagging_a_typed_name_exits_to_the_menu() {
    // Slice D6b. A filename typed at the flag prompt is added via
    // addFlagToList (express.e:12638); a *new* file returns
    // RESULT_FAILURE (stat=2 -> -1), so alterFlags returns at once with
    // NO trailing blank line. Live: ae_tierd_alterflags.txt
    // `flag 'mydemo.dms' -> ... Menu`. The name is upper-cased.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("mydemo.dms")]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "A").await;

    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("flagging a name returns to the menu");
    };
    assert_eq!(
        session.flagged_files().names().collect::<Vec<_>>(),
        vec!["MYDEMO.DMS"],
        "the typed name is flagged, upper-cased"
    );
    let mut expected = b"\r\nNo file flags\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT);
    assert_eq!(
        terminal.output,
        expected,
        "a new flag exits with no trailing blank line, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_single_char_token_is_a_no_op_that_ends_the_loop() {
    // Slice D6b. addFlagToList's `StrLen(fileName)>1` gate
    // (express.e:12532): a one-character token is not flagged and returns
    // 0 (RESULT_SUCCESS), so the loop ends with a trailing blank line —
    // unlike a real filename, which exits with none.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("x")]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "A").await;

    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("a no-op token stays in the menu");
    };
    assert!(
        session.flagged_files().is_empty(),
        "a one-character token must not be flagged"
    );
    let mut expected = b"\r\nNo file flags\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT);
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_clear_all_empties_the_set_and_reprompts() {
    // Slice D6b. Bare `C` opens the clear sub-prompt (express.e:12614);
    // `*` runs clearFlagItems (:12622), emits the post-input `\b\n`, and
    // returns 1, so the REPEAT loop re-shows the now-empty listing. A
    // final `<CR>` ends it. Live: ae_tierd_alterflags.txt
    // `C -> Clear sub-prompt` / `* -> cleared, empty listing`.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("C"), line("*"), line("")]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "A").await;

    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("clearing stays in the menu");
    };
    assert!(
        session.flagged_files().is_empty(),
        "`C` -> `*` clears every flag"
    );
    let mut expected = b"\r\nMYDEMO.DMS\r\n".to_vec(); // leading + listing
    expected.extend_from_slice(FLAG_PROMPT); // main prompt
    expected.extend_from_slice(CLEAR_PROMPT); // bare C -> clear sub-prompt
    expected.extend_from_slice(b"\r\n"); // post-clear `\b\n`
    expected.extend_from_slice(b"No file flags\r\n"); // reloop listing (empty)
    expected.extend_from_slice(FLAG_PROMPT); // main prompt again
    expected.extend_from_slice(b"\r\n"); // trailing (CR=none)
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_inline_clear_star_skips_the_subprompt() {
    // Slice D6b. `C *` on one line (express.e:12610) strips the `C `
    // prefix and clears directly — no clear sub-prompt, no post-input
    // blank line — then returns 1 and re-shows the empty listing. The
    // two-step `C` then `*` is the capture-pinned form
    // (ae_tierd_alterflags.txt); this inline variant is source-derived.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("C *"), line("")]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "A").await;

    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("clearing stays in the menu");
    };
    assert!(
        session.flagged_files().is_empty(),
        "`C *` clears every flag"
    );
    let mut expected = b"\r\nMYDEMO.DMS\r\n".to_vec();
    expected.extend_from_slice(FLAG_PROMPT); // main prompt
    expected.extend_from_slice(b"No file flags\r\n"); // reloop — NO sub-prompt, NO post-`\b\n`
    expected.extend_from_slice(FLAG_PROMPT);
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        terminal.output,
        expected,
        "inline `C *` skips the sub-prompt, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn a_clear_by_name_is_deferred_and_leaves_the_set_intact() {
    // Slice D6b deferral. At the clear sub-prompt a name (not `*`) maps
    // to removeFlagFromList (express.e:12622), which NextExpress does not
    // yet implement: the flag set is left intact and the loop re-shows
    // it. Pins the `*`-only clear guard so a mutant clearing on any token
    // is caught.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_lines(vec![line("C"), line("mydemo.dms"), line("")]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "A").await;

    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("stays in the menu");
    };
    assert_eq!(
        session.flagged_files().names().collect::<Vec<_>>(),
        vec!["MYDEMO.DMS"],
        "clear-by-name is deferred — the flag survives"
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

    assert!(matches!(
        outcome,
        TestDispatchOutcome::Exit(MenuExit::CarrierLost)
    ));
    assert_eq!(
        terminal.output,
        LEAVE_FLAGGED_CONFIRM,
        "a mid-confirm disconnect writes the prompt only, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn confirm_idle_timeout_mid_prompt_propagates_to_the_driver() {
    // The menu reports the idle timeout without writing a command-specific
    // line. The driver first applies the lifecycle transition, then owns the
    // standard timeout notice and finalisation.
    let services = test_services();
    let mut terminal = CaptureTerminal::with_keys(vec![KeyRead::IdleTimedOut]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G").await;

    assert!(matches!(
        outcome,
        TestDispatchOutcome::Exit(MenuExit::IdleTimedOut)
    ));
    let expected = LEAVE_FLAGGED_CONFIRM.to_vec();
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

#[test]
fn version_banner_carries_lineage_lines_verbatim() {
    // Pin the lineage block so a future edit can't quietly drift
    // the wording. Each line is checked individually so a swap or
    // reorder fails the test.
    let banner = std::str::from_utf8(super::VERSION_BANNER).expect("utf8 banner");
    assert!(
        banner.contains("Based on Versions:\r\n"),
        "missing lineage label: {banner:?}",
    );
    assert!(
        banner.contains("AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n"),
        "missing AmiExpress copyright line: {banner:?}",
    );
    assert!(
        banner.contains("  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n"),
        "missing Thomas author line: {banner:?}",
    );
    assert!(
        banner.contains("  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n"),
        "missing Hodge author line: {banner:?}",
    );
}

#[test]
fn version_banner_carries_nextexpress_version_and_sha() {
    // Slice A2: the leading line pins the running Rust port to
    // its `Cargo.toml` version + `build.rs` SHA so the operator
    // can correlate a running session with a specific build.
    let banner = std::str::from_utf8(super::VERSION_BANNER).expect("utf8 banner");
    let version = env!("CARGO_PKG_VERSION");
    let sha = env!("NEXTEXPRESS_GIT_SHA");
    let needle = format!("NextExpress {version} ({sha}) Copyright \u{00A9}2026 Paul Ingles\r\n");
    assert!(
        banner.contains(&needle),
        "expected `{needle}` in banner: {banner:?}",
    );
}

#[test]
fn version_banner_starts_with_crlf_and_omits_registration_key_line() {
    // Slice A2 (Out of Scope): the legacy `Registered to <key>.`
    // line (`amiexpress/express.e:25696`) is deliberately elided.
    let banner = std::str::from_utf8(super::VERSION_BANNER).expect("utf8 banner");
    assert!(
        banner.starts_with("\r\n"),
        "banner missing CRLF prefix: {banner:?}"
    );
    assert!(
        !banner.contains("Registered to"),
        "banner must elide the legacy `Registered to` line: {banner:?}",
    );
}

#[tokio::test]
async fn flag_prompt_stamps_the_idle_clock() {
    // Item 10's fraying-stamp finding: the `A` loop's FLAG_PROMPT read
    // skipped `record_input`, so a caller actively typing at the flag
    // prompt still looked idle to the menu loop's timeout accounting.
    // Every accepted prompt line must stamp the idle clock — the merged
    // reader owns the stamp so a new prompt cannot forget it.
    let mut services = test_services();
    let stamp = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    services.clock = Arc::new(crate::adapters::system_clock::ManualClock::set_to(stamp));
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line(String::new())]);
    let outcome = dispatch_line(&services, &mut terminal, menu_session(), "A").await;
    let TestDispatchOutcome::Continue(session) = outcome else {
        panic!("a blank flag-prompt line returns to the menu");
    };
    assert_eq!(
        (*session).into_inner().last_input_at(),
        stamp,
        "the accepted flag-prompt line stamps the idle clock"
    );
}

#[tokio::test]
async fn t_command_renders_the_clock_ports_instant_exactly() {
    // The `T` handler must resolve "now" through `services.clock`, not
    // the ambient `SystemTime::now()` — the seam that lets tests (and
    // the `N` date scan's smoke) pin exact time values. 1970-01-02
    // 03:04:05 UTC, matching the render_time_line pin below.
    let mut services = test_services();
    services.clock = Arc::new(crate::adapters::system_clock::ManualClock::set_to(
        SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 + 3 * 3600 + 4 * 60 + 5),
    ));
    let mut terminal = CaptureTerminal::default();
    dispatch_line(&services, &mut terminal, menu_session(), "T").await;
    assert_eq!(
        terminal.output,
        b"\r\nIt is 01-02-70 03:04:05\r\n".to_vec(),
        "T renders the port's instant, not the wall clock"
    );
}

#[test]
fn render_time_line_emits_legacy_it_is_prefix_and_us_format() {
    // Pin the legacy `It is <MM-DD-YY> <HH:MM:SS>` wire format
    // (`amiexpress/express.e:25636-25640`, FORMAT_USA). 1970-01-02
    // 03:04:05 UTC is the chosen fixed instant so all fields are
    // distinct two-digit numbers — any swap of fields shows up
    // immediately in the assertion.
    use std::time::{Duration, UNIX_EPOCH};
    let at = UNIX_EPOCH + Duration::from_secs(86_400 + 3 * 3600 + 4 * 60 + 5);
    assert_eq!(
        super::render_time_line(at),
        b"\r\nIt is 01-02-70 03:04:05\r\n"
    );
}

#[test]
fn render_time_line_zero_pads_single_digit_fields() {
    // FORMAT_USA pads every numeric field to two digits; a leading
    // zero is required for `09` and the like.
    use std::time::{Duration, UNIX_EPOCH};
    // 1970-01-01 00:00:00 UTC — every field is `00`.
    let at = UNIX_EPOCH + Duration::from_secs(0);
    assert_eq!(
        super::render_time_line(at),
        b"\r\nIt is 01-01-70 00:00:00\r\n"
    );
}

#[test]
fn render_time_line_uses_two_digit_year_wrap_after_2000() {
    // FORMAT_USA on AmigaOS produces a two-digit year; 2001 must
    // render as `01`, not `2001`. The Unix billennium (1e9 seconds
    // past the epoch) is 2001-09-09 01:46:40 UTC — a widely-known
    // reference instant.
    use std::time::{Duration, UNIX_EPOCH};
    let at = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    assert_eq!(
        super::render_time_line(at),
        b"\r\nIt is 09-09-01 01:46:40\r\n"
    );
}

#[tokio::test]
async fn logoff_saves_the_flag_set_for_the_user_slot() {
    // Slice D5-persist: saveFlagged (express.e:2806) writes the session
    // set to the durable store on the `G Y` logoff path, after the
    // autosave banner. The sysop fixture is slot 2 (test_user()).
    let spy = Arc::new(SpyFlaggedStore::default());
    let services = services_with_flagged_store(spy.clone());
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;

    assert!(matches!(outcome, TestDispatchOutcome::UserRequestedLogoff));
    let saved = spy.saved.lock().unwrap();
    assert_eq!(saved.len(), 1, "save called exactly once on logoff");
    assert_eq!(
        saved[0],
        (2, vec!["MYDEMO.DMS".to_string()]),
        "saved (slot, names) for the logged-on user"
    );
}

#[tokio::test]
async fn logoff_proceeds_when_the_flag_save_fails() {
    let spy = Arc::new(SpyFlaggedStore {
        fail: true,
        ..SpyFlaggedStore::default()
    });
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::with_keys(vec![char_key(b'N')]);
    let outcome = dispatch_line(&services, &mut terminal, session_with_flagged_file(), "G Y").await;
    assert!(
        matches!(outcome, TestDispatchOutcome::UserRequestedLogoff),
        "a save failure must not block logoff"
    );
}

#[tokio::test]
async fn logon_restores_flags_and_announces_when_non_empty() {
    // Slice D5-persist: loadFlagged (express.e:2757) restores the set on
    // logon; a non-empty restore emits the banner (express.e:2791-2794).
    let spy = Arc::new(SpyFlaggedStore::default());
    {
        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(1, "ansipack.lha"));
        spy.seeded.lock().unwrap().insert(2, flags); // slot 2 = test_user
    }
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.restore_flags_and_announce(&mut session)
            .await
            .expect("restore");
    }
    assert!(
        session
            .flagged_files()
            .contains(&FlaggedKey::new(1, "ANSIPACK.LHA")),
        "the saved flag is restored into the session"
    );
    assert_eq!(
        terminal.output, FLAGGED_FILES_EXIST,
        "a non-empty restore emits exactly the banner"
    );
}

#[tokio::test]
async fn logon_with_no_saved_flags_is_silent() {
    let spy = Arc::new(SpyFlaggedStore::default()); // nothing seeded
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.restore_flags_and_announce(&mut session)
            .await
            .expect("restore");
    }
    assert!(session.flagged_files().is_empty());
    assert!(
        terminal.output.is_empty(),
        "an empty restore emits no banner"
    );
}

#[tokio::test]
async fn logon_with_a_load_error_starts_empty_and_silent() {
    let spy = Arc::new(SpyFlaggedStore {
        fail: true,
        ..SpyFlaggedStore::default()
    });
    let services = services_with_flagged_store(spy);
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    {
        let mut flow = MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.restore_flags_and_announce(&mut session)
            .await
            .expect("restore");
    }
    assert!(session.flagged_files().is_empty());
    assert!(terminal.output.is_empty());
}
