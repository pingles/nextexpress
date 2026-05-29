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
use nextexpress::domain::conference::{Conference, MessageBase, MessageBaseRef};
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
    let out = drain_until(&mut stream, b"mins. left): ").await;

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

    // Restore invariant: the menu prompt that follows `MS` still shows
    // conference 1 ("One") — the scan never moved the session.
    assert!(
        contains(&out, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "MS must leave the session in conference 1, got {:?}",
        String::from_utf8_lossy(&out)
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

async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
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
