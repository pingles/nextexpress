//! Tier B (Slice B4) in-process smoke: the `R` read sub-prompt
//! scaffolding.
//!
//! Boots a [`TelnetListener`] in-process (the `tierb_mail_scan_smoke.rs`
//! shape) with one conference ("One") whose message base carries two
//! public messages addressed to the seeded sysop. After signing in the
//! sysop types `R 1`:
//!
//!   * message 1 is displayed (legacy header block), then the legacy
//!     `readMSG` sub-prompt appears with the runtime range `1+2`
//!     (`amiexpress/express.e:12016-12021`);
//!   * pressing `<CR>` (an empty line) advances to message 2, which is
//!     displayed and followed by the sub-prompt re-rendered at range
//!     `2+2`;
//!   * pressing `Q` returns to the main conference menu prompt.
//!
//! This pins the verbatim sub-prompt wire bytes and the `<CR>`-advance /
//! `Q`-quit navigation that Slice B4 introduces. Options other than
//! `<CR>` / `Q` (A/F/R/L/D/M/EH/?/??) land in B5 and are not exercised
//! here.

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
async fn r_enters_sub_prompt_then_cr_advances_then_q_quits_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // `R 1` reads message 1 and drops into the sub-prompt.
    write_line(&mut stream, b"R 1").await;
    let first = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&first, b"First Subject"),
        "R 1 must display message 1, got {:?}",
        String::from_utf8_lossy(&first)
    );
    assert!(
        contains(&first, &sub_prompt(b"1+2")),
        "missing the legacy sub-prompt at range 1+2, got {:?}",
        String::from_utf8_lossy(&first)
    );

    // `<CR>` (an empty line) advances to message 2, which is displayed
    // and followed by the sub-prompt re-rendered at range 2+2.
    write_line(&mut stream, b"").await;
    let second = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&second, b"Second Subject"),
        "<CR> must advance to and display message 2, got {:?}",
        String::from_utf8_lossy(&second)
    );
    assert!(
        contains(&second, &sub_prompt(b"2+2")),
        "missing the sub-prompt re-rendered at range 2+2, got {:?}",
        String::from_utf8_lossy(&second)
    );

    // `Q` returns to the main conference menu prompt.
    write_line(&mut stream, b"Q").await;
    let back = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&back, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "Q must return to the conference 1 menu prompt, got {:?}",
        String::from_utf8_lossy(&back)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn cr_past_the_last_message_returns_to_the_menu() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Read the last message (2 of 2): the sub-prompt range is `2+2`.
    write_line(&mut stream, b"R 2").await;
    let at_last = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&at_last, &sub_prompt(b"2+2")),
        "expected the sub-prompt at the last message, got {:?}",
        String::from_utf8_lossy(&at_last)
    );

    // `<CR>` past the highest existing message returns to the menu —
    // the legacy out-of-range -> `QUIT` clamp (`express.e:12012`). It
    // must NOT probe a non-existent message 3 or re-render the
    // sub-prompt.
    write_line(&mut stream, b"").await;
    let back = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&back, b"Message not found"),
        "advancing past the last message must not probe a missing message, got {:?}",
        String::from_utf8_lossy(&back)
    );
    assert!(
        contains(&back, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "expected return to the conference 1 menu prompt, got {:?}",
        String::from_utf8_lossy(&back)
    );

    end_session(&mut stream).await;
}

/// Seeds a two-message file mail base: two unread public messages
/// addressed to the seeded sysop (slot 1), numbered 1 and 2.
fn seed_two_message_base(msgbase: &std::path::Path) {
    std::fs::create_dir_all(msgbase).expect("create msgbase");
    std::fs::write(
        msgbase.join("0000001.json"),
        seeded_mail_json(1, "Carol", "First Subject", "First message body."),
    )
    .expect("seed message 1");
    std::fs::write(
        msgbase.join("0000002.json"),
        seeded_mail_json(2, "Dave", "Second Subject", "Second message body."),
    )
    .expect("seed message 2");
}

/// The verbatim ungated `readMSG` sub-prompt skeleton
/// (`amiexpress/express.e:12016-12021`) with `range` substituted into
/// the `( <range> )` slot. ANSI escapes are emitted literally; note the
/// doubled `ESC[36m` seam where `A` joins `F` (the skipped `D` / `M`
/// fragments would otherwise sit between them).
fn sub_prompt(range: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(
        b"\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m\x1b[36m,\x1b[33mF\x1b[36m,\x1b[33mR\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mQ\x1b[36m,\x1b[33m?\x1b[36m,\x1b[33m??\x1b[36m,\x1b[32m<\x1b[33mCR\x1b[32m> \x1b[32m(\x1b[0m ",
    );
    v.extend_from_slice(range);
    v.extend_from_slice(b" \x1b[32m )\x1b[0m>: ");
    v
}

/// JSON payload for one public message addressed to the seeded sysop
/// (slot 1, handle "sysop"), in the [`FileMailStore`] on-disk format.
fn seeded_mail_json(number: u32, from: &str, subject: &str, body: &str) -> String {
    format!(
        r#"{{
            "conference_number": 1,
            "msgbase_number": 1,
            "number": {number},
            "visibility": "public",
            "from_name": "{from}",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "{subject}",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": 1,
            "body": "{body}"
        }}"#
    )
}

/// Builds a `Runtime` with one conference accessible to the seeded
/// sysop, backing its message base with a file-backed store rooted at
/// the supplied temp directory, then binds a [`TelnetListener`] and
/// spawns its accept loop.
async fn spawn_one_conference_listener(
    bbs_path: std::path::PathBuf,
    msgbase: &std::path::Path,
) -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![Conference::new(
        1,
        "One".to_string(),
        vec![MessageBase::new(1, 1, "general".to_string())],
    )
    .expect("valid conference")];

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
            FileMailStore::open(msgbase.to_path_buf(), MessageBaseRef::new(1, 1))
                .expect("open conf1 store"),
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
