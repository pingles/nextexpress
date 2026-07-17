use std::collections::VecDeque;
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE;
use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::app::menu_command::{parse_menu_command, JoinArg, MenuCommand};
use crate::app::menu_flow::test_support::test_services;
use crate::app::services::AppServices;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
use crate::domain::conference::{Conference, ConferenceMembership, MessageBase, MessageBaseRef};
use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
use crate::domain::messaging::mail_store::MailStore;
use crate::domain::password::PasswordHashKind;
use crate::domain::session::typed::MenuSession;
use crate::domain::session::{apply_password_match, CallId, LogonChannel, Session, SessionPolicy};
use crate::domain::user::User;

/// Write-capturing terminal with a scripted input queue
/// (the `FakeTerminal` precedent from `session_driver.rs`);
/// reads past the script return `Eof`.
struct ScriptTerminal {
    inputs: VecDeque<TerminalRead>,
    output: Vec<u8>,
}

impl ScriptTerminal {
    fn new(inputs: impl IntoIterator<Item = TerminalRead>) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            output: Vec::new(),
        }
    }

    fn line(text: &str) -> TerminalRead {
        TerminalRead::Line(text.to_string())
    }
}

impl Terminal for ScriptTerminal {
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
        Box::pin(async move { Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof)) })
    }
}

fn conference(number: u32, name: &str) -> Conference {
    Conference::new(
        number,
        name.to_string(),
        vec![MessageBase::new(number, 1, "main".to_string())],
    )
    .expect("valid conference")
}

fn three_conferences() -> Vec<Conference> {
    vec![
        conference(1, "One"),
        conference(2, "Two"),
        conference(3, "Three"),
    ]
}

/// Conference with two message bases (`main`, `tech`) — the
/// multi-base shape `JM` and the dotted `J` forms target.
fn multi_base_conference(number: u32, name: &str) -> Conference {
    Conference::new(
        number,
        name.to_string(),
        vec![
            MessageBase::new(number, 1, "main".to_string()),
            MessageBase::new(number, 2, "tech".to_string()),
        ],
    )
    .expect("valid conference")
}

/// One multi-base conference with a mail store per base, each
/// holding a single broadcast — lets the per-base read-pointer
/// independence show up as scan output.
fn services_with_multibase_broadcasts() -> AppServices {
    let mut stores = InMemoryMailStores::new();
    for base in [1, 2] {
        let coord = MessageBaseRef::new(1, base);
        let mut store = InMemoryMailStore::new(coord);
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: "carol".to_string(),
                to_name: "ALL".to_string(),
                broadcast_to: BroadcastTo::All,
                subject: "hello everyone".to_string(),
                posted_at: SystemTime::UNIX_EPOCH,
                author_slot: 1,
                addressee_slot: None,
                body: String::new(),
            })
            .expect("insert broadcast");
        stores.register(coord, Box::new(store));
    }
    services_with(
        vec![multi_base_conference(1, "One")],
        stores,
        &std::env::temp_dir(),
    )
}

fn services_with(
    conferences: Vec<Conference>,
    stores: InMemoryMailStores,
    bbs_path: &Path,
) -> AppServices {
    let mut services = test_services();
    services.screens = Arc::new(FileScreenRepository::new(bbs_path.to_path_buf()));
    services.conferences = Arc::new(conferences);
    services.mail_stores = Arc::new(stores);
    services
}

fn services_with_one_broadcast_message() -> AppServices {
    let coord = MessageBaseRef::new(1, 1);
    let mut store = InMemoryMailStore::new(coord);
    store
        .insert(MailDraft {
            visibility: MailVisibility::Public,
            from_name: "carol".to_string(),
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            subject: "hello everyone".to_string(),
            posted_at: SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: None,
            body: String::new(),
        })
        .expect("insert broadcast");
    let mut stores = InMemoryMailStores::new();
    stores.register(coord, Box::new(store));
    services_with(vec![conference(1, "One")], stores, &std::env::temp_dir())
}

fn alice_with_grants(grants: &[u32]) -> User {
    let mut user = User::new(
        2,
        "alice".to_string(),
        PasswordHashKind::Pbkdf210000,
        "hash".to_string(),
        Some("salt".to_string()),
        SystemTime::UNIX_EPOCH,
        100,
    )
    .expect("valid user");
    for &number in grants {
        user.upsert_membership(ConferenceMembership::new(number, true));
    }
    user
}

/// User whose `last_joined` is `conference_number`, so the
/// fixture's auto-rejoin attaches there instead of the
/// lowest-numbered grant — making a later move observable.
fn alice_last_joined(conference_number: u32, grants: &[u32]) -> User {
    let mut user = alice_with_grants(grants);
    let conf = conference(conference_number, "Anywhere");
    user.record_join(&conf, &conf.msgbases()[0]);
    user
}

fn alice_last_joined_two(grants: &[u32]) -> User {
    alice_last_joined(2, grants)
}

/// Menu-phase session attached (via auto-rejoin) to the first
/// accessible conference of `conferences`.
fn menu_session_attached(conferences: &[Conference], user: User) -> MenuSession {
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
    session
        .auto_rejoin_conference(conferences, SystemTime::UNIX_EPOCH)
        .expect("rejoin");
    session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
    MenuSession::from_session(session)
}

fn menu_session(with_visit: bool) -> MenuSession {
    let conferences = vec![conference(1, "One")];
    let user = alice_with_grants(&[1]);
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
    if with_visit {
        session
            .auto_rejoin_conference(&conferences, SystemTime::UNIX_EPOCH)
            .expect("rejoin");
    }
    session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
    MenuSession::from_session(session)
}

/// Runs `handle_join_command` against a fresh flow, returning the
/// session it yields.
async fn run_join(
    services: &AppServices,
    terminal: &mut ScriptTerminal,
    mut session: MenuSession,
    arg: JoinArg,
) -> MenuSession {
    let mut flow = super::super::MenuFlow { terminal, services };
    flow.handle_join_command(&mut session, arg)
        .await
        .expect("join command");
    session
}

fn join_arg(line: &str) -> JoinArg {
    match parse_menu_command(line) {
        MenuCommand::Join(arg) => arg,
        other => panic!("`{line}` must parse as a join command, got {other:?}"),
    }
}

/// Routes `command` through the real `dispatch` (pinning the
/// dispatch arm as well as the handler), returning the continued
/// session.
async fn run_command(
    services: &AppServices,
    terminal: &mut ScriptTerminal,
    mut session: MenuSession,
    command: MenuCommand,
) -> MenuSession {
    let mut flow = super::super::MenuFlow { terminal, services };
    match flow
        .dispatch(&mut session, command)
        .await
        .expect("dispatch")
    {
        super::super::DispatchOutcome::Continue => session,
        super::super::DispatchOutcome::UserRequestedLogoff => {
            panic!("conference navigation must never log the caller off")
        }
    }
}

#[tokio::test]
async fn join_prompt_blank_abort_is_byte_exact_and_stays_put() {
    // Live capture: `J` at the menu yields exactly
    // `b'Conference Number (1-N): '` (no JoinConf screen
    // installed → nothing precedes it; no leading CRLF; trailing
    // space, no trailing CRLF), and a blank line aborts with one
    // CRLF (`amiexpress/express.e:25144-25148`, lineInput `:2378`).
    // Conference numbers 1 and 3 pin that N is the *highest
    // number* (legacy `cmds.numConf`), not the catalogue length.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(3, "Three")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 3]));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    assert_eq!(
        terminal.output,
        b"Conference Number (1-3): \r\n",
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(session.current_conference_number(), Some(1));
}

#[tokio::test]
async fn join_prompt_eof_exits_the_menu_silently() {
    // EOF at a nested prompt is a connection-level exit. It writes
    // nothing beyond the prompt and propagates immediately so the
    // driver can apply the carrier-loss transition.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let mut session = menu_session_attached(&conferences, alice_with_grants(&[1, 2, 3]));
    let result = {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_join_command(&mut session, JoinArg::Missing)
            .await
    };
    assert!(matches!(
        result,
        Err(super::super::MenuFlowError::Exit(
            super::super::MenuExit::CarrierLost
        ))
    ));
    assert_eq!(terminal.output, b"Conference Number (1-3): ");
    assert_eq!(session.current_conference_number(), Some(1));
}

#[tokio::test]
async fn join_prompt_clamps_high_input_to_the_highest_conference() {
    // Live capture: `99` at `Conference Number (1-2): ` is clamped
    // to the highest conference (`amiexpress/express.e:25154`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("99")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2, 3]));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    assert_eq!(session.current_conference_number(), Some(3));
    assert_eq!(
        terminal.output,
        b"Conference Number (1-3): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Three\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_prompt_clamps_zero_input_to_conference_one() {
    // Live capture: `0` clamps to conference 1
    // (`amiexpress/express.e:25153`). The session starts in 2 so
    // the clamp's effect is observable as a move.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("0")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2, 3]));
    assert_eq!(session.current_conference_number(), Some(2));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    assert_eq!(session.current_conference_number(), Some(1));
    assert_eq!(
        terminal.output,
        b"Conference Number (1-3): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_prompt_clamps_non_numeric_input_to_conference_one() {
    // Live capture: `abc` → Val 0 → clamps to conference 1; no
    // error message, no re-prompt (`amiexpress/express.e:25150-25154`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("abc")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2, 3]));
    assert_eq!(session.current_conference_number(), Some(2));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    assert_eq!(session.current_conference_number(), Some(1));
    // Byte-exactness also pins single-shot behaviour: exactly one
    // prompt, no error text, straight into the join output.
    assert_eq!(
        terminal.output,
        b"Conference Number (1-3): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_prompt_whitespace_only_input_is_not_blank_and_joins_conference_one() {
    // A whitespace-only line is NOT the blank abort — there is no
    // trimming; it `Val`s to 0 and clamps to conference 1
    // (`amiexpress/express.e:25148` checks StrLen of the raw
    // buffer).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line(" ")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2, 3]));
    assert_eq!(session.current_conference_number(), Some(2));
    let session = run_join(&services, &mut terminal, session, join_arg("J 0")).await;
    assert_eq!(
        session.current_conference_number(),
        Some(1),
        "whitespace-only input joins conference 1, it does not abort"
    );
    assert_eq!(
        terminal.output,
        b"Conference Number (1-3): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_argument_with_digit_prefix_joins_that_conference_directly() {
    // `J 2abc` → Val("2abc") = 2 → in range → joins 2 without any
    // prompt (`amiexpress/express.e:25131`, `:25142`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2, 3]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 2abc")).await;
    assert_eq!(session.current_conference_number(), Some(2));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Two\r\n".to_vec(),
        "no prompt may precede a direct in-range join, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_direct_out_of_range_argument_opens_the_prompt_instead_of_clamping() {
    // Live capture: `J 99` (and `J 0`, `J -1`, `J abc`) opens the
    // prompt — clamping applies only to input typed *at* the
    // prompt (`amiexpress/express.e:25142` vs `:25153-25154`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 99")).await;
    assert_eq!(
        terminal.output,
        b"Conference Number (1-2): \r\n",
        "out-of-range direct argument must prompt, not clamp; got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(session.current_conference_number(), Some(1));
}

#[tokio::test]
async fn join_negative_argument_opens_the_prompt() {
    // `J -1` → Val = -1 → below range → prompt
    // (`amiexpress/express.e:25142`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let _session = run_join(&services, &mut terminal, session, join_arg("J -1")).await;
    assert_eq!(terminal.output, b"Conference Number (1-2): \r\n");
}

#[tokio::test]
async fn join_dotted_form_on_a_single_base_conference_is_byte_identical_to_plain_j() {
    // Live capture: `J 1.1` joins conference 1 (its only base)
    // with the normal join output — no prompt, no extra bytes
    // (`amiexpress/express.e:25132-25133`, then the in-range
    // direct join).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    let mut dotted_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_join(&services, &mut dotted_terminal, session, join_arg("J 1.1")).await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));

    let mut plain_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let _session = run_join(&services, &mut plain_terminal, session, join_arg("J 1")).await;

    assert_eq!(
        dotted_terminal.output,
        plain_terminal.output,
        "`J 1.1` must be byte-identical to `J 1`: got {:?} vs {:?}",
        String::from_utf8_lossy(&dotted_terminal.output),
        String::from_utf8_lossy(&plain_terminal.output)
    );
    assert_eq!(
        dotted_terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&dotted_terminal.output)
    );
}

#[tokio::test]
async fn join_dotted_form_joins_the_requested_base_of_a_multi_base_conference() {
    // Pinned from source (`amiexpress/express.e:25132-25133` +
    // `joinConf` banner `:5077-5084`): the dotted form lands on
    // the requested base and the announcement appends ` [<base>]`
    // — spacing identical to the single-base form.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), multi_base_conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 2.2")).await;
    assert_eq!(session.current_msgbase(), Some((2, 2)));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Two [tech]\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_two_token_form_is_equivalent_to_the_dotted_form() {
    // `J 2 2` ≡ `J 2.2` (`amiexpress/express.e:25134-25135`); the
    // third token of `J 2 2 9` is discarded (only items 0 and 1
    // are read).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), multi_base_conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["J 2 2", "J 2 2 9"] {
        let mut terminal = ScriptTerminal::new([]);
        let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
        let session = run_join(&services, &mut terminal, session, join_arg(line)).await;
        assert_eq!(
            session.current_msgbase(),
            Some((2, 2)),
            "`{line}` must join conference 2 base 2"
        );
        assert_eq!(
            terminal.output,
            b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Two [tech]\r\n".to_vec(),
            "`{line}` got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }
}

#[tokio::test]
async fn join_explicit_out_of_range_base_opens_the_msgbase_prompt_and_blank_aborts() {
    // Live capture: `J 1 2` on single-base conference 1 yields
    // exactly `b'J 1 2\r\nMessage Base Number (1-1): '` — the
    // legacy message-base prompt (`amiexpress/express.e:25169-25180`)
    // fires even on a single-base conference, and the single-base
    // notice is JM-only and must NOT appear here. A blank line
    // aborts with one CRLF (`:25176`, lineInput `:2378`) and stays
    // put.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["J 1 2", "J 1.9", "J 1."] {
        let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
        let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
        let session = run_join(&services, &mut terminal, session, join_arg(line)).await;
        assert_eq!(
            terminal.output,
            b"Message Base Number (1-1): \r\n".to_vec(),
            "`{line}` must open the (1-1) prompt and blank-abort, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
        assert_eq!(
            session.current_msgbase(),
            Some((2, 1)),
            "`{line}` + blank must not join anywhere"
        );
    }
}

#[tokio::test]
async fn join_msgbase_prompt_answer_is_not_clamped_the_domain_resets_to_the_primary_base() {
    // The J/JM post-prompt asymmetry (Tier C C4b): J's message-base
    // prompt passes its `Val` to `joinConf` UNCLAMPED
    // (`amiexpress/express.e:25179`), and `joinConf` resets an
    // out-of-range base to the primary (`:4995`) — so answering
    // `9` at a `(1-2)` prompt joins base 1 [main], NOT the clamped
    // base 2 [tech] that JM's own prompt would produce
    // (`:25233-25234`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("9")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 1 9")).await;
    assert_eq!(
        session.current_msgbase(),
        Some((1, 1)),
        "an out-of-range prompt answer must land on the primary base, not clamp to the top"
    );
    assert_eq!(
        terminal.output,
        b"Message Base Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [main]\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_msgbase_prompt_on_a_single_base_conference_joins_the_only_base_unclamped() {
    // Observed semantics chained end-to-end: `J 1 2` on
    // single-base conference 1, answering `5` at the `(1-1)`
    // prompt — the unclamped 5 reaches the domain join, which
    // resets it to the primary base, so the caller lands on base 1
    // with the plain (no-bracket) announcement and never sees the
    // JM-only single-base notice.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("5")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 1 2")).await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    assert_eq!(
        terminal.output,
        b"Message Base Number (1-1): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_msgbase_prompt_in_range_answer_joins_that_base() {
    // An in-range prompt answer joins exactly that base of the
    // already-resolved target conference
    // (`amiexpress/express.e:25179`, then `joinConf`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("2")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 1 9")).await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    assert_eq!(
        terminal.output,
        b"Message Base Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [tech]\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_msgbase_prompt_eof_exits_the_menu_silently() {
    // EOF at the message-base prompt writes nothing extra
    // (`amiexpress/express.e:25175` propagates the status) and exits
    // immediately so the driver can apply carrier loss.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let mut session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let result = {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_join_command(&mut session, join_arg("J 1 2"))
            .await
    };
    assert!(matches!(
        result,
        Err(super::super::MenuFlowError::Exit(
            super::super::MenuExit::CarrierLost
        ))
    ));
    assert_eq!(terminal.output, b"Message Base Number (1-1): ");
    assert_eq!(session.current_msgbase(), Some((2, 1)));
}

#[tokio::test]
async fn join_msgbase_prompt_screen_comes_from_the_current_conference() {
    // The legacy `confScreenDir` is repointed only inside
    // `joinConf` (`amiexpress/express.e:5052`), so when `J`
    // prompts for *another* conference's bases the `JoinMsgBase`
    // screen resolves against the conference the caller is still
    // in — while the prompt's `(1-N)` bound is the TARGET's count
    // (`:25167`).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("Conf01")).expect("mkdir");
    std::fs::create_dir_all(dir.path().join("Conf02")).expect("mkdir");
    std::fs::write(
        dir.path().join("Conf01").join("JoinMsgBase.txt"),
        b"TARGET CONF\n",
    )
    .expect("write screen");
    std::fs::write(
        dir.path().join("Conf02").join("JoinMsgBase.txt"),
        b"CURRENT CONF\n",
    )
    .expect("write screen");
    let conferences = vec![multi_base_conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let _session = run_join(&services, &mut terminal, session, join_arg("J 1 9")).await;
    assert_eq!(
        terminal.output,
        b"CURRENT CONF\r\nMessage Base Number (1-2): \r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_msgbase_form_access_check_precedes_the_base_range_check() {
    // Legacy ordering: `checkConfAccess`
    // (`amiexpress/express.e:25156`) runs before the message-base
    // range check (`:25168`) — a denied conference always writes
    // the no-access notice, even when the requested base is also
    // out of range.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), multi_base_conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["J 2.1", "J 2.9", "J 2 9"] {
        let mut terminal = ScriptTerminal::new([]);
        let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
        let session = run_join(&services, &mut terminal, session, join_arg(line)).await;
        assert_eq!(
            terminal.output,
            NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE.to_vec(),
            "`{line}` must write the no-access notice, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
        assert_eq!(session.current_msgbase(), Some((1, 1)));
    }
}

#[tokio::test]
async fn join_msgbase_request_survives_the_conference_prompt() {
    // The legacy parses `newMsgBase` from the dotted argument
    // (`amiexpress/express.e:25133`) *before* the conference
    // prompt fires (`:25142`), so `J 99.2` + `1` at the prompt
    // joins conference 1 at base 2.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("1")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 99.2")).await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    assert_eq!(
        terminal.output,
        b"Conference Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [tech]\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn jm_on_a_single_base_conference_writes_the_exact_failure_bytes() {
    // Live capture: every non-dotted `JM` form — no-arg, "valid"
    // `JM 1`, out-of-range `JM 9`, non-numeric `JM abc` — on a
    // single-base conference writes exactly the legacy notice and
    // neither joins nor prompts (`amiexpress/express.e:25211-25215`:
    // the NMSGBASES tooltype probe returns -1 when absent, failing
    // before any range logic).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["JM", "JM 1", "JM 9", "JM abc", "jm 1"] {
        let mut terminal = ScriptTerminal::new([]);
        let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
        let session =
            run_command(&services, &mut terminal, session, parse_menu_command(line)).await;
        assert_eq!(
            terminal.output,
            b"\r\nThis conference does not contain multiple message bases\r\n\r\n".to_vec(),
            "`{line}` must write the single-base notice, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
        assert_eq!(
            session.current_msgbase(),
            Some((1, 1)),
            "`{line}` must not move the session"
        );
    }
}

#[tokio::test]
async fn jm_in_range_argument_joins_the_base_with_the_bracketed_announcement() {
    // Pinned from source (`legacy-JM.md`): `JM <in-range n>` on a
    // multi-base conference joins base n of the *current*
    // conference with the full join sequence; the announcement
    // appends ` [<base>]` (`amiexpress/express.e:5077-5084`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 2"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [tech]\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn jm_rejoining_the_current_base_runs_the_full_join_sequence() {
    // There is no "already there" check anywhere in
    // `internalCommandJM` or `joinConf`: `JM <current>` performs
    // the complete re-join with identical output.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 1"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [main]\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn jm_missing_or_out_of_range_opens_the_prompt_whose_answer_is_clamped() {
    // Every non-dotted `JM` form without an in-range base on a
    // multi-base conference opens the single-shot
    // `Message Base Number (1-N): ` prompt
    // (`amiexpress/express.e:25220-25230`), and the answer is
    // CLAMPED into `1..=N` (`:25233-25234`) — answering `9` at a
    // `(1-2)` prompt joins base 2 [tech], the documented asymmetry
    // with `J`'s unclamped prompt (which would land on base 1).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["JM", "JM 9", "JM abc", "JM 0", "JM -1"] {
        let mut terminal = ScriptTerminal::new([ScriptTerminal::line("9")]);
        let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
        let session =
            run_command(&services, &mut terminal, session, parse_menu_command(line)).await;
        assert_eq!(
            session.current_msgbase(),
            Some((1, 2)),
            "`{line}` + `9` must clamp to the top base"
        );
        assert_eq!(
                terminal.output,
                b"Message Base Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [tech]\r\n"
                    .to_vec(),
                "`{line}` got {:?}",
                String::from_utf8_lossy(&terminal.output)
            );
    }
}

#[tokio::test]
async fn jm_prompt_answer_below_range_clamps_to_base_one() {
    // The low side of `JM`'s clamp (`amiexpress/express.e:25233`):
    // `0` (and any non-numeric `Val` of 0) joins base 1.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("0")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 9"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    assert_eq!(
        terminal.output,
        b"Message Base Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [main]\r\n"
            .to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn jm_prompt_blank_aborts_but_eof_exits_the_menu() {
    // Blank input at the `JM` prompt aborts silently
    // (`amiexpress/express.e:25228`) — the only wire output is
    // `lineInput`'s trailing CRLF (`:2378`); the session keeps its
    // base. EOF writes nothing at all and propagates a carrier exit.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_command(&services, &mut terminal, session, parse_menu_command("JM")).await;
    assert_eq!(terminal.output, b"Message Base Number (1-2): \r\n".to_vec());
    assert_eq!(session.current_msgbase(), Some((1, 1)));

    let mut terminal = ScriptTerminal::new([]);
    let mut session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let result = {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.dispatch(&mut session, parse_menu_command("JM")).await
    };
    assert!(matches!(
        result,
        Err(super::super::MenuFlowError::Exit(
            super::super::MenuExit::CarrierLost
        ))
    ));
    assert_eq!(terminal.output, b"Message Base Number (1-2): ".to_vec());
    assert_eq!(session.current_msgbase(), Some((1, 1)));
}

#[tokio::test]
async fn jm_prompt_renders_the_conference_local_joinmsgbase_screen() {
    // The `JoinMsgBase` screen precedes the prompt when installed
    // (`amiexpress/express.e:25221-25222`): the conference-local
    // asset wins over the node-level one; with neither, nothing
    // precedes the prompt (live reference).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("Conf01")).expect("mkdir");
    std::fs::create_dir_all(dir.path().join("Screens")).expect("mkdir");
    std::fs::write(
        dir.path().join("Conf01").join("JoinMsgBase.txt"),
        b"CONF LOCAL\n",
    )
    .expect("write screen");
    std::fs::write(
        dir.path().join("Screens").join("JoinMsgBase.txt"),
        b"NODE LEVEL\n",
    )
    .expect("write screen");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let _session = run_command(&services, &mut terminal, session, parse_menu_command("JM")).await;
    assert_eq!(
        terminal.output,
        b"CONF LOCAL\r\nMessage Base Number (1-2): \r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn next_msgbase_steps_up_and_joins_in_full() {
    // `>>` in bounds is a full message-base join of the current
    // conference (`internalCommandGT2`,
    // `amiexpress/express.e:24585-24590`) — byte-identical to
    // `JM <n>`'s output, bracketed base name included.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    let session = run_command(&services, &mut terminal, session, parse_menu_command(">>")).await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [tech]\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn prev_msgbase_steps_down_and_joins_in_full() {
    // `<<` in bounds mirrors `>>` downward (`internalCommandLT2`,
    // `amiexpress/express.e:24571-24576`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 2"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    terminal.output.clear();
    let session = run_command(&services, &mut terminal, session, parse_menu_command("<<")).await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One [main]\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn msgbase_steps_past_either_edge_fall_into_the_jm_prompt() {
    // `<<` below base 1 (`amiexpress/express.e:24573-24574`) and
    // `>>` above the count (`:24587-24588`) both run the `JM`
    // no-arg flow: on a multi-base conference that is the
    // `Message Base Number (1-N): ` prompt; blank stays put. No
    // wraparound.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![multi_base_conference(1, "One")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    // `<<` at the bottom.
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_command(&services, &mut terminal, session, parse_menu_command("<<")).await;
    assert_eq!(terminal.output, b"Message Base Number (1-2): \r\n".to_vec());
    assert_eq!(session.current_msgbase(), Some((1, 1)));

    // `>>` at the top (position there first).
    let mut terminal = ScriptTerminal::new([]);
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 2"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 2)));
    terminal.output.clear();
    terminal.inputs.push_back(ScriptTerminal::line(""));
    let session = run_command(&services, &mut terminal, session, parse_menu_command(">>")).await;
    assert_eq!(terminal.output, b"Message Base Number (1-2): \r\n".to_vec());
    assert_eq!(session.current_msgbase(), Some((1, 2)));
}

#[tokio::test]
async fn msgbase_steps_on_a_single_base_conference_write_the_exact_failure_bytes() {
    // Live capture: both `<<` and `>>` on a single-base conference
    // print exactly the legacy single-base notice — the edge falls
    // into `internalCommandJM('')`, whose `NMSGBASES` probe fails
    // first (`amiexpress/express.e:25211-25215`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    for line in ["<<", ">>", "<< 2", ">> 2"] {
        let mut terminal = ScriptTerminal::new([]);
        let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
        let session =
            run_command(&services, &mut terminal, session, parse_menu_command(line)).await;
        assert_eq!(
            terminal.output,
            b"\r\nThis conference does not contain multiple message bases\r\n\r\n".to_vec(),
            "`{line}` must write the single-base notice, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
        assert_eq!(session.current_msgbase(), Some((1, 1)));
    }
}

#[tokio::test]
async fn jm_dotted_argument_is_byte_identical_to_the_j_dotted_form() {
    // Live capture: `JM 1.1` joins conference 1 — identical to
    // `J 1.1` (delegation at `amiexpress/express.e:25203-25206`
    // hands the raw params to `internalCommandJ`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    let mut jm_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_command(
        &services,
        &mut jm_terminal,
        session,
        parse_menu_command("JM 1.1"),
    )
    .await;
    assert_eq!(session.current_msgbase(), Some((1, 1)));

    let mut direct_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let _session = run_join(&services, &mut direct_terminal, session, join_arg("J 1.1")).await;

    assert_eq!(
        jm_terminal.output,
        direct_terminal.output,
        "`JM 1.1` must be byte-identical to `J 1.1`: got {:?} vs {:?}",
        String::from_utf8_lossy(&jm_terminal.output),
        String::from_utf8_lossy(&direct_terminal.output)
    );
}

#[tokio::test]
async fn jm_joins_track_read_pointers_per_message_base() {
    // `ConferenceMembership.pointers` is per-msgbase: the join
    // scan that advances base 2's read pointer must leave base
    // 1's pointer untouched, so each base's broadcast surfaces
    // exactly once.
    let services = services_with_multibase_broadcasts();
    let session = menu_session_attached(services.conferences.as_ref(), alice_with_grants(&[1]));
    assert_eq!(session.current_msgbase(), Some((1, 1)));

    let mut terminal = ScriptTerminal::new([]);
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 2"),
    )
    .await;
    let first = String::from_utf8_lossy(&terminal.output).into_owned();
    assert!(
        first.contains("You have 1 new message. First: 1."),
        "the first base-2 join must surface its broadcast, got {first:?}"
    );

    let mut terminal = ScriptTerminal::new([]);
    let session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 2"),
    )
    .await;
    let second = String::from_utf8_lossy(&terminal.output).into_owned();
    assert!(
        second.contains("No new mail."),
        "re-joining base 2 must start past the advanced pointer, got {second:?}"
    );

    let mut terminal = ScriptTerminal::new([]);
    let _session = run_command(
        &services,
        &mut terminal,
        session,
        parse_menu_command("JM 1"),
    )
    .await;
    let third = String::from_utf8_lossy(&terminal.output).into_owned();
    assert!(
        third.contains("You have 1 new message. First: 1."),
        "base 1's pointer must be independent of base 2's, got {third:?}"
    );
}

#[tokio::test]
async fn join_denied_writes_the_notice_and_stays_in_the_current_conference() {
    // Legacy `internalCommandJ` access check
    // (`amiexpress/express.e:25156-25158`): denied requests print
    // the no-access notice and stay put — no fallback join, no
    // logoff.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_join(&services, &mut terminal, session, join_arg("J 2")).await;
    assert_eq!(
        terminal.output,
        NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE.to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(
        session.current_conference_number(),
        Some(1),
        "a denied join must leave the session in its current conference"
    );
}

#[tokio::test]
async fn join_prompt_input_clamped_onto_a_denied_conference_stays_put() {
    // The access check applies to clamped prompt input too: the
    // user types 2 at the prompt but holds no grant for 2.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("2")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1]));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    let mut expected = b"Conference Number (1-2): ".to_vec();
    expected.extend_from_slice(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE);
    assert_eq!(
        terminal.output,
        expected,
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(session.current_conference_number(), Some(1));
}

#[tokio::test]
async fn join_prompt_input_landing_in_a_catalogue_hole_is_denied() {
    // Defensive: legacy contiguous numbering makes every clamped
    // number resolvable, but NextExpress allows gaps — prompt
    // input `2` against a {1, 3} catalogue denies and stays put.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(3, "Three")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("2")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 3]));
    let session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    let mut expected = b"Conference Number (1-3): ".to_vec();
    expected.extend_from_slice(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE);
    assert_eq!(terminal.output, expected);
    assert_eq!(session.current_conference_number(), Some(1));
}

#[tokio::test]
async fn joinconf_screen_bytes_precede_the_prompt_when_installed() {
    // `displayScreen(SCREEN_JOINCONF)` runs before the prompt
    // (`amiexpress/express.e:25143`); the screen renders only when
    // the sysop installs `Screens/JoinConf.txt`.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("Screens")).expect("mkdir");
    std::fs::write(
        dir.path().join("Screens").join("JoinConf.txt"),
        b"PICK A CONF\n",
    )
    .expect("write screen");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let _session = run_join(&services, &mut terminal, session, JoinArg::Missing).await;
    assert_eq!(
        terminal.output,
        b"PICK A CONF\r\nConference Number (1-2): \r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_scan_without_an_open_visit_writes_nothing() {
    // The auto-scan-on-join is silent when the session has no open
    // visit (the deleted `ScanMailOutcome::NoOpenMsgbase` arm): no
    // summary, no error — the menu prompt follows immediately.
    let services = services_with_one_broadcast_message();
    let mut terminal = ScriptTerminal::new([]);
    let mut session = menu_session(false);
    {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.scan_mail_on_join(&mut session).await.expect("scan");
    }
    assert!(
        terminal.output.is_empty(),
        "scan without a visit must write nothing, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn join_scan_follows_the_read_pointer_not_message_one() {
    // Spec `conferences.allium:ScanMailOnJoin` scans from
    // `pointers.last_scanned + 1` (`from_message = 0` sentinel; the
    // legacy `forceMailScan = NOFORCE`). A broadcast message stays
    // "unread" for as long as it is in scan range, so the second
    // join-scan only reports `No new mail.` if the first one
    // advanced the pointer past it — a mutant hardcoding the scan
    // to start from message 1 re-surfaces the broadcast forever.
    let services = services_with_one_broadcast_message();
    let mut terminal = ScriptTerminal::new([]);
    let mut session = menu_session(true);
    {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.scan_mail_on_join(&mut session).await.expect("scan");
    }
    let first = String::from_utf8_lossy(&terminal.output).into_owned();
    assert!(
        first.contains("You have 1 new message. First: 1."),
        "first scan must surface the broadcast, got {first:?}"
    );
    // The SCREEN_MAILSCAN render is gated on `unread_count > 0`
    // (here the adapter's built-in fallback banner).
    assert!(
        first.contains("New mail in this conference"),
        "an unread scan must render the mailscan screen, got {first:?}"
    );

    terminal.output.clear();
    {
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.scan_mail_on_join(&mut session).await.expect("rescan");
    }
    let second = String::from_utf8_lossy(&terminal.output).into_owned();
    assert!(
        second.contains("No new mail."),
        "second scan must start past the advanced pointer, got {second:?}"
    );
    assert!(
        !second.contains("New mail in this conference"),
        "a zero-unread scan must not render the mailscan screen, got {second:?}"
    );
}

#[tokio::test]
async fn next_conference_join_is_byte_identical_to_the_direct_explicit_join() {
    // Live capture: `>` from conference 1 produces exactly the
    // `J 2` join output — the legacy hit path is
    // `joinConf(newConf,1,FALSE,FALSE)`
    // (`amiexpress/express.e:24562`), the same call a direct join
    // makes. Run both against equivalent sessions and compare the
    // wire bytes, then pin the literal.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    let mut next_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let session = run_command(
        &services,
        &mut next_terminal,
        session,
        MenuCommand::NextConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(2));

    let mut direct_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    let _session = run_join(&services, &mut direct_terminal, session, join_arg("J 2")).await;

    assert_eq!(
        next_terminal.output,
        direct_terminal.output,
        "`>` must be byte-identical to `J 2`: got {:?} vs {:?}",
        String::from_utf8_lossy(&next_terminal.output),
        String::from_utf8_lossy(&direct_terminal.output)
    );
    assert_eq!(
        next_terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Two\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&next_terminal.output)
    );
}

#[tokio::test]
async fn prev_conference_join_is_byte_identical_to_the_direct_explicit_join() {
    // Live capture: `<` from conference 2 joins conference 1 with
    // the normal join output (`joinConf(newConf,1,FALSE,FALSE)`,
    // `amiexpress/express.e:24543`).
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());

    let mut prev_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    assert_eq!(session.current_conference_number(), Some(2));
    let session = run_command(
        &services,
        &mut prev_terminal,
        session,
        MenuCommand::PrevConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(1));

    let mut direct_terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let _session = run_join(&services, &mut direct_terminal, session, join_arg("J 1")).await;

    assert_eq!(
        prev_terminal.output,
        direct_terminal.output,
        "`<` must be byte-identical to `J 1`: got {:?} vs {:?}",
        String::from_utf8_lossy(&prev_terminal.output),
        String::from_utf8_lossy(&direct_terminal.output)
    );
    assert_eq!(
        prev_terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&prev_terminal.output)
    );
}

#[tokio::test]
async fn next_conference_skips_a_conference_without_a_grant() {
    // The legacy walk skips inaccessible conferences transparently
    // — no message per skip (`amiexpress/express.e:24555-24557`).
    // Conference 2's membership is revoked, so `>` from 1 lands
    // on 3.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let mut user = alice_with_grants(&[1, 3]);
    user.upsert_membership(ConferenceMembership::new(2, false));
    let session = menu_session_attached(&conferences, user);
    assert_eq!(session.current_conference_number(), Some(1));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::NextConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(3));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Three\r\n".to_vec(),
        "the skip is silent — straight to the conference-3 join, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn prev_conference_skips_a_conference_without_a_grant() {
    // Mirror walk downward (`amiexpress/express.e:24536-24538`):
    // from 3 with no grant for 2, `<` lands on 1.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = three_conferences();
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_last_joined(3, &[1, 3]));
    assert_eq!(session.current_conference_number(), Some(3));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::PrevConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(1));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m One\r\n".to_vec(),
        "the skip is silent — straight to the conference-1 join, got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn prev_conference_at_the_bottom_edge_opens_the_join_prompt_and_blank_stays_put() {
    // Live capture: `<` at the lowest accessible conference yields
    // `b'<\r\nConference Number (1-2): '` — the walk falls off the
    // bottom and the legacy runs `internalCommandJ('')`
    // (`amiexpress/express.e:24540-24541`); a blank line at that
    // prompt aborts silently and stays put. No wraparound.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 2]));
    assert_eq!(session.current_conference_number(), Some(1));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::PrevConference,
    )
    .await;
    assert_eq!(
        terminal.output,
        b"Conference Number (1-2): \r\n",
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(
        session.current_conference_number(),
        Some(1),
        "blank input at the fallback prompt must stay in the current conference"
    );
}

#[tokio::test]
async fn next_conference_at_the_top_edge_opens_the_join_prompt_and_blank_stays_put() {
    // Live capture: `>` at the highest accessible conference yields
    // `b'>\r\nConference Number (1-2): '`
    // (`amiexpress/express.e:24559-24560`); blank aborts and stays
    // put. No wraparound.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    assert_eq!(session.current_conference_number(), Some(2));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::NextConference,
    )
    .await;
    assert_eq!(
        terminal.output,
        b"Conference Number (1-2): \r\n",
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
    assert_eq!(
        session.current_conference_number(),
        Some(2),
        "blank input at the fallback prompt must stay in the current conference"
    );
}

#[tokio::test]
async fn edge_fallback_prompt_accepts_a_number_like_a_bare_j() {
    // The edge fallback IS `internalCommandJ('')`
    // (`amiexpress/express.e:24541`), so typed prompt input joins
    // exactly as it would after a bare `J` — pinning that the miss
    // path delegates the full prompt flow, not just the prompt
    // text.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(2, "Two")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([ScriptTerminal::line("2")]);
    let session = menu_session_attached(&conferences, alice_last_joined_two(&[1, 2]));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::NextConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(2));
    assert_eq!(
        terminal.output,
        b"Conference Number (1-2): \r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Two\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[tokio::test]
async fn next_conference_walks_the_catalogue_not_number_arithmetic() {
    // NextExpress allows non-contiguous conference numbers; the
    // walk follows the sorted catalogue ({1, 5}: `>` from 1 joins
    // 5), not `current + 1` probing.
    let dir = tempfile::tempdir().expect("tempdir");
    let conferences = vec![conference(1, "One"), conference(5, "Five")];
    let services = services_with(conferences.clone(), InMemoryMailStores::new(), dir.path());
    let mut terminal = ScriptTerminal::new([]);
    let session = menu_session_attached(&conferences, alice_with_grants(&[1, 5]));
    let session = run_command(
        &services,
        &mut terminal,
        session,
        MenuCommand::NextConference,
    )
    .await;
    assert_eq!(session.current_conference_number(), Some(5));
    assert_eq!(
        terminal.output,
        b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Five\r\n".to_vec(),
        "got {:?}",
        String::from_utf8_lossy(&terminal.output)
    );
}

#[test]
fn conference_number_prompt_matches_the_legacy_capture() {
    // Live capture (AmiExpress 5.6.0, two conferences):
    // `b'Conference Number (1-2): '` — trailing space, no CRLF on
    // either side (`amiexpress/express.e:25144`).
    assert_eq!(
        super::render_conference_number_prompt(2),
        b"Conference Number (1-2): "
    );
}

#[test]
fn render_scan_summary_emits_no_new_mail_for_zero() {
    // Legacy `\tNo New Mail!\b\n` (`amiexpress/express.e:26499`).
    assert_eq!(super::render_scan_summary(0, None), b"\r\nNo new mail.\r\n");
    // first_unread_number is ignored when count is zero.
    assert_eq!(
        super::render_scan_summary(0, Some(5)),
        b"\r\nNo new mail.\r\n"
    );
}

#[test]
fn render_scan_summary_pluralises_message_for_more_than_one() {
    assert_eq!(
        super::render_scan_summary(3, Some(5)),
        b"\r\nYou have 3 new messages. First: 5.\r\n",
    );
}

#[test]
fn render_scan_summary_uses_singular_for_one_message() {
    assert_eq!(
        super::render_scan_summary(1, Some(7)),
        b"\r\nYou have 1 new message. First: 7.\r\n",
    );
}

#[test]
fn render_scan_summary_handles_missing_first_unread_number() {
    // Defensive: a non-zero count without a number would be a
    // bug, but the renderer must not panic on it.
    assert_eq!(
        super::render_scan_summary(2, None),
        b"\r\nYou have 2 new messages.\r\n",
    );
}
