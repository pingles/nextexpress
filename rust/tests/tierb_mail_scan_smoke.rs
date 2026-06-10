//! Tier B (Slice B1) in-process smoke: the `MS` multi-conference mail
//! scan.
//!
//! Boots a [`TelnetListener`] in-process (the `quickwins_smoke.rs`
//! shape) with two conferences the seeded sysop can access. Conference
//! 1 ("One") has an empty message base; conference 2 ("Two") has one
//! message addressed to the sysop. The sysop auto-rejoins conference 1
//! at logon, then types `MS`:
//!
//!   * the `Scanning conferences for mail...` header appears once;
//!   * conference 1's banner is followed by `No mail today!` (nothing
//!     new since its empty base was scanned on join);
//!   * conference 2's banner is followed by the legacy
//!     `Type/From/Subject/Msg` listing table with the seeded message;
//!   * the session is still attached to conference 1 afterwards (`MS`
//!     restores the original conference — here it never left it).
//!
//! This proves the headline Tier B capability is reachable through the
//! same composition root and telnet adapter the binary uses.

use std::sync::Arc;
use std::time::Duration;

use nextexpress::adapters::file_mail_store::FileMailStore;
use nextexpress::adapters::in_memory_caller_log::InMemoryCallerLog;
use nextexpress::adapters::in_memory_file_repository::InMemoryFileRepository;
use nextexpress::adapters::in_memory_mail_stores::InMemoryMailStores;
use nextexpress::adapters::in_memory_user_repository::InMemoryUserRepository;
use nextexpress::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use nextexpress::adapters::telnet_listener::TelnetListener;
use nextexpress::app::config::Config;
use nextexpress::app::mail_stores::MailStores;
use nextexpress::app::seed;
use nextexpress::app::services::{
    SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedUserRepo,
};
use nextexpress::bootstrap;
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{Conference, MessageBase, MessageBaseRef, ScanFlag};
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

#[tokio::test]
async fn ms_scans_every_accessible_conference_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");
    // Conference 2 has one unread message addressed to the seeded sysop
    // (slot 1). Conference 1 is left empty.
    std::fs::write(
        conf2_msgbase.join("0000001.json"),
        seeded_mail_json(2, 1, "Carol", "Tier B Greetings"),
    )
    .expect("seed conf2 message");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"MS").await;
    // Conference 2's matched mail triggers the read-it-now prompt; drain
    // up to it, then decline so the scan returns to the menu.
    let out = drain_until(
        &mut stream,
        b"Would you like to read it now \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[32m?\x1b[0m ",
    )
    .await;

    // The opening header (`amiexpress/express.e:25258`).
    assert!(
        contains(&out, b"\r\nScanning conferences for mail...\r\n\r\n"),
        "missing MS header, got {:?}",
        String::from_utf8_lossy(&out)
    );
    // Conference 1: banner then `No mail today!` (empty base).
    assert!(
        contains(&out, b"\x1b[32mScanning Conference\x1b[33m: \x1b[0mOne - "),
        "missing conference 1 banner, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        contains(&out, b"No mail today!\r\n"),
        "conference 1 (empty) must report `No mail today!`, got {:?}",
        String::from_utf8_lossy(&out)
    );
    // Conference 2: banner then the listing table with the seeded mail.
    assert!(
        contains(&out, b"\x1b[32mScanning Conference\x1b[33m: \x1b[0mTwo - "),
        "missing conference 2 banner, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        contains(
            &out,
            b"\x1b[32mType     From                           Subject                Msg    \r\n"
        ),
        "missing listing table header, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        contains(&out, b"Carol") && contains(&out, b"Tier B Greetings"),
        "missing the seeded row's From/Subject, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        contains(&out, b"\x1b[0m000001\r\n"),
        "missing the zero-padded message number column, got {:?}",
        String::from_utf8_lossy(&out)
    );

    // Decline the read-it-now offer; the scan returns to the menu.
    write_line(&mut stream, b"N").await;
    let menu = drain_until(&mut stream, b"mins. left): ").await;

    // Restore invariant: the menu prompt that follows `MS` still shows
    // conference 1 ("One") — the scan never moved the session.
    assert!(
        contains(&menu, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "MS must leave the session in conference 1, got {:?}",
        String::from_utf8_lossy(&menu)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn ms_offers_to_read_found_mail_now_and_drops_into_the_read_subprompt() {
    // Legacy `searchNewMail` getOUT (`amiexpress/express.e:11738-11765`):
    // once a base's listing shows matched mail, AmiExpress asks
    // `Would you like to read it now ` (`yesNo(1)`, default Yes) and, on
    // Yes, drops into the read/reply sub-prompt for the found message,
    // restoring the caller's home conference afterwards.
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");
    std::fs::write(
        conf2_msgbase.join("0000001.json"),
        seeded_mail_json(2, 1, "Carol", "Tier B Greetings"),
    )
    .expect("seed conf2 message");
    // A second unread message so the found base's highest is 2: reading
    // the found message 1 opens the sub-prompt at the NEXT message (2),
    // pinning the read-first `start + 1` entry (slice B10).
    std::fs::write(
        conf2_msgbase.join("0000002.json"),
        r#"{
            "conference_number": 2,
            "msgbase_number": 1,
            "number": 2,
            "visibility": "public",
            "from_name": "Dave",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "Tier B Follow-up",
            "posted_at": "1970-01-01T00:00:02Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": 1,
            "body": "Second.\n"
        }"#,
    )
    .expect("seed conf2 message 2");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"MS").await;
    // After conference 2's listing table, the read-it-now prompt appears
    // (`yesNo(1)`, default Yes).
    drain_until(
        &mut stream,
        b"Would you like to read it now \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[32m?\x1b[0m ",
    )
    .await;

    // Yes: the found message (1) renders, then the read sub-prompt opens
    // at the NEXT message — range `2+2` of the 2-message base (read-first
    // `start + 1` entry, slice B10).
    write_line(&mut stream, b"Y").await;
    let read = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&read, b"Welcome to Tier B."),
        "expected the found message body to be displayed, got {:?}",
        String::from_utf8_lossy(&read)
    );
    assert!(
        contains(&read, b"\x1b[0m 2+2 \x1b[32m )\x1b[0m>: "),
        "expected the read sub-prompt at range 2+2, got {:?}",
        String::from_utf8_lossy(&read)
    );

    // Quitting the sub-prompt returns to the menu, restored to conf 1.
    write_line(&mut stream, b"Q").await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "MS read-it-now must restore the home conference (1), got {:?}",
        String::from_utf8_lossy(&after)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn n_is_an_unknown_command_and_no_longer_scans_mail_over_telnet() {
    // Tier B B2: `N`'s mail-scan binding (a drift) is removed; the real
    // new-files scan lands in Tier D. Until then `N` is unrecognized.
    // Conference 2 carries unread mail — if `N` still scanned, it would
    // surface here.
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");
    std::fs::write(
        conf2_msgbase.join("0000001.json"),
        seeded_mail_json(2, 1, "Carol", "Tier B Greetings"),
    )
    .expect("seed conf2 message");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;

    assert!(
        contains(&out, b"Unknown command. Type G to log off.\r\n"),
        "N must now be an unknown command, got {:?}",
        String::from_utf8_lossy(&out)
    );
    // `N` must run no mail scan at all: no summary for the (empty)
    // current conference, and conference 2's mail stays unsurfaced.
    assert!(
        !contains(&out, b"No new mail."),
        "N must not scan the current conference, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !contains(&out, b"You have ") && !contains(&out, b"Tier B Greetings"),
        "N must not surface any mail, got {:?}",
        String::from_utf8_lossy(&out)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn e_uses_the_ruler_line_editor_with_a_msg_options_save_menu() {
    // Tier B / Fix 4: `E` replaces the minimal `.`-terminated editor
    // with AmiExpress's ruler + numbered-line editor
    // (`amiexpress/express.e:10148-10165`), ending on a blank line and
    // offering the `Msg. Options:` save menu (`:10375-10379`); `S`
    // saves. (The full-screen editor is still skipped.)
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"E sysop").await;
    drain_until(&mut stream, b"Subject: ").await;
    write_line(&mut stream, b"Greetings").await;
    drain_until(&mut stream, b"Private (y/N)? ").await;
    write_line(&mut stream, b"N").await;

    // The ruler intro, ruler line, and first numbered prompt appear
    // (75 chars/line; line 1 prompt is `1 > `).
    let intro = drain_until(&mut stream, b"\r\n1 > ").await;
    assert!(
        contains(
            &intro,
            b"   Enter your text. (Enter) alone to end. (75 chars/line)\r\n"
        ),
        "missing the ruler editor intro, got {:?}",
        String::from_utf8_lossy(&intro)
    );
    assert!(
        contains(&intro, b"   (|-------|-------|"),
        "missing the editor ruler, got {:?}",
        String::from_utf8_lossy(&intro)
    );

    // One body line, then a blank line ends input and shows the menu.
    write_line(&mut stream, b"Hello from the ruler editor.").await;
    write_line(&mut stream, b"").await;
    let menu = drain_until(&mut stream, b"Msg. Options:").await;
    assert!(
        contains(
            &menu,
            b"\x1b[32mMsg. Options: \x1b[33mA\x1b[36m,\x1b[33mC\x1b[36m,\x1b[33mD\x1b[36m,\x1b[33mE\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mS\x1b[36m,\x1b[33m? \x1b[0m>:"
        ),
        "missing the Msg. Options save menu, got {:?}",
        String::from_utf8_lossy(&menu)
    );

    // `S` saves; the message lands as #1 in the (empty) conference 1.
    write_line(&mut stream, b"S").await;
    let saved = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&saved, b"Message #1 saved."),
        "expected the save confirmation, got {:?}",
        String::from_utf8_lossy(&saved)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn bare_e_prompts_for_recipient_then_uses_the_ruler_editor() {
    // A bare `E` (no inline recipient) prompts `To: ` before the ruler
    // editor; the typed recipient is used (not rerouted to ALL).
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Bare `E` must prompt for the recipient.
    write_line(&mut stream, b"E").await;
    drain_until(&mut stream, b"To: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"Subject: ").await;
    write_line(&mut stream, b"Bare E").await;
    drain_until(&mut stream, b"Private (y/N)? ").await;
    write_line(&mut stream, b"N").await;
    drain_until(&mut stream, b"\r\n1 > ").await;
    write_line(&mut stream, b"Addressed to the sysop.").await;
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"Msg. Options:").await;
    write_line(&mut stream, b"S").await;
    let saved = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&saved, b"Message #1 saved."),
        "expected save confirmation for bare E, got {:?}",
        String::from_utf8_lossy(&saved)
    );

    // Read it back: it is addressed to the typed recipient, not ALL.
    write_line(&mut stream, b"R 1").await;
    let read = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&read, b"To     \x1b[33m:\x1b[0m ALL"),
        "bare E with a typed recipient must not reroute to ALL, got {:?}",
        String::from_utf8_lossy(&read)
    );
    assert!(
        contains(&read, b"Addressed to the sysop."),
        "expected the body on read-back, got {:?}",
        String::from_utf8_lossy(&read)
    );
    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session(&mut stream).await;
}

#[tokio::test]
async fn e_editor_help_list_continue_and_save_round_trip() {
    // Exercises the save-menu verbs `?` (help list), `L` (list lines),
    // `C` (continue editing) and `S` (save) in one editor session.
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"E sysop").await;
    drain_until(&mut stream, b"Subject: ").await;
    write_line(&mut stream, b"Verbs").await;
    drain_until(&mut stream, b"Private (y/N)? ").await;
    write_line(&mut stream, b"N").await;
    drain_until(&mut stream, b"   (|-------|-------|").await;

    // First line, then a blank line opens the save menu.
    write_line(&mut stream, b"Line one").await;
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"Msg. Options:").await;

    // `?` swaps in the verb help list (`C>ontinue` is unique to it).
    write_line(&mut stream, b"?").await;
    drain_until(&mut stream, b"ontinue").await;

    // `L` lists the line entered so far.
    write_line(&mut stream, b"L").await;
    drain_until(&mut stream, b"1 > Line one").await;

    // `C` resumes input; add a second line, blank, then save with `S`.
    write_line(&mut stream, b"C").await;
    write_line(&mut stream, b"Line two").await;
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"Msg. Options:").await;
    write_line(&mut stream, b"S").await;
    let saved = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&saved, b"Message #1 saved."),
        "expected save confirmation, got {:?}",
        String::from_utf8_lossy(&saved)
    );

    // Read it back: both lines (continue resumed input) are present.
    write_line(&mut stream, b"R 1").await;
    let read = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&read, b"Line one") && contains(&read, b"Line two"),
        "expected both saved lines on read-back, got {:?}",
        String::from_utf8_lossy(&read)
    );
    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session(&mut stream).await;
}

#[tokio::test]
async fn e_editor_abort_confirmation_abandons_the_message() {
    // `A` from the save menu confirms with `Abort message entry (y/n)?`
    // (`amiexpress/express.e:10568`); a `y` abandons the message — no
    // mail is saved and the `Message aborted.` notice appears.
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"E sysop").await;
    drain_until(&mut stream, b"Subject: ").await;
    write_line(&mut stream, b"Throwaway").await;
    drain_until(&mut stream, b"Private (y/N)? ").await;
    write_line(&mut stream, b"N").await;
    drain_until(&mut stream, b"   (|-------|-------|").await;
    write_line(&mut stream, b"Never mind.").await;
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"Msg. Options:").await;

    // `A` then `y` abandons.
    write_line(&mut stream, b"A").await;
    drain_until(&mut stream, b"Abort message entry (y/n)? ").await;
    write_line(&mut stream, b"y").await;
    let aborted = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&aborted, b"Message aborted."),
        "expected the abort notice, got {:?}",
        String::from_utf8_lossy(&aborted)
    );
    assert!(
        !contains(&aborted, b"saved."),
        "an aborted message must not be saved, got {:?}",
        String::from_utf8_lossy(&aborted)
    );

    end_session(&mut stream).await;
}

/// JSON payload for one public message addressed to the seeded sysop
/// (slot 1, handle "sysop"), in the [`FileMailStore`] on-disk format.
fn seeded_mail_json(conference: u32, msgbase: u32, from: &str, subject: &str) -> String {
    format!(
        r#"{{
            "conference_number": {conference},
            "msgbase_number": {msgbase},
            "number": 1,
            "visibility": "public",
            "from_name": "{from}",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "{subject}",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": 1,
            "body": "Welcome to Tier B.\n"
        }}"#
    )
}

/// Builds a `Runtime` with two conferences (both accessible to the
/// seeded sysop), backing each conference's message base with a
/// file-backed store rooted at the supplied temp directories, then
/// binds a [`TelnetListener`] and spawns its accept loop.
async fn spawn_two_conference_listener(
    bbs_path: std::path::PathBuf,
    conf1_msgbase: &std::path::Path,
    conf2_msgbase: &std::path::Path,
) -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![
        Conference::new(
            1,
            "One".to_string(),
            vec![MessageBase::new(1, 1, "general".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Two".to_string(),
            vec![MessageBase::new(2, 1, "general".to_string())],
        )
        .expect("valid conference"),
    ];

    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &conferences);
    // This smoke exercises the `MS` command, which scans every accessible
    // conference regardless of the per-conference `mail_scan` flag. Opt
    // the sysop out of the *logon* conference scan (which honours the
    // flag) so login reaches the menu without a read-it-now detour; the
    // logon scan has its own dedicated smoke.
    for membership in sysop.memberships_mut() {
        membership.set_scan_flag(ScanFlag::MailScan, false);
    }
    let user_repo: SharedUserRepo =
        Arc::new(InMemoryUserRepository::new(vec![sysop])) as Arc<dyn UserRepository + Send + Sync>;
    let hasher_shared: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;

    let mut registry = InMemoryMailStores::new();
    registry.register(
        MessageBaseRef::new(1, 1),
        Box::new(
            FileMailStore::open(conf1_msgbase.to_path_buf(), MessageBaseRef::new(1, 1))
                .expect("open conf1 store"),
        ),
    );
    registry.register(
        MessageBaseRef::new(2, 1),
        Box::new(
            FileMailStore::open(conf2_msgbase.to_path_buf(), MessageBaseRef::new(2, 1))
                .expect("open conf2 store"),
        ),
    );
    let mail_stores: SharedMailStores = Arc::new(registry) as Arc<dyn MailStores + Send + Sync>;
    let conferences_handle: SharedConferences = Arc::new(conferences);

    let config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        bbs_path,
        ..Config::default()
    };
    let runtime = bootstrap::build_runtime(
        &config,
        user_repo,
        hasher_shared,
        caller_log,
        conferences_handle,
        mail_stores,
        Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
    );

    let listener = TelnetListener::bind("127.0.0.1:0", runtime)
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local_addr");
    let listener = Arc::new(listener);
    let task_listener = listener.clone();
    tokio::spawn(async move { task_listener.run().await });
    addr
}

async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"mins. left): ").await;
    stream
}

async fn end_session(stream: &mut TcpStream) {
    write_line(stream, b"G").await;
    drain_until(stream, b"Goodbye").await;
}

async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

async fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let n = match tokio::time::timeout(DRAIN_DEADLINE, stream.read(&mut chunk)).await {
            Ok(Ok(n)) => n,
            Ok(Err(_)) | Err(_) => 0,
        };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
        if contains(&out, needle) {
            break;
        }
    }
    assert!(
        contains(&out, needle),
        "needle {:?} not found within {DRAIN_DEADLINE:?}; got {:?}",
        std::str::from_utf8(needle).unwrap_or("<bin>"),
        String::from_utf8_lossy(&out),
    );
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
