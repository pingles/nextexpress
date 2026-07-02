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
///
/// The four positional fields cover the common shape (a seeded sysop
/// on one node); the `with_*` builder knobs cover everything the
/// per-smoke spawn helpers used to hand-roll — extra users, sysop
/// adjustments, and `Config` overrides such as `max_nodes`.
/// Deferred [`Config`] adjustment applied after the fixture defaults.
type ConfigTune = Box<dyn FnOnce(&mut Config) + Send>;
/// Deferred adjustment of the seeded sysop.
type SysopTune = Box<dyn FnOnce(&mut nextexpress::domain::user::User) + Send>;
/// Deferred construction of an extra user; receives the runtime's hasher.
type UserSeed = Box<dyn FnOnce(&Pbkdf2PasswordHasher) -> nextexpress::domain::user::User + Send>;

pub struct TestRuntime {
    /// BBS root used by file-screen and file-mail adapters.
    pub bbs_path: PathBuf,
    /// Conference catalogue installed in the runtime.
    pub conferences: Vec<Conference>,
    /// Mail stores installed in the runtime.
    pub mail_stores: SharedMailStores,
    /// File catalogue installed in the runtime.
    pub file_repo: SharedFileRepo,
    tune_config: Option<ConfigTune>,
    tune_sysop: Option<SysopTune>,
    extra_users: Vec<UserSeed>,
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
            tune_config: None,
            tune_sysop: None,
            extra_users: Vec::new(),
        }
    }

    /// Overrides [`Config`] fields after the fixture defaults are set
    /// (`max_nodes: 1`, `max_password_failures: 3`, the fixture's
    /// `bbs_path`) — e.g. `.with_config(|c| c.max_nodes = 2)` for a
    /// two-session smoke.
    #[must_use]
    pub fn with_config(mut self, tune: impl FnOnce(&mut Config) + Send + 'static) -> Self {
        self.tune_config = Some(Box::new(tune));
        self
    }

    /// Adjusts the seeded sysop after its memberships are granted —
    /// e.g. clearing per-conference scan flags so a smoke skips the
    /// logon conference scan.
    #[must_use]
    pub fn with_sysop(
        mut self,
        tune: impl FnOnce(&mut nextexpress::domain::user::User) + Send + 'static,
    ) -> Self {
        self.tune_sysop = Some(Box::new(tune));
        self
    }

    /// Seeds one additional user. The closure receives the runtime's
    /// password hasher so the user's credentials verify at the real
    /// login prompt; sign in with [`sign_in`].
    #[must_use]
    pub fn with_user(
        mut self,
        make: impl FnOnce(&Pbkdf2PasswordHasher) -> nextexpress::domain::user::User + Send + 'static,
    ) -> Self {
        self.extra_users.push(Box::new(make));
        self
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
    if let Some(tune) = fixture.tune_sysop {
        tune(&mut sysop);
    }
    let mut users = vec![sysop];
    for make in fixture.extra_users {
        users.push(make(hasher.as_ref()));
    }

    let user_repo: SharedUserRepo =
        Arc::new(InMemoryUserRepository::new(users)) as Arc<dyn UserRepository + Send + Sync>;
    let hasher: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;
    let conferences: SharedConferences = Arc::new(fixture.conferences);

    let mut config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        bbs_path: fixture.bbs_path,
        ..Config::default()
    };
    if let Some(tune) = fixture.tune_config {
        tune(&mut config);
    }
    let runtime = bootstrap::build_runtime(
        &config,
        RuntimeAdapters {
            user_repo,
            hasher,
            caller_log,
            conferences,
            mail_stores: fixture.mail_stores,
            file_repo: fixture.file_repo,
            flagged_store: Arc::new(
                nextexpress::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new(),
            ),
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

/// Connects as `handle`/`password` (e.g. a [`TestRuntime::with_user`]
/// seed) and returns the stream at the menu prompt.
pub async fn sign_in(addr: &SocketAddr, handle: &[u8], password: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, handle).await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, password).await;
    drain_until(&mut stream, b"mins. left): ").await;
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

/// Forces a logoff with `G Y` (slice D5/Ga): a plain `G` opens the
/// flagged-file confirm when a test left a file flagged, so teardown
/// uses the force form — mirroring the FS-UAE reference discipline of
/// always ending a session with `G Y`.
pub async fn end_session_forced(stream: &mut TcpStream) {
    write_line(stream, b"G Y").await;
    drain_until(stream, b"Goodbye").await;
}

/// Writes one CRLF-terminated command line.
pub async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

/// Sends one bare pager hotkey — no line terminator (slice D2b: the
/// `More?` prompt acts per keypress, `ae_tierd_aquascan3.txt:321`,
/// `ae_tierd_aquascan4.txt` U1; a terminated `n\r\n` would instead
/// mean held-n + Enter = the probe-P1 quit).
pub async fn write_key(stream: &mut TcpStream, key: &[u8]) {
    stream.write_all(key).await.expect("write key");
    stream.flush().await.expect("flush");
}

/// Reads whatever arrives within `window` of idle — the keystroke-
/// granular observation primitive (slice D2b: prove a key echoes on
/// its own keypress, before any terminator is sent).
pub async fn read_idle(stream: &mut TcpStream, window: Duration) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    while let Ok(Ok(n)) = tokio::time::timeout(window, stream.read(&mut chunk)).await {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
    }
    out
}

/// Reads until `needle` appears, returning all consumed bytes.
///
/// Panics with a distinct message per failure mode — deadline
/// expired, connection closed, read error — so a failing smoke says
/// what actually happened instead of a uniform "needle not found".
pub async fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let Ok(read) = tokio::time::timeout(DRAIN_DEADLINE, stream.read(&mut chunk)).await else {
            panic!(
                "needle {:?} not seen within {DRAIN_DEADLINE:?}; got {:?}",
                std::str::from_utf8(needle).unwrap_or("<bin>"),
                String::from_utf8_lossy(&out),
            );
        };
        match read {
            Ok(0) => panic!(
                "connection closed before needle {:?}; got {:?}",
                std::str::from_utf8(needle).unwrap_or("<bin>"),
                String::from_utf8_lossy(&out),
            ),
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if contains(&out, needle) {
                    return out;
                }
            }
            Err(error) => panic!(
                "read failed ({error}) before needle {:?}; got {:?}",
                std::str::from_utf8(needle).unwrap_or("<bin>"),
                String::from_utf8_lossy(&out),
            ),
        }
    }
}

/// Returns true when `needle` appears in `haystack`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Returns the first byte offset where `needle` appears in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
