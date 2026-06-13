//! Tier D `F` (`NextScan` file listings) in-process integration tests.
//!
//! Each scenario boots a [`TelnetListener`] in-process, signs in as
//! the seeded sysop, drives the `NextScan` lister over a real telnet
//! client, and asserts the captured wire bytes (parity target: the
//! `AquaScan` door with `NextScan` branding —
//! `comparison/evidence-tierD/live-observations.md`; cleanest capture
//! `comparison/transcripts/ae_tierd_aquascan3.txt`). The expected
//! literals are restated here independently of the production
//! constants on purpose: the smoke guards them against drift.

use std::sync::Arc;
use std::time::Duration;

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
    SharedCallerLog, SharedConferences, SharedFileRepo, SharedHasher, SharedMailStores,
    SharedUserRepo,
};
use nextexpress::bootstrap;
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{Conference, MessageBase};
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

/// The `NextScan` listing banner (branding per `designs/NEXTSCAN.md` §7).
const LISTING_BANNER: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m\r\n";

/// The `More?` pager prompt (`ae_tierd_aquascan3.txt:158`).
const MORE_PROMPT: &[u8] =
    b"\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m ";

#[tokio::test]
async fn f_1_pages_the_seeded_corpus_and_q_quits() {
    // ae_tierd_aquascan3.txt S4: banner, scan header, framed rows;
    // the first More? lands exactly after the 02-03-26 separator
    // block (the captured 29-line page); Q echoes Quit and exits
    // through the two-reset tail.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 1").await;
    let page = drain_until(&mut stream, MORE_PROMPT).await;

    let mut expected_head = b"F 1\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(LISTING_BANNER);
    expected_head.extend_from_slice(b"\r\nScanning dir 1 from top... Ok!\r\n\r\n");
    assert!(
        page.starts_with(&expected_head),
        "F 1 must open with the NextScan preamble, got {:?}",
        String::from_utf8_lossy(&page[..expected_head.len().min(page.len())]),
    );
    assert!(
        contains(
            &page,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34mP\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the\r\n",
        ),
        "first framed row missing: {:?}",
        String::from_utf8_lossy(&page),
    );
    // The captured page-1 boundary: the 02-03-26 separator block's
    // closing blank, then More? — mid-frame, before File #4's header.
    let mut expected_tail = b" 02-03-26\r\n\x1b[0m\r\n".to_vec();
    expected_tail.extend_from_slice(MORE_PROMPT);
    assert!(
        page.ends_with(&expected_tail),
        "page 1 must pause after the 02-03-26 separator block, got tail {:?}",
        String::from_utf8_lossy(&page[page.len().saturating_sub(120)..]),
    );

    // D2b re-pin: `Q` is a single bare keypress, no Enter
    // (ae_tierd_aquascan3.txt:321 — the capture harness sent the
    // lone byte); the echoed Quit and tail bytes are unchanged.
    write_key(&mut stream, b"Q").await;
    let quit = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        quit.starts_with(b"Quit\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "Q must echo Quit into the two-reset tail, got {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn f_2_butt_joins_same_date_files_and_post_end_n_is_erased_by_q() {
    // ae_tierd_aquascan3.txt S2 + :158-163: the same-date pair
    // butt-joins (no separator), the footer is followed by the
    // post-End More?, and a held `n` is erased by the next verb.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 2").await;
    let listing = drain_until(&mut stream, MORE_PROMPT).await;
    assert!(
        contains(
            &listing,
            b"Greets to everyone on node 1.\r\n\x1b[0m\x1b[34m[\x1b[0m File #3 ",
        ),
        "same-date TOOLPACK must butt-join after MYDEMO's continuation: {:?}",
        String::from_utf8_lossy(&listing),
    );
    let mut footer_then_more =
        b"\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m\r\n".to_vec();
    footer_then_more.extend_from_slice(MORE_PROMPT);
    assert!(
        listing.ends_with(&footer_then_more),
        "the post-End More? must follow the footer directly: {:?}",
        String::from_utf8_lossy(&listing[listing.len().saturating_sub(160)..]),
    );

    // D2b re-pin: bare keys, no terminators (ae_tierd_aquascan4.txt
    // U1 :133 — `n` echoes on its own keypress and holds; a
    // terminated `n\r\n` would now quit via probe P1 instead).
    write_key(&mut stream, b"n").await;
    let held = drain_until(&mut stream, b"n").await;
    assert!(
        held.ends_with(b"n"),
        "lone n echoes and holds, got {:?}",
        String::from_utf8_lossy(&held),
    );
    write_key(&mut stream, b"Q").await;
    let quit = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        quit.starts_with(b"\x08 \x08Quit\r\n"),
        "the next verb erases the held n before running: {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn bare_f_opens_the_directories_prompt_and_enter_aborts() {
    // ae_tierd_aquascan3.txt S3: the door's own Directories prompt
    // with the live (1-2) range; Enter aborts silently with a single
    // reset.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F").await;
    let prompt = drain_until(&mut stream, b"=None ?\x1b[0m ").await;
    assert!(
        contains(
            &prompt,
            b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, ",
        ),
        "the Directories prompt must flex to (1-2): {:?}",
        String::from_utf8_lossy(&prompt),
    );
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session(&mut stream).await;
}

#[tokio::test]
async fn f_99_reports_the_highest_directory() {
    // ae_tierd_aquascan.txt A7 (:330-342), max flexed to 2.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 99").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"The highest directory number is 2!\r\n"),
        "F 99 must report the highest dir: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn f_h_reports_nothing_held() {
    // ae_tierd_aquascan3.txt S9 (:675-687).
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F H").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning HOLD dir from top... Nothing found!\r\n"),
        "F H must report the empty hold dir: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn f_in_an_unseeded_conference_reports_nothing_found() {
    // The demo catalogue seeds only the landing conference; other
    // conferences carry one empty area (ae_tierd_aquascan.txt E2's
    // Nothing-found shape).
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"J 2").await;
    drain_until(&mut stream, b"mins. left): ").await;
    write_line(&mut stream, b"F 1").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning dir 1 from top... Nothing found!\r\n"),
        "the unseeded conference must list nothing: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn utf8_gate_every_session_byte_decodes() {
    // Encoding policy (AGENTS.md): the wire is valid UTF-8. Drive the
    // listing body (wave art) and the F ? help (©) and assert the
    // entire received stream decodes. The More?/flag prompt constants
    // are pinned in wire.rs unit tests and join this gate once the
    // hotkey pager lands; the login banner is gated by its own slice.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;
    let mut all = Vec::new();
    // sign_in_seeded_sysop already drains through "mins. left): ";
    // the F surfaces below are what this gate owns.
    write_line(&mut stream, b"F ?").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"F A NS").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    assert!(
        std::str::from_utf8(&all).is_ok(),
        "session stream contains non-UTF-8 bytes: {:?}",
        String::from_utf8_lossy(&all)
    );
    end_session(&mut stream).await;
}

/// Boots an in-process listener whose file catalogue is the seeded
/// demo corpus (landing conference 1: areas 1-2; conference 2: one
/// empty area) — the same wiring `bootstrap::run` performs.
async fn spawn_listener_with_demo_files() -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![
        Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Other".to_string(),
            vec![MessageBase::new(2, 1, "main".to_string())],
        )
        .expect("valid conference"),
    ];

    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &conferences);
    let user_repo: SharedUserRepo =
        Arc::new(InMemoryUserRepository::new(vec![sysop])) as Arc<dyn UserRepository + Send + Sync>;
    let hasher_shared: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;
    let mail_stores: SharedMailStores =
        Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>;
    let (areas, files) = seed::demo_file_catalogue(&conferences);
    let file_repo: SharedFileRepo = Arc::new(InMemoryFileRepository::new(areas, files));
    let conferences_handle: SharedConferences = Arc::new(conferences);

    let config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        bbs_path: std::env::temp_dir(),
        ..Config::default()
    };
    let runtime = bootstrap::build_runtime(
        &config,
        user_repo,
        hasher_shared,
        caller_log,
        conferences_handle,
        mail_stores,
        file_repo,
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

/// Sends one bare pager hotkey — no line terminator (slice D2b: the
/// `More?` prompt acts per keypress, `ae_tierd_aquascan3.txt:321`,
/// `ae_tierd_aquascan4.txt` U1; a terminated `n\r\n` would now mean
/// held-n + Enter = the probe-P1 quit).
async fn write_key(stream: &mut TcpStream, key: &[u8]) {
    stream.write_all(key).await.expect("write key");
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
        String::from_utf8_lossy(needle),
        String::from_utf8_lossy(&out),
    );
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
