//! Telnet listener and terminal adapter (Slice 8 / Slice 9).
//!
//! Boots a [`tokio::net::TcpListener`], allocates a node from the
//! application [`NodePool`] for every accepted connection, performs
//! telnet negotiation and delegates the BBS workflow to
//! [`crate::app::session_driver`].

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::app::config::Config;
use crate::app::node_pool::NodePool;
use crate::app::services::{
    AppServices, SharedCallerLog, SharedConferences, SharedHasher, SharedScreens, SharedUserRepo,
};
use crate::app::session_driver::SessionDriver;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
#[cfg(test)]
use crate::app::wire_text::{
    ANSI_PROMPT, EMAIL_PROMPT, LINE_LENGTH_PROMPT, LOCATION_PROMPT, MENU_PROMPT,
    NEW_USER_PASSWORD_PROMPT, PASSWORD_PROMPT, PHONE_PROMPT, REGISTRATION_HANDLE_PROMPT,
    REGISTRATION_PASSWORD_CONFIRM_PROMPT, REGISTRATION_PASSWORD_PROMPT,
};
use crate::domain::session::LogonChannel;

use super::file_screen_repository::FileScreenRepository;
use super::telnet_line::{read_telnet_line, EchoMode};

/// Bytes sent at the start of every accepted connection to set up
/// telnet line-mode in a way that is friendly to common clients:
///   - `IAC WILL SUPPRESS-GO-AHEAD` and `IAC DO SUPPRESS-GO-AHEAD`
///     enable full-duplex.
///   - `IAC WILL ECHO` lets the server echo input so the user can see
///     what they type even if their client doesn't echo locally.
const IAC_INIT: &[u8] = &[
    0xFF, 0xFB, 0x03, // IAC WILL SUPPRESS-GO-AHEAD
    0xFF, 0xFD, 0x03, // IAC DO   SUPPRESS-GO-AHEAD
    0xFF, 0xFB, 0x01, // IAC WILL ECHO
];

/// Sent to clients that arrive when every node is in use.
const BUSY_LINE: &[u8] = b"All BBS nodes are busy. Please try again later.\r\n";

#[derive(Clone)]
struct ListenerRuntime {
    pool: Arc<NodePool>,
    services: AppServices,
}

/// Telnet listener and the application state it drives.
pub struct TelnetListener {
    listener: TcpListener,
    pool: Arc<NodePool>,
    services: AppServices,
}

impl TelnetListener {
    /// Binds a [`TcpListener`] on `addr` and constructs a fresh
    /// [`NodePool`] sized at `config.max_nodes`. `conferences` is the
    /// catalogue the post-auth `auto_rejoin` flow draws from
    /// (Slice 34a). Tests that don't exercise the conference flow may
    /// pass an empty `Vec`.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the bind fails.
    pub async fn bind<A: ToSocketAddrs>(
        addr: A,
        config: Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        conferences: SharedConferences,
    ) -> io::Result<Self> {
        let screens: SharedScreens = Arc::new(FileScreenRepository::new(config.bbs_path.clone()));
        Self::bind_with_screens(
            addr,
            config,
            user_repo,
            hasher,
            caller_log,
            screens,
            conferences,
        )
        .await
    }

    /// Binds a [`TcpListener`] using an injected screen repository.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the bind fails.
    pub async fn bind_with_screens<A: ToSocketAddrs>(
        addr: A,
        config: Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        screens: SharedScreens,
        conferences: SharedConferences,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let pool = Arc::new(NodePool::new(config.max_nodes));
        let default_ratio = DefaultRatio {
            mode: config.default_ratio_mode,
            value: config.default_ratio_value,
        };
        let new_user_gate = NewUserGateConfig {
            allow_new_users: config.allow_new_users,
            new_user_password: config.new_user_password.clone(),
            max_new_user_password_attempts: config.max_new_user_password_attempts,
        };
        let services = AppServices::new(
            user_repo,
            hasher,
            caller_log,
            screens,
            conferences,
            config.session_policy(),
            default_ratio,
            new_user_gate,
        );
        Ok(Self {
            listener,
            pool,
            services,
        })
    }

    /// Returns the local address the listener is bound to.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the address can't be
    /// queried.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Returns a clone of the shared [`NodePool`] for tests and the
    /// supervisor to introspect.
    pub fn pool(&self) -> Arc<NodePool> {
        self.pool.clone()
    }

    fn runtime(&self) -> ListenerRuntime {
        ListenerRuntime {
            pool: self.pool.clone(),
            services: self.services.clone(),
        }
    }

    /// Accepts connections forever, spawning a per-session task for
    /// each one. Returns only on a listener error.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] from `accept`.
    pub async fn run(&self) -> io::Result<()> {
        loop {
            let (stream, _peer) = self.listener.accept().await?;
            let runtime = self.runtime();
            tokio::spawn(async move {
                let _ = handle_connection(stream, runtime).await;
            });
        }
    }
}

/// Per-connection task body.
///
/// Splits into "could allocate a node" and "couldn't"; on the happy
/// path the task lives until the client closes the connection. On the
/// busy path it writes the busy line and exits.
async fn handle_connection(mut stream: TcpStream, runtime: ListenerRuntime) -> io::Result<()> {
    let Some(node_number) = runtime.pool.allocate().await else {
        stream.write_all(BUSY_LINE).await?;
        stream.flush().await?;
        return Ok(());
    };

    let result = {
        stream.write_all(IAC_INIT).await?;
        let terminal = TelnetTerminal::new(&mut stream);
        let mut driver = SessionDriver::new(
            terminal,
            node_number,
            LogonChannel::Remote,
            runtime.services,
        );
        driver.run().await
    };
    let _ = runtime.pool.release(node_number).await;
    result
}

struct TelnetTerminal<'a> {
    stream: &'a mut TcpStream,
    pushback: Option<u8>,
}

impl<'a> TelnetTerminal<'a> {
    fn new(stream: &'a mut TcpStream) -> Self {
        Self {
            stream,
            pushback: None,
        }
    }
}

impl Terminal for TelnetTerminal<'_> {
    type Error = io::Error;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move { self.stream.write_all(bytes).await })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async move { self.stream.flush().await })
    }

    fn read_line(
        &mut self,
        echo: TerminalEcho,
        timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async move {
            match tokio::time::timeout(
                timeout,
                read_telnet_line(self.stream, &mut self.pushback, echo.into()),
            )
            .await
            {
                Ok(result) => match result? {
                    Some(line) => Ok(TerminalRead::Line(line)),
                    None => Ok(TerminalRead::Eof),
                },
                Err(_elapsed) => Ok(TerminalRead::IdleTimedOut),
            }
        })
    }
}

impl From<TerminalEcho> for EchoMode {
    fn from(value: TerminalEcho) -> Self {
        match value {
            TerminalEcho::Visible => Self::Visible,
            TerminalEcho::Masked => Self::Masked,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::domain::node::NodeStatus;
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::user::User;
    use crate::domain::user_repository::NameLookupResult;

    fn nonexistent_bbs() -> PathBuf {
        std::env::temp_dir().join(format!(
            "nextexpress-test-no-such-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    fn test_config(max_nodes: u32) -> Config {
        Config {
            max_nodes,
            bbs_path: nonexistent_bbs(),
            max_password_failures: 3,
            ..Config::default()
        }
    }

    fn empty_repo() -> SharedUserRepo {
        Arc::new(InMemoryUserRepository::default())
    }

    fn test_hasher() -> SharedHasher {
        Arc::new(Pbkdf2PasswordHasher::new())
    }

    fn test_caller_log() -> SharedCallerLog {
        Arc::new(InMemoryCallerLog::new())
    }

    fn empty_conferences() -> SharedConferences {
        Arc::new(Vec::new())
    }

    fn test_conferences() -> SharedConferences {
        use crate::domain::conference::{Conference, MessageBase};
        let conf = Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference");
        Arc::new(vec![conf])
    }

    fn user_with_conf_membership(
        mut user: User,
        conferences: &[crate::domain::conference::Conference],
    ) -> User {
        crate::app::seed::grant_all_memberships(&mut user, conferences);
        user
    }

    /// Builds a user with a real PBKDF2 hash for `password`.
    fn alice_with_password(password: &str) -> User {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash(password, PasswordHashKind::Pbkdf210000)
            .expect("compute");
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    fn alice() -> User {
        // Stub credentials; never verified in tests that don't need auth.
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    fn repo_with_alice() -> SharedUserRepo {
        Arc::new(InMemoryUserRepository::new(vec![alice()]))
    }

    fn repo_with(user: User) -> SharedUserRepo {
        Arc::new(InMemoryUserRepository::new(vec![user]))
    }

    /// Reads from `stream` until either `needle` is found or the
    /// stream returns EOF. Returns the bytes read.
    async fn read_until_banner(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        for _ in 0..50 {
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(needle.len()).any(|w| w == needle) {
                break;
            }
        }
        buf
    }

    #[tokio::test]
    async fn listener_binds_and_reports_address() {
        let listener = TelnetListener::bind(
            "127.0.0.1:0",
            test_config(1),
            empty_repo(),
            test_hasher(),
            test_caller_log(),
            empty_conferences(),
        )
        .await
        .unwrap();
        let addr = listener.local_addr().unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn single_connection_sees_fallback_banner() {
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                empty_repo(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        let l = listener.clone();
        let _loop_task = tokio::spawn(async move { l.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = read_until_banner(&mut stream, b"NextExpress").await;
        assert!(
            buf.windows(b"NextExpress".len())
                .any(|w| w == b"NextExpress"),
            "expected NextExpress fallback banner, got {buf:?}"
        );
    }

    #[tokio::test]
    async fn banner_is_loaded_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("BBSTITLE.txt"),
            b"WELCOME TO TESTBBS\r\n",
        )
        .unwrap();

        let config = Config {
            max_nodes: 1,
            bbs_path: dir.path().to_path_buf(),
            max_password_failures: 3,
            ..Config::default()
        };
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                empty_repo(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        let l = listener.clone();
        let _loop_task = tokio::spawn(async move { l.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = read_until_banner(&mut stream, b"WELCOME TO TESTBBS").await;
        assert!(
            buf.windows(b"WELCOME TO TESTBBS".len())
                .any(|w| w == b"WELCOME TO TESTBBS"),
            "expected disk banner, got {buf:?}"
        );
    }

    #[tokio::test]
    async fn surplus_connection_receives_busy_line() {
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(2),
                empty_repo(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        let l = listener.clone();
        let _loop_task = tokio::spawn(async move { l.run().await });

        // Hold two connections open so both nodes are taken.
        let mut held = Vec::new();
        for _ in 0..2 {
            let mut s = TcpStream::connect(addr).await.unwrap();
            let _ = read_until_banner(&mut s, b"NextExpress").await;
            held.push(s);
        }

        // Wait briefly for the server tasks to settle into their read
        // loops (so the pool is at capacity before the surplus
        // connects).
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut surplus = TcpStream::connect(addr).await.unwrap();
        let buf = read_until_banner(&mut surplus, b"busy").await;
        assert!(
            buf.windows(4).any(|w| w == b"busy"),
            "expected busy line, got {buf:?}"
        );
    }

    #[tokio::test]
    async fn closing_a_connection_releases_its_node() {
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                empty_repo(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let pool = listener.pool();
        let addr = listener.local_addr().unwrap();
        let l = listener.clone();
        let _loop_task = tokio::spawn(async move { l.run().await });

        // First connection: take the only node.
        let mut first = TcpStream::connect(addr).await.unwrap();
        let _ = read_until_banner(&mut first, b"NextExpress").await;
        // Allow the server to observe the connection and finish allocating.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(pool.status_of(1).await, Some(NodeStatus::Connecting));

        // Drop the client; the server should detect EOF and release.
        drop(first);
        // Poll for release: read the status until it flips back to Idle
        // (give the runtime a moment per try).
        let mut idle = false;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if pool.status_of(1).await == Some(NodeStatus::Idle) {
                idle = true;
                break;
            }
        }
        assert!(idle, "node should return to idle after client closes");

        // A new connection should now be served.
        let mut second = TcpStream::connect(addr).await.unwrap();
        let buf = read_until_banner(&mut second, b"NextExpress").await;
        assert!(
            buf.windows(b"NextExpress".len())
                .any(|w| w == b"NextExpress"),
            "expected fresh banner after release, got {buf:?}"
        );
    }

    /// Spawns a listener bound to ephemeral port with `repo` and the
    /// fallback banner, returns the address it's listening on.
    async fn spawn_listener_with(repo: SharedUserRepo) -> SocketAddr {
        spawn_listener_with_config(repo, test_config(1)).await
    }

    /// Variant that lets a test pin a specific [`Config`].
    async fn spawn_listener_with_config(repo: SharedUserRepo, config: Config) -> SocketAddr {
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                repo,
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });
        addr
    }

    /// Variant that returns the concrete caller log so the test can
    /// inspect entries afterwards.
    async fn spawn_listener_with_log(
        repo: SharedUserRepo,
        config: Config,
        log: Arc<InMemoryCallerLog>,
    ) -> SocketAddr {
        let shared_log: SharedCallerLog = log;
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                repo,
                test_hasher(),
                shared_log,
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });
        addr
    }

    /// Reads bytes from `stream` until `needle` appears, EOF arrives,
    /// or 2s elapse with no further data. The timeout matters: under
    /// the default kernel buffering, a broken server will simply leave
    /// us blocked on `read` forever. With this we surface the failure
    /// in a couple of seconds instead.
    async fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 256];
        for _ in 0..200 {
            let read =
                tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut chunk))
                    .await;
            let n = match read {
                Ok(Ok(n)) => n,
                _ => 0,
            };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if out.windows(needle.len()).any(|w| w == needle) {
                break;
            }
        }
        out
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[tokio::test]
    async fn name_prompt_matches_amiexpress_wording() {
        // express.e:31774 sets the default name prompt to
        // 'Enter your Name:' and express.e:29571 prints it as
        // '\b\n<prompt> ' — i.e. CRLF prefix and trailing space on the
        // wire.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = drain_until(&mut stream, b"Enter your Name: ").await;
        assert!(
            contains(&buf, b"\r\nEnter your Name: "),
            "expected CRLF-prefixed AmiExpress name prompt: {buf:?}"
        );
    }

    #[tokio::test]
    async fn connection_displays_amiexpress_copyright_line() {
        // Mirrors express.e:25690 's 'AmiExpress \s Copyright ©<years> Darren Coles'.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = drain_until(&mut stream, b"Enter your Name: ").await;
        assert!(
            contains(
                &buf,
                "AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n".as_bytes()
            ),
            "expected AmiExpress copyright line: {buf:?}"
        );
    }

    #[tokio::test]
    async fn connection_displays_nextexpress_copyright_line() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = drain_until(&mut stream, b"Enter your Name: ").await;
        let needle = format!(
            "NextExpress {} Copyright \u{00A9}2026\r\n",
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            contains(&buf, needle.as_bytes()),
            "expected NextExpress copyright line: {buf:?}"
        );
    }

    #[tokio::test]
    async fn nextexpress_copyright_appears_above_amiexpress_copyright() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let buf = drain_until(&mut stream, b"Enter your Name: ").await;
        let nx_index = find_subslice(&buf, b"NextExpress ").expect("NextExpress line present");
        let ax_index = find_subslice(&buf, b"AmiExpress 5 ").expect("AmiExpress line present");
        assert!(
            nx_index < ax_index,
            "NextExpress line should appear above AmiExpress line: {buf:?}"
        );
    }

    /// Returns the byte index of the first occurrence of `needle` in `haystack`.
    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[tokio::test]
    async fn name_prompt_echoes_each_typed_character() {
        // express.e:2342 echoes the typed character back to the user
        // for ordinary line input. Our `IAC WILL ECHO` advertisement
        // means the client suppresses local echo, so we have to.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice").await.unwrap();
        let buf = drain_until(&mut stream, b"alice").await;
        assert!(
            contains(&buf, b"alice"),
            "expected typed handle echoed back to client: {buf:?}"
        );
    }

    #[tokio::test]
    async fn password_prompt_masks_typed_characters_with_asterisks() {
        // express.e:1543 sends a literal '*' on the wire for every
        // password byte, regardless of the local-console echo. The
        // VIEW_PASSWORD tooltype only affects the sysop's local
        // console, never the wire.
        let addr = spawn_listener_with(repo_with(alice_with_password("secret"))).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret").await.unwrap();
        let buf = drain_until(&mut stream, b"******").await;
        assert!(
            contains(&buf, b"******"),
            "expected six asterisks for a six-char password: {buf:?}"
        );
        assert!(
            !contains(&buf, b"secret"),
            "password must NEVER be echoed in plaintext: {buf:?}"
        );
    }

    #[tokio::test]
    async fn backspace_at_name_prompt_emits_bs_space_bs() {
        // express.e:2304-2320 erases the previous character with the
        // classic '<BS><SPACE><BS>' triplet. We mirror that.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"a\x08").await.unwrap();
        let buf = drain_until(&mut stream, b"a\x08 \x08").await;
        assert!(
            contains(&buf, b"a\x08 \x08"),
            "expected typed 'a' echo followed by BS-SPACE-BS: {buf:?}"
        );
    }

    #[tokio::test]
    async fn backspace_actually_deletes_from_typed_name() {
        // BS isn't just cosmetic — it must remove the previous byte
        // from what the server eventually submits to `name_typed`.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        // 'aliceX' + BS + Enter -> handle should be 'alice', advancing
        // to the password prompt instead of "Unknown user".
        stream.write_all(b"aliceX\x08\r\n").await.unwrap();
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, PASSWORD_PROMPT),
            "BS should leave handle as 'alice', advancing to password: {buf:?}"
        );
    }

    #[tokio::test]
    async fn backspace_on_empty_buffer_is_a_noop() {
        // Don't underflow or echo anything if the user mashes BS at
        // the start of the line.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"\x08\x08alice\r\n").await.unwrap();
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, PASSWORD_PROMPT),
            "BS at start should be ignored; 'alice' should authenticate normally: {buf:?}"
        );
    }

    #[tokio::test]
    async fn bare_cr_at_name_prompt_advances_without_blocking_on_trailer() {
        // SyncTerm (and other BBS-oriented clients) send a bare CR
        // for <Enter> rather than the CR+LF / CR+NUL pair some other
        // clients send. We must not block waiting for a trailer that
        // never arrives — otherwise the user has to press Enter
        // twice before the server reacts.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r").await.unwrap();
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, PASSWORD_PROMPT),
            "bare CR should advance to password prompt: {buf:?}"
        );
    }

    #[tokio::test]
    async fn cr_nul_trailer_at_name_prompt_advances() {
        // Telnet's traditional newline per RFC 854 is CR+NUL — we
        // must accept that as a single line break.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\0").await.unwrap();
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, PASSWORD_PROMPT),
            "CR+NUL trailer should be treated as one line break: {buf:?}"
        );
    }

    #[tokio::test]
    async fn enter_at_name_prompt_emits_crlf_to_client() {
        // After echoing the typed name, the server must also echo a
        // CRLF when the user presses Enter so the cursor moves to the
        // next line on the client display.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        // We expect bytes between the echoed 'e' (last char of alice)
        // and the next "Password: " prompt to include "\r\n".
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        let after_alice = find_subslice(&buf, b"alice")
            .map(|i| i + b"alice".len())
            .expect("'alice' echo present");
        let pwd_index = find_subslice(&buf, PASSWORD_PROMPT).expect("Password prompt present");
        let between = &buf[after_alice..pwd_index];
        assert!(
            contains(between, b"\r\n"),
            "expected CRLF between echoed handle and Password prompt: {between:?}"
        );
    }

    #[tokio::test]
    async fn existing_handle_advances_to_password_prompt() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let buf = drain_until(&mut stream, PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, PASSWORD_PROMPT),
            "expected password prompt: {buf:?}"
        );
    }

    #[tokio::test]
    async fn correct_password_authenticates_user() {
        let addr = spawn_listener_with(repo_with(alice_with_password("secret"))).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"Authenticated").await;
        assert!(
            contains(&buf, b"Authenticated"),
            "expected Authenticated line: {buf:?}"
        );
    }

    #[tokio::test]
    async fn closing_socket_at_name_prompt_appends_carrier_loss_entry() {
        // Slice 18: closing the TCP socket mid-prompt fires the
        // CarrierLost rule. The caller log gets a finalise entry
        // tagged with carrier_loss.
        let log = Arc::new(InMemoryCallerLog::new());
        let addr = spawn_listener_with_log(repo_with_alice(), test_config(1), log.clone()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        drop(stream);
        // Wait for the listener task to observe EOF and finalise.
        let mut entries = vec![];
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            entries = log.entries();
            if entries.iter().any(|e| e.text.contains("carrier_loss")) {
                break;
            }
        }
        assert!(
            entries.iter().any(|e| e.text.contains("carrier_loss")),
            "expected carrier_loss in log: {entries:?}"
        );
    }

    #[tokio::test]
    async fn closing_socket_at_password_prompt_appends_carrier_loss_entry() {
        let log = Arc::new(InMemoryCallerLog::new());
        let addr = spawn_listener_with_log(
            repo_with(alice_with_password("secret")),
            test_config(1),
            log.clone(),
        )
        .await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        drop(stream);
        let mut entries = vec![];
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            entries = log.entries();
            if entries.iter().any(|e| e.text.contains("carrier_loss")) {
                break;
            }
        }
        assert!(
            entries.iter().any(|e| e.text.contains("carrier_loss")),
            "expected carrier_loss in log: {entries:?}"
        );
    }

    #[tokio::test]
    async fn idle_timeout_at_name_prompt_disconnects_with_goodbye() {
        // Slice 17: the listener applies the IdleTimeout rule when
        // it doesn't see input within `input_timeout`. With a 100ms
        // budget and no client typing, the listener should write
        // the goodbye line and close.
        let config = Config {
            input_timeout: std::time::Duration::from_millis(100),
            ..test_config(1)
        };
        let addr = spawn_listener_with_config(repo_with_alice(), config).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        // Don't type anything; the listener will time out.
        let buf = drain_until(&mut stream, b"Idle timeout").await;
        assert!(
            contains(&buf, b"Idle timeout"),
            "expected idle goodbye: {buf:?}"
        );
    }

    #[tokio::test]
    async fn idle_timeout_at_password_prompt_disconnects_with_goodbye() {
        let config = Config {
            input_timeout: std::time::Duration::from_millis(100),
            ..test_config(1)
        };
        let addr =
            spawn_listener_with_config(repo_with(alice_with_password("secret")), config).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        // Don't send a password.
        let buf = drain_until(&mut stream, b"Idle timeout").await;
        assert!(
            contains(&buf, b"Idle timeout"),
            "expected idle goodbye at password prompt: {buf:?}"
        );
    }

    #[tokio::test]
    async fn idle_timeout_in_menu_disconnects_with_goodbye() {
        // Reaching the menu now requires conference access (Slice
        // 34a auto-rejoin), so seed alice with a Conf01 grant.
        let config = Config {
            input_timeout: std::time::Duration::from_millis(100),
            ..test_config(1)
        };
        let conferences = test_conferences();
        let alice = user_with_conf_membership(alice_with_password("secret"), &conferences);
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                Arc::new(InMemoryUserRepository::new(vec![alice])),
                test_hasher(),
                test_caller_log(),
                conferences,
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret\r\n").await.unwrap();
        // Wait for the menu, then idle.
        let _ = drain_until(&mut stream, MENU_PROMPT).await;
        let buf = drain_until(&mut stream, b"Idle timeout").await;
        assert!(
            contains(&buf, b"Idle timeout"),
            "expected idle goodbye in menu: {buf:?}"
        );
    }

    #[tokio::test]
    async fn locked_user_authenticating_sees_logon_rejected() {
        // Slice 16: a user whose account_locked is true authenticates
        // with the right password but RejectLockedOrInsufficientAccess
        // bounces them. Listener writes "Logon rejected. Goodbye."
        let mut user = alice_with_password("secret");
        user.lock_account();
        let addr = spawn_listener_with(repo_with(user)).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"Logon rejected").await;
        assert!(
            contains(&buf, b"Logon rejected"),
            "expected rejection goodbye: {buf:?}"
        );
    }

    #[tokio::test]
    async fn five_unknown_handles_end_session() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        for _ in 0..5 {
            let _ = drain_until(&mut stream, b"Enter your Name: ").await;
            stream.write_all(b"nobody\r\n").await.unwrap();
        }
        let buf = drain_until(&mut stream, b"Too many").await;
        assert!(
            contains(&buf, b"Too many"),
            "expected 'Too many' line: {buf:?}"
        );
    }

    #[tokio::test]
    async fn new_keyword_displays_fallback_newuserpw_screen_then_prompts_for_handle() {
        // Slice 19 / Slice 20: typing NEW transitions the session to
        // new_user_registering, the listener writes the NEWUSERPW
        // screen (built-in fallback when no asset is on disk), and
        // then re-prompts for the handle the user wants to register
        // with.
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let buf =
            drain_until_both(&mut stream, b"New user registration.", b"Enter your Name: ").await;
        assert!(
            contains(&buf, b"New user registration."),
            "expected NEWUSERPW fallback: {buf:?}"
        );
        assert!(
            contains(&buf, b"Enter your Name: "),
            "expected re-prompt for handle: {buf:?}"
        );
    }

    #[tokio::test]
    async fn new_keyword_displays_disk_newuserpw_screen_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("NEWUSERPW.txt"),
            b"WELCOME NEW USER\x08\n",
        )
        .unwrap();
        let config = Config {
            max_nodes: 1,
            bbs_path: dir.path().to_path_buf(),
            max_password_failures: 3,
            ..Config::default()
        };
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                repo_with_alice(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"WELCOME NEW USER").await;
        assert!(
            contains(&buf, b"WELCOME NEW USER\r\n"),
            "expected disk NEWUSERPW asset: {buf:?}"
        );
    }

    #[tokio::test]
    async fn full_signin_menu_goodbye_ended_path() {
        // Slice 13's headline test: telnet in, type handle, type
        // password, type G, see goodbye and the connection close
        // with the node back in idle.
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            max_nodes: 1,
            bbs_path: dir.path().to_path_buf(),
            max_password_failures: 3,
            ..Config::default()
        };
        let log = Arc::new(InMemoryCallerLog::new());
        let conferences = test_conferences();
        let alice = user_with_conf_membership(alice_with_password("secret"), &conferences);
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                Arc::new(InMemoryUserRepository::new(vec![alice])),
                test_hasher(),
                log.clone() as SharedCallerLog,
                conferences,
            )
            .await
            .unwrap(),
        );
        let pool = listener.pool();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret\r\n").await.unwrap();
        let _ = drain_until(&mut stream, b"Command: ").await;
        stream.write_all(b"G\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"Goodbye").await;
        assert!(contains(&buf, b"Goodbye"), "expected goodbye: {buf:?}");

        // The connection should now close (stream EOF).
        let mut tail = Vec::new();
        let mut chunk = [0u8; 256];
        for _ in 0..50 {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => tail.extend_from_slice(&chunk[..n]),
            }
        }

        // Wait for the server to release the node.
        let mut idle = false;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if pool.status_of(1).await == Some(NodeStatus::Idle) {
                idle = true;
                break;
            }
        }
        assert!(idle, "node should be idle after goodbye");

        // Caller log should contain logon and logoff lines for alice.
        let entries = log.entries();
        assert!(
            entries
                .iter()
                .any(|e| e.text.contains("Logon:") && e.text.contains("alice")),
            "expected logon entry: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.text.contains("Logoff:") && e.text.contains("alice")),
            "expected logoff entry: {entries:?}"
        );
    }

    #[tokio::test]
    async fn authenticated_session_receives_menu() {
        // Slice 34a: post-auth the session auto-rejoins the user's
        // first accessible conference and renders the per-conference
        // `Conf<NN>/menu.txt`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        std::fs::write(
            dir.path().join("Conf01").join("conference.toml"),
            b"number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Conf01").join("menu.txt"),
            b"MENU CONTENT\x08\n",
        )
        .unwrap();
        let config = Config {
            max_nodes: 1,
            bbs_path: dir.path().to_path_buf(),
            max_password_failures: 3,
            ..Config::default()
        };
        let conferences = test_conferences();
        let alice = user_with_conf_membership(alice_with_password("secret"), &conferences);
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                Arc::new(InMemoryUserRepository::new(vec![alice])),
                test_hasher(),
                test_caller_log(),
                conferences,
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PASSWORD_PROMPT).await;
        stream.write_all(b"secret\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"MENU CONTENT").await;
        assert!(
            contains(&buf, b"MENU CONTENT"),
            "expected menu content: {buf:?}"
        );
        // Translation: Amiga \b\n becomes \r\n in the output.
        assert!(
            contains(&buf, b"MENU CONTENT\r\n"),
            "expected CRLF after translation: {buf:?}"
        );
    }

    /// Reads from `stream` until both `a` and `b` have appeared in the
    /// accumulated buffer (or EOF / 2s of silence).
    async fn drain_until_both(stream: &mut TcpStream, a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 256];
        for _ in 0..200 {
            let read =
                tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut chunk))
                    .await;
            let n = match read {
                Ok(Ok(n)) => n,
                _ => 0,
            };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if contains(&out, a) && contains(&out, b) {
                break;
            }
        }
        out
    }

    #[tokio::test]
    async fn full_new_user_registration_persists_account_and_lands_in_menu() {
        // Slice 20 + 34a: completing the registration form persists
        // the freshly-created account. Because the new user has a
        // pre-existing Conf01 membership granted to them at sign-up
        // time (here pre-seeded by the test for parity with the
        // bootstrap path), auto-rejoin succeeds and the menu loop
        // engages, delivering the headline "Welcome aboard..." +
        // "Command:" sequence.
        let log = Arc::new(InMemoryCallerLog::new());
        let repo: SharedUserRepo = Arc::new(InMemoryUserRepository::default());
        let conferences = test_conferences();
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                repo.clone(),
                test_hasher(),
                log.clone() as SharedCallerLog,
                conferences,
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_HANDLE_PROMPT).await;
        stream.write_all(b"newbie\r\n").await.unwrap();
        let _ = drain_until(&mut stream, LOCATION_PROMPT).await;
        stream.write_all(b"Townsville\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PHONE_PROMPT).await;
        stream.write_all(b"555-0123\r\n").await.unwrap();
        let _ = drain_until(&mut stream, EMAIL_PROMPT).await;
        stream.write_all(b"newbie@example.com\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_PROMPT).await;
        stream.write_all(b"hunter2\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_CONFIRM_PROMPT).await;
        stream.write_all(b"hunter2\r\n").await.unwrap();
        let _ = drain_until(&mut stream, LINE_LENGTH_PROMPT).await;
        stream.write_all(b"80\r\n").await.unwrap();
        let _ = drain_until(&mut stream, ANSI_PROMPT).await;
        stream.write_all(b"Y\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"No accessible conferences").await;
        assert!(
            contains(&buf, b"Welcome aboard"),
            "expected registration-complete line: {buf:?}"
        );
        // Slice 34a: a freshly-registered user has no granted
        // memberships, so the auto-rejoin path terminates with
        // `no_conference_access`. Sysop validates and grants
        // memberships in a later phase; until then this is the
        // expected post-registration outcome.
        assert!(
            contains(&buf, b"No accessible conferences"),
            "expected no-conference-access line: {buf:?}"
        );

        // Account persisted with spec defaults.
        match repo.find_by_handle("newbie") {
            NameLookupResult::Found(user) => {
                assert!(user.is_new_user());
                assert_eq!(user.location(), Some("Townsville"));
                assert_eq!(user.phone_number(), Some("555-0123"));
                assert_eq!(user.email(), Some("newbie@example.com"));
                assert_eq!(user.line_length(), 80);
                assert!(user.ansi_colour());
                assert_eq!(user.access_level(), 2);
                assert!(
                    user.memberships().is_empty(),
                    "new user starts with no grants"
                );
            }
            other => panic!("expected newbie to be created, got {other:?}"),
        }

        // FinaliseLogoff still runs; the caller log carries a logoff
        // entry for the new user even though enter_menu was skipped
        // by the no-access branch.
        let mut entries = vec![];
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            entries = log.entries();
            if entries
                .iter()
                .any(|e| e.text.contains("Logoff:") && e.text.contains("newbie"))
            {
                break;
            }
        }
        assert!(
            entries
                .iter()
                .any(|e| e.text.contains("Logoff:") && e.text.contains("newbie")),
            "expected logoff entry for new user: {entries:?}"
        );
    }

    #[tokio::test]
    async fn registration_with_mismatched_passwords_re_prompts() {
        let repo: SharedUserRepo = Arc::new(InMemoryUserRepository::default());
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                repo,
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_HANDLE_PROMPT).await;
        stream.write_all(b"newbie\r\n").await.unwrap();
        let _ = drain_until(&mut stream, LOCATION_PROMPT).await;
        stream.write_all(b"Townsville\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PHONE_PROMPT).await;
        stream.write_all(b"555\r\n").await.unwrap();
        let _ = drain_until(&mut stream, EMAIL_PROMPT).await;
        stream.write_all(b"n@example.com\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_PROMPT).await;
        stream.write_all(b"hunter2\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_CONFIRM_PROMPT).await;
        stream.write_all(b"different\r\n").await.unwrap();
        // The mismatch line and the re-prompt arrive in either one or
        // two reads; drain until both have been seen.
        let buf =
            drain_until_both(&mut stream, b"do not match", REGISTRATION_PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, b"Passwords do not match"),
            "expected mismatch line: {buf:?}"
        );
        assert!(
            contains(&buf, REGISTRATION_PASSWORD_PROMPT),
            "expected re-prompt after mismatch: {buf:?}"
        );
    }

    #[tokio::test]
    async fn registration_rejects_existing_handle_and_reprompts() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_HANDLE_PROMPT).await;
        // alice is already registered.
        stream.write_all(b"alice\r\n").await.unwrap();
        let buf = drain_until_both(&mut stream, b"is taken", REGISTRATION_HANDLE_PROMPT).await;
        assert!(
            contains(&buf, b"That name is taken"),
            "expected handle-taken rejection: {buf:?}"
        );
    }

    #[tokio::test]
    async fn typing_new_when_registration_disallowed_writes_screen_and_disconnects() {
        // Slice 20a: with allow_new_users = false, typing NEW should
        // render the NONEWUSERS screen (built-in fall-back here) and
        // close the session via RejectDisallowedRegistration.
        let log = Arc::new(InMemoryCallerLog::new());
        let config = Config {
            allow_new_users: false,
            ..test_config(1)
        };
        let addr = spawn_listener_with_log(repo_with_alice(), config, log.clone()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"not available").await;
        assert!(
            contains(&buf, b"New user registration is not available"),
            "expected NONEWUSERS fall-back: {buf:?}"
        );

        // FinaliseLogoff must run; caller log should carry a logoff line.
        let mut entries = vec![];
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            entries = log.entries();
            if entries.iter().any(|e| e.text.contains("Logoff:")) {
                break;
            }
        }
        assert!(
            entries.iter().any(|e| e.text.contains("Logoff:")),
            "expected logoff entry after RejectDisallowedRegistration: {entries:?}"
        );
    }

    #[tokio::test]
    async fn new_user_password_gate_accepts_correct_password() {
        let log = Arc::new(InMemoryCallerLog::new());
        let repo: SharedUserRepo = Arc::new(InMemoryUserRepository::default());
        let config = Config {
            new_user_password: Some("letmein".to_string()),
            ..test_config(1)
        };
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                repo.clone(),
                test_hasher(),
                log.clone() as SharedCallerLog,
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        // Gate prompt must appear before the registration handle prompt.
        let buf = drain_until(&mut stream, NEW_USER_PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, NEW_USER_PASSWORD_PROMPT),
            "expected new-user password prompt: {buf:?}"
        );
        // Case-insensitive match (parity with StriCmp).
        stream.write_all(b"LETMEIN\r\n").await.unwrap();
        let buf = drain_until(&mut stream, REGISTRATION_HANDLE_PROMPT).await;
        assert!(
            contains(&buf, b"Correct"),
            "expected gate-passed acknowledgement: {buf:?}"
        );
        assert!(
            contains(&buf, REGISTRATION_HANDLE_PROMPT),
            "expected registration form to follow: {buf:?}"
        );
    }

    #[tokio::test]
    async fn new_user_password_gate_three_failures_disconnects() {
        let log = Arc::new(InMemoryCallerLog::new());
        let config = Config {
            new_user_password: Some("letmein".to_string()),
            ..test_config(1)
        };
        let addr = spawn_listener_with_log(
            Arc::new(InMemoryUserRepository::default()),
            config,
            log.clone(),
        )
        .await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        for _ in 0..3 {
            let _ = drain_until(&mut stream, NEW_USER_PASSWORD_PROMPT).await;
            stream.write_all(b"wrong\r\n").await.unwrap();
        }
        let buf = drain_until(&mut stream, b"Excessive Password Failure").await;
        assert!(
            contains(&buf, b"Excessive Password Failure"),
            "expected excessive-failures line: {buf:?}"
        );

        // Three failure entries plus the logoff line.
        let mut entries = vec![];
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            entries = log.entries();
            if entries.iter().filter(|e| e.is_password_failure).count() >= 3 {
                break;
            }
        }
        let failures = entries
            .iter()
            .filter(|e| e.text.contains("New-user password failure"))
            .count();
        assert_eq!(
            failures, 3,
            "expected three new-user-password failure entries: {entries:?}"
        );
    }

    #[tokio::test]
    async fn new_user_password_gate_blocks_registration_until_passed() {
        // With the gate armed, completing the registration form
        // should be blocked until the gate passes. Verify by entering
        // a wrong password, then a right one — the registration
        // proceeds only after the gate is satisfied.
        let repo: SharedUserRepo = Arc::new(InMemoryUserRepository::default());
        let config = Config {
            new_user_password: Some("opensesame".to_string()),
            ..test_config(1)
        };
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                repo.clone(),
                test_hasher(),
                test_caller_log(),
                empty_conferences(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        // First: wrong password → re-prompt.
        let _ = drain_until(&mut stream, NEW_USER_PASSWORD_PROMPT).await;
        stream.write_all(b"nope\r\n").await.unwrap();
        let buf =
            drain_until_both(&mut stream, b"Invalid PassWord", NEW_USER_PASSWORD_PROMPT).await;
        assert!(
            contains(&buf, b"Invalid PassWord"),
            "expected invalid-password line: {buf:?}"
        );
        // Right password → registration form starts.
        stream.write_all(b"opensesame\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_HANDLE_PROMPT).await;
        stream.write_all(b"newbie\r\n").await.unwrap();
        let _ = drain_until(&mut stream, LOCATION_PROMPT).await;
        stream.write_all(b"Town\r\n").await.unwrap();
        let _ = drain_until(&mut stream, PHONE_PROMPT).await;
        stream.write_all(b"555\r\n").await.unwrap();
        let _ = drain_until(&mut stream, EMAIL_PROMPT).await;
        stream.write_all(b"n@example.com\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_PROMPT).await;
        stream.write_all(b"hunter2\r\n").await.unwrap();
        let _ = drain_until(&mut stream, REGISTRATION_PASSWORD_CONFIRM_PROMPT).await;
        stream.write_all(b"hunter2\r\n").await.unwrap();
        let _ = drain_until(&mut stream, LINE_LENGTH_PROMPT).await;
        stream.write_all(b"80\r\n").await.unwrap();
        let _ = drain_until(&mut stream, ANSI_PROMPT).await;
        stream.write_all(b"Y\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"Welcome aboard").await;
        assert!(
            contains(&buf, b"Welcome aboard"),
            "expected gated-registration to complete: {buf:?}"
        );

        match repo.find_by_handle("newbie") {
            NameLookupResult::Found(user) => {
                assert!(user.is_new_user());
            }
            other => panic!("expected newbie to be created, got {other:?}"),
        }
    }
}
