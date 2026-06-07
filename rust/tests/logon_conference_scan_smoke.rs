//! Login-fixes (Slice L1) in-process smoke: the logon conference scan.
//!
//! Boots a [`TelnetListener`] in-process (the `tierb_mail_scan_smoke.rs`
//! shape) with two conferences, each holding one unread message
//! addressed to the seeded sysop, but with the per-conference
//! `mail_scan` flag set only on the **second** conference. After signing
//! in, the driver runs the logon conference scan (legacy `confScan`,
//! `amiexpress/express.e:28066`) *before* the auto-rejoin announcement:
//!
//!   * the `Scanning conferences for mail...` header renders;
//!   * conference 2 ("Beta", flagged) is scanned — its banner, the
//!     listing table carrying its message, and the legacy read-it-now
//!     offer all appear before the menu;
//!   * conference 1 ("Alpha", `mail_scan` cleared — the legacy
//!     `checkMailConfScan` gate) is skipped.
//!
//! The skipped conference is deliberately the **lower-numbered** one:
//! the scan walks conferences ascending, so if the `mail_scan` filter
//! were broken, "Alpha" would be scanned *first* and its banner + mail +
//! read-it-now would land in the drained buffer **before** "Beta"'s
//! offer. Asserting "Alpha" is entirely absent from that buffer
//! therefore genuinely proves the skip (the verifier confirmed a
//! skipped-second assertion passes vacuously, because the first
//! conference's read-it-now blocks before the second renders).
//!
//! This proves the L1 capability — a multi-conference logon mail scan
//! that honours the `mail_scan` flag the `CF` editor (Slice C5) sets —
//! is reachable through the same composition root and telnet adapter the
//! binary uses, reusing the `MS` rendering and read-it-now flow.

use std::sync::Arc;
use std::time::Duration;

use nextexpress::adapters::file_mail_store::FileMailStore;
use nextexpress::adapters::in_memory_caller_log::InMemoryCallerLog;
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

const READ_IT_NOW: &[u8] = b"read it now ";
const SCAN_HEADER: &[u8] = b"Scanning conferences for mail";
const FLAGGED_SUBJECT: &[u8] = b"Beta flag subject";
/// The skipped conference's name. Its complete absence from the scan
/// buffer is the skip proof (see the module doc).
const SKIPPED_CONFERENCE: &[u8] = b"Alpha";

#[tokio::test]
async fn logon_scan_surfaces_flagged_conference_and_skips_unflagged_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let conf1_msgbase = dir.path().join("conf1_msgbase");
    let conf2_msgbase = dir.path().join("conf2_msgbase");
    std::fs::create_dir_all(&conf1_msgbase).expect("create conf1 msgbase");
    std::fs::create_dir_all(&conf2_msgbase).expect("create conf2 msgbase");
    // Both conferences have one unread message addressed to the sysop,
    // but only conference 2 ("Beta") keeps its `mail_scan` flag (set
    // below). Conference 1 ("Alpha") is the skipped one.
    std::fs::write(
        conf1_msgbase.join("0000001.json"),
        seeded_mail_json(1, 1, "Alpha skip subject"),
    )
    .expect("seed conf1 message");
    std::fs::write(
        conf2_msgbase.join("0000001.json"),
        seeded_mail_json(2, 1, "Beta flag subject"),
    )
    .expect("seed conf2 message");

    let addr =
        spawn_two_conference_listener(dir.path().to_path_buf(), &conf1_msgbase, &conf2_msgbase)
            .await;
    let mut stream = TcpStream::connect(&addr).await.expect("connect");

    // Sign in. The logon conference scan runs before the menu, so the
    // sign-in stops at the read-it-now offer rather than the menu prompt.
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;

    let scan = drain_until(&mut stream, READ_IT_NOW).await;
    assert!(
        contains(&scan, SCAN_HEADER),
        "the logon conference scan header should render, got {:?}",
        String::from_utf8_lossy(&scan)
    );
    assert!(
        contains(&scan, FLAGGED_SUBJECT),
        "conference 2's mail (flagged) should appear in the logon scan listing, got {:?}",
        String::from_utf8_lossy(&scan)
    );
    // The skipped conference is lower-numbered, so a broken filter would
    // have scanned it first and its name (in the banner) would be in this
    // buffer ahead of "Beta"'s read-it-now. Its absence proves the skip.
    assert!(
        !contains(&scan, SKIPPED_CONFERENCE),
        "conference 1 (mail_scan cleared) must be skipped — its banner must not appear, got {:?}",
        String::from_utf8_lossy(&scan)
    );

    // Decline the read-it-now; the scan finishes and the deferred
    // auto-rejoin announcement places the caller in the home conference
    // before the menu prompt.
    write_line(&mut stream, b"n").await;
    let menu = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&menu, b"Auto-ReJoined"),
        "the auto-rejoin announcement should follow the logon scan, got {:?}",
        String::from_utf8_lossy(&menu)
    );
    assert!(
        contains(&menu, b"[\x1b[36m1\x1b[34m:\x1b[36mAlpha\x1b[0m]"),
        "the menu prompt should show the home conference, got {:?}",
        String::from_utf8_lossy(&menu)
    );

    end_session(&mut stream).await;
}

/// JSON payload for one public message addressed to the seeded sysop
/// (slot 1, handle "sysop"), in the [`FileMailStore`] on-disk format.
fn seeded_mail_json(conference: u32, msgbase: u32, subject: &str) -> String {
    format!(
        r#"{{
            "conference_number": {conference},
            "msgbase_number": {msgbase},
            "number": 1,
            "visibility": "public",
            "from_name": "Carol",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "{subject}",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": 1,
            "body": "Logon scan reached the body.\n"
        }}"#
    )
}

/// Builds a `Runtime` with two conferences (both accessible to the
/// seeded sysop) backed by file stores at the supplied temp
/// directories, clears the `mail_scan` flag on conference 1, then binds
/// a [`TelnetListener`] and spawns its accept loop.
async fn spawn_two_conference_listener(
    bbs_path: std::path::PathBuf,
    conf1_msgbase: &std::path::Path,
    conf2_msgbase: &std::path::Path,
) -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![
        Conference::new(
            1,
            "Alpha".to_string(),
            vec![MessageBase::new(1, 1, "general".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Beta".to_string(),
            vec![MessageBase::new(2, 1, "general".to_string())],
        )
        .expect("valid conference"),
    ];

    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &conferences);
    // Clear conference 1's `mail_scan` flag (the legacy `checkMailConfScan`
    // gate / the `CF` editor's effect) so the logon scan skips it while
    // still scanning conference 2.
    for membership in sysop.memberships_mut() {
        if membership.conference_number() == 1 {
            membership.set_scan_flag(ScanFlag::MailScan, false);
        }
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
