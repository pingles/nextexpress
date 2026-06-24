#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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
    SharedCallerLog, SharedConferences, SharedFileRepo, SharedHasher, SharedMailStores,
    SharedUserRepo,
};
use nextexpress::bootstrap::{self, RuntimeAdapters};
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{Conference, MessageBaseRef};
use nextexpress::domain::messaging::mail_store::MailStore;
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Per-read deadline for in-process telnet smokes.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

/// Runtime fixture used by in-process telnet integration tests.
pub struct TestRuntime {
    /// BBS root used by file-screen and file-mail adapters.
    pub bbs_path: PathBuf,
    /// Conference catalogue installed in the runtime.
    pub conferences: Vec<Conference>,
    /// Mail stores installed in the runtime.
    pub mail_stores: SharedMailStores,
    /// File catalogue installed in the runtime.
    pub file_repo: SharedFileRepo,
}

impl TestRuntime {
    /// Builds an in-process fixture rooted at `bbs_path`.
    pub fn new(
        bbs_path: PathBuf,
        conferences: Vec<Conference>,
        mail_stores: SharedMailStores,
        file_repo: SharedFileRepo,
    ) -> Self {
        Self {
            bbs_path,
            conferences,
            mail_stores,
            file_repo,
        }
    }
}

/// Returns an empty in-memory mail-store registry.
pub fn empty_mail_stores() -> SharedMailStores {
    Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>
}

/// Opens one file-backed mail store per configured message base.
///
/// The directory layout matches production bootstrap:
/// `ConfNN/MsgBase` for base 1 and `ConfNN/MsgBase<M>` for higher bases.
pub fn file_mail_stores(bbs_path: &Path, conferences: &[Conference]) -> SharedMailStores {
    let mut registry = InMemoryMailStores::new();
    for conf in conferences {
        for msgbase in conf.msgbases() {
            let dir_name = if msgbase.number() == 1 {
                "MsgBase".to_string()
            } else {
                format!("MsgBase{}", msgbase.number())
            };
            let dir = bbs_path
                .join(format!("Conf{:02}", conf.number()))
                .join(dir_name);
            let coord = MessageBaseRef::new(conf.number(), msgbase.number());
            let store = FileMailStore::open(dir, coord).expect("open file mail store");
            let boxed: Box<dyn MailStore + Send> = Box::new(store);
            registry.register(coord, boxed);
        }
    }
    Arc::new(registry) as Arc<dyn MailStores + Send + Sync>
}

/// Returns an empty in-memory file catalogue.
pub fn empty_file_repo() -> SharedFileRepo {
    Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new()))
}

/// Binds a [`TelnetListener`] for a seeded-sysop fixture and returns
/// the bound address.
pub async fn spawn_seeded_sysop(fixture: TestRuntime) -> SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &fixture.conferences);

    let user_repo: SharedUserRepo =
        Arc::new(InMemoryUserRepository::new(vec![sysop])) as Arc<dyn UserRepository + Send + Sync>;
    let hasher: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;
    let conferences: SharedConferences = Arc::new(fixture.conferences);

    let config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        bbs_path: fixture.bbs_path,
        ..Config::default()
    };
    let runtime = bootstrap::build_runtime(
        &config,
        RuntimeAdapters {
            user_repo,
            hasher,
            caller_log,
            conferences,
            mail_stores: fixture.mail_stores,
            file_repo: fixture.file_repo,
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

/// Connects as the seeded `sysop` user and returns the stream at the
/// menu prompt.
pub async fn sign_in_seeded_sysop(addr: &SocketAddr) -> TcpStream {
    let (stream, _) = sign_in_seeded_sysop_capturing_menu(addr).await;
    stream
}

/// Connects as the seeded `sysop` user and returns both the stream at
/// the menu prompt and the bytes consumed from password submission
/// through that prompt.
pub async fn sign_in_seeded_sysop_capturing_menu(addr: &SocketAddr) -> (TcpStream, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    (stream, capture)
}

/// Connects as the seeded `sysop`, declines the logon scan read-now
/// offer, and returns the stream at the menu prompt plus the scan
/// capture.
pub async fn sign_in_seeded_sysop_declining_logon_scan(addr: &SocketAddr) -> (TcpStream, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;
    let capture = drain_until(&mut stream, b"read it now ").await;
    write_line(&mut stream, b"n").await;
    drain_until(&mut stream, b"mins. left): ").await;
    (stream, capture)
}

/// Sends `G` and waits for the goodbye line.
pub async fn end_session(stream: &mut TcpStream) {
    write_line(stream, b"G").await;
    drain_until(stream, b"Goodbye").await;
}

/// Writes one CRLF-terminated command line.
pub async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

/// Reads until `needle` appears, returning all consumed bytes.
pub async fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
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

/// Returns true when `needle` appears in `haystack`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Returns the first byte offset where `needle` appears in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
