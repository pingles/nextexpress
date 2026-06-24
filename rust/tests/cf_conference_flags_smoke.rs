//! Tier C (Slice C5) in-process smoke: the `CF` conference-flags editor.
//!
//! Boots a [`TelnetListener`] in-process (the `tierb_mail_scan_smoke.rs`
//! shape) with two conferences the seeded sysop can access, then drives
//! the `CF` editor over telnet:
//!
//!   * the M/A/F/Z listing shows both conferences with mail- and
//!     file-scan set (the C5 default for a granted membership);
//!   * `M` then `1` toggles conference 1's mail-scan off — its `M` cell
//!     clears on the redraw while file-scan stays set;
//!   * `M` then `*` toggles **every** conference's mail-scan (design D1:
//!     the legacy advertises `*` but no-ops it — `NextExpress` honours the
//!     advertised toggle-all), flipping conference 1 back on and
//!     conference 2 off;
//!   * a non-M/A/F/Z key (`Q`) leaves the editor for the menu.
//!
//! This proves the headline C5 capability is reachable through the same
//! composition root and telnet adapter the binary uses.

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
    SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedUserRepo,
};
use nextexpress::bootstrap;
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{Conference, MessageBase};
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

const MASK_PROMPT: &[u8] = b"[Z]oom >: ";
const EXPR_PROMPT: &[u8] = b"'+' All on >: ";
const HEADER: &[u8] = b"\x1b[32m        M A F Z Conference";

#[tokio::test]
async fn cf_lists_toggles_one_and_star_toggles_all_over_telnet() {
    let addr = spawn_two_conference_listener().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // --- initial listing: both conferences mail+file scan on (default) ---
    write_line(&mut stream, b"CF").await;
    let listing = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&listing, HEADER),
        "missing the CF column header, got {:?}",
        String::from_utf8_lossy(&listing)
    );
    assert!(
        contains(&listing, b"    1\x1b[34m] \x1b[36m*   *   \x1b[0mOne"),
        "conference 1 should show mail+file scan set, got {:?}",
        String::from_utf8_lossy(&listing)
    );
    assert!(
        contains(&listing, b"    2\x1b[34m] \x1b[36m*   *   \x1b[0mTwo"),
        "conference 2 should show mail+file scan set, got {:?}",
        String::from_utf8_lossy(&listing)
    );

    // --- M then `1`: toggle conference 1's mail-scan off ---
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, EXPR_PROMPT).await;
    write_line(&mut stream, b"1").await;
    let after_one = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&after_one, b"    1\x1b[34m] \x1b[36m    *   \x1b[0mOne"),
        "conference 1's mail cell should clear (file stays set), got {:?}",
        String::from_utf8_lossy(&after_one)
    );
    assert!(
        contains(&after_one, b"    2\x1b[34m] \x1b[36m*   *   \x1b[0mTwo"),
        "conference 2 must be unchanged by a `1` edit, got {:?}",
        String::from_utf8_lossy(&after_one)
    );

    // --- M then `*`: toggle-all mail-scan (D1) ---
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, EXPR_PROMPT).await;
    write_line(&mut stream, b"*").await;
    let after_star = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&after_star, b"    1\x1b[34m] \x1b[36m*   *   \x1b[0mOne"),
        "`*` must toggle conference 1's mail cell back on, got {:?}",
        String::from_utf8_lossy(&after_star)
    );
    assert!(
        contains(&after_star, b"    2\x1b[34m] \x1b[36m    *   \x1b[0mTwo"),
        "`*` must toggle conference 2's mail cell off (legacy no-ops this), got {:?}",
        String::from_utf8_lossy(&after_star)
    );

    // --- a non-M/A/F/Z key leaves the editor for the menu ---
    write_line(&mut stream, b"Q").await;
    let menu = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&menu, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "CF must return to the menu in conference 1, got {:?}",
        String::from_utf8_lossy(&menu)
    );

    end_session(&mut stream).await;
}

/// Builds a `Runtime` with two conferences (both accessible to the
/// seeded sysop) and empty in-memory mail stores, then binds a
/// [`TelnetListener`] and spawns its accept loop.
async fn spawn_two_conference_listener() -> std::net::SocketAddr {
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
    let mail_stores: SharedMailStores =
        Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>;
    let conferences_handle: SharedConferences = Arc::new(conferences);

    let config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        ..Config::default()
    };
    let runtime = bootstrap::build_runtime(
        &config,
        bootstrap::RuntimeAdapters {
            user_repo,
            hasher: hasher_shared,
            caller_log,
            conferences: conferences_handle,
            mail_stores,
            file_repo: Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
        },
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
