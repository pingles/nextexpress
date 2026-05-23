//! Tier A "quick wins" in-process integration tests.
//!
//! Each scenario boots a [`TelnetListener`] in-process on a tokio
//! task, opens a real telnet client to the bound address, drives one
//! Tier A quickwin, and asserts the verbatim `AmiExpress` wire text.
//! Going in-process (rather than spawning the `nextexpress` binary)
//! cuts the per-test cost from a full process startup to a single
//! `Runtime` build, while still exercising the same composition root
//! and the same telnet adapter the binary uses.

use std::sync::Arc;
use std::time::Duration;

use nextexpress::adapters::in_memory_caller_log::InMemoryCallerLog;
use nextexpress::adapters::in_memory_mail_stores::InMemoryMailStores;
use nextexpress::adapters::in_memory_user_repository::InMemoryUserRepository;
use nextexpress::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use nextexpress::adapters::telnet_listener::TelnetListener;
use nextexpress::app::config::Config;
use nextexpress::app::mail_stores::MailStores;
use nextexpress::app::runtime::Runtime;
use nextexpress::app::seed;
use nextexpress::app::services::{
    SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedUserRepo,
};
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{Conference, MessageBase};
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Per-`drain_until` deadline. A real BBS prompt arrives in
/// milliseconds; two seconds is generous enough to forgive a slow CI
/// runner without making genuine failures wait forever.
const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

#[tokio::test]
async fn t_command_renders_legacy_it_is_format() {
    // Slice A1 — `T` (current date/time). Mirrors
    // `internalCommandT()` at `amiexpress/express.e:25622-25644`.
    // The wall-clock fields are wall-clock-dependent so the smoke
    // pins the surrounding literal: `It is ` prefix, CRLF terminator,
    // and a `MM-DD-YY HH:MM:SS` structure.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"T").await;
    let post_t = drain_until(&mut stream, b"Command: ").await;
    assert!(
        contains(&post_t, b"It is "),
        "expected legacy `It is ` prefix after T, got {:?}",
        String::from_utf8_lossy(&post_t)
    );
    assert_time_line_shape(&post_t);

    end_session(&mut stream).await;
}

/// Builds a `Runtime` with an in-memory user repo, the seeded sysop,
/// a single `Main` conference, an empty mail store, and an in-memory
/// caller log, then binds a [`TelnetListener`] on an ephemeral port
/// and spawns its accept loop. Returns the address the listener is
/// bound to.
async fn spawn_listener_with_seeded_sysop() -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid conference")];

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
    let runtime = Runtime::from_config(
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

/// Connects to `addr`, walks the standard auth handshake as the
/// seeded `sysop` / `sysop`, and returns the open stream sitting at
/// the menu's `Command: ` prompt.
async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"Command: ").await;
    stream
}

/// Sends `G` to log off and drains until the listener emits the
/// `Goodbye` line, mirroring the close-of-session pattern shared by
/// every quickwin scenario.
async fn end_session(stream: &mut TcpStream) {
    write_line(stream, b"G").await;
    drain_until(stream, b"Goodbye").await;
}

/// Structural check on the rendered `T` line: between `It is ` and
/// the CRLF the format is `MM-DD-YY HH:MM:SS` — three hyphen-separated
/// date parts and three colon-separated time parts. Anything else
/// (e.g. a stub literal or a swapped separator) fails the parse.
fn assert_time_line_shape(post_t: &[u8]) {
    let it_is_idx = find(post_t, b"It is ").expect("`It is ` prefix not found");
    let tail = &post_t[it_is_idx + b"It is ".len()..];
    let line_end = tail
        .windows(2)
        .position(|w| w == b"\r\n")
        .expect("missing CRLF terminator after time line");
    let line = std::str::from_utf8(&tail[..line_end]).expect("non-utf8 time line");
    let (date, clock) = line.split_once(' ').unwrap_or_else(|| {
        panic!("expected `<date> <time>`, got {line:?}");
    });
    let date_parts: Vec<&str> = date.split('-').collect();
    let clock_parts: Vec<&str> = clock.split(':').collect();
    assert!(
        date_parts.len() == 3 && clock_parts.len() == 3,
        "expected `MM-DD-YY HH:MM:SS` after `It is `, got {line:?}",
    );
}

async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

/// Reads bytes from `stream` until `needle` appears in the
/// accumulated buffer, EOF arrives, or [`DRAIN_DEADLINE`] elapses.
/// The deadline matters: a broken server would otherwise leave us
/// blocked on `read` forever.
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
