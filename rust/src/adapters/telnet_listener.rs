//! Telnet listener and per-session task (Slice 8 / Slice 9).
//!
//! Boots a [`tokio::net::TcpListener`], allocates a node from the
//! application [`NodePool`] for every accepted connection, invokes the
//! `AcceptConnection`, `PromptForName` and `NameTyped` rules.
//! Subsequent slices extend the per-session task with the
//! `VerifyPassword` and menu rules.

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use crate::app::node_pool::NodePool;
use crate::app::session_flow;
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::config::Config;
use crate::domain::password::PasswordHasher;
use crate::domain::session::{LogonChannel, NameTypedOutcome, Session, VerifyPasswordOutcome};
use crate::domain::user_repository::UserRepository;

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

/// Built-in fallback banner used when the configured `BBSTITLE.txt`
/// file is missing. Telnet line ending (CRLF) so it renders correctly
/// on Linux and macOS clients that don't translate bare LF.
const FALLBACK_BANNER: &[u8] = b"NextExpress\r\n";

/// Two-line copyright block printed on every accepted connection,
/// directly after the BBS title banner. The NextExpress line sits
/// above the AmiExpress line to make the lineage obvious; the
/// AmiExpress line mirrors the original BBS's banner verbatim
/// (`amiexpress/express.e:25690`, modulo the legacy file's mojibake of
/// the © glyph).
const COPYRIGHT_LINES: &[u8] = concat!(
    "NextExpress ",
    env!("CARGO_PKG_VERSION"),
    " Copyright \u{00A9}2026\r\n",
    "AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n",
)
.as_bytes();

/// Prompt sent before reading the user's handle. Mirrors the original
/// AmiExpress wire format: a CRLF prefix and trailing space around the
/// default `NAME_PROMPT` of `Enter your Name:` (see
/// `amiexpress/express.e:29571` and `:31774`).
const NAME_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Sent after a not-found name lookup to invite a retry.
const UNKNOWN_USER_LINE: &[u8] = b"Unknown user.\r\n";

/// Sent when the user types `NEW`. Slice 19 wires this up to the
/// registration flow; until then we explain why and re-prompt.
const NEW_NOT_SUPPORTED_LINE: &[u8] =
    b"New users are not yet supported. Please use an existing handle.\r\n";

/// Sent when the user has burned through all five name retries.
const TOO_MANY_RETRIES_LINE: &[u8] = b"Too many failed login attempts. Goodbye.\r\n";

/// Prompt for the user's password.
const PASSWORD_PROMPT: &[u8] = b"Password: ";

/// Sent after a successful authentication. The conference menu lands
/// in Slice 12; until then we acknowledge and idle.
const AUTHENTICATED_LINE: &[u8] = b"Authenticated.\r\n";

/// Sent when the password didn't match. Slice 11 elaborates this with
/// retry counts and lockout; for now we just inform and close.
const WRONG_PASSWORD_LINE: &[u8] = b"Incorrect password.\r\n";

/// Built-in fallback menu used when the configured `Conf02/Menu.txt`
/// file is missing.
const FALLBACK_MENU: &[u8] = b"[ Default menu - type G to log off ]\r\n";

/// Prompt printed after each menu screen, awaiting a command.
const MENU_PROMPT: &[u8] = b"Command: ";

/// Sent for unrecognised menu commands.
const UNKNOWN_COMMAND_LINE: &[u8] = b"Unknown command. Type G to log off.\r\n";

/// Sent immediately before the connection closes on a normal logoff.
const GOODBYE_LINE: &[u8] = b"Goodbye!\r\n";

/// Type alias for the user repository the listener drives.
type SharedUserRepo = Arc<dyn UserRepository + Send + Sync + 'static>;

/// Type alias for the password hasher the listener drives.
type SharedHasher = Arc<dyn PasswordHasher + Send + Sync + 'static>;

/// Type alias for the caller-log appender the listener drives.
type SharedCallerLog = Arc<dyn CallerLogAppender + Send + Sync + 'static>;

/// Telnet listener and the application state it drives.
pub struct TelnetListener {
    listener: TcpListener,
    pool: Arc<NodePool>,
    bbs_path: PathBuf,
    max_password_failures: u32,
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
}

impl TelnetListener {
    /// Binds a [`TcpListener`] on `addr` and constructs a fresh
    /// [`NodePool`] sized at `config.max_nodes`.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] if the bind fails.
    pub async fn bind<A: ToSocketAddrs>(
        addr: A,
        config: Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let pool = Arc::new(NodePool::new(config.max_nodes));
        Ok(Self {
            listener,
            pool,
            bbs_path: config.bbs_path,
            max_password_failures: config.max_password_failures,
            user_repo,
            hasher,
            caller_log,
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

    /// Accepts connections forever, spawning a per-session task for
    /// each one. Returns only on a listener error.
    ///
    /// # Errors
    /// Returns the underlying [`io::Error`] from `accept`.
    pub async fn run(&self) -> io::Result<()> {
        loop {
            let (stream, _peer) = self.listener.accept().await?;
            let pool = self.pool.clone();
            let bbs_path = self.bbs_path.clone();
            let repo = self.user_repo.clone();
            let hasher = self.hasher.clone();
            let log = self.caller_log.clone();
            let max_pw_fails = self.max_password_failures;
            tokio::spawn(async move {
                let _ = handle_connection(stream, pool, bbs_path, repo, hasher, log, max_pw_fails)
                    .await;
            });
        }
    }
}

/// Per-connection task body.
///
/// Splits into "could allocate a node" and "couldn't"; on the happy
/// path the task lives until the client closes the connection. On the
/// busy path it writes the busy line and exits.
#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: TcpStream,
    pool: Arc<NodePool>,
    bbs_path: PathBuf,
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
    max_password_failures: u32,
) -> io::Result<()> {
    let Some(node_number) = pool.allocate().await else {
        stream.write_all(BUSY_LINE).await?;
        stream.flush().await?;
        return Ok(());
    };

    let result = run_session(
        &mut stream,
        &bbs_path,
        node_number,
        user_repo.as_ref(),
        hasher.as_ref(),
        caller_log.as_ref(),
        max_password_failures,
    )
    .await;
    let _ = pool.release(node_number).await;
    result
}

/// Inner per-session loop. Drives the session from the IAC handshake
/// through banner, name prompt, password and (in later slices) menu.
#[allow(clippy::too_many_arguments)]
async fn run_session(
    stream: &mut TcpStream,
    bbs_path: &Path,
    node_number: u32,
    user_repo: &(dyn UserRepository + Send + Sync),
    hasher: &(dyn PasswordHasher + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    max_password_failures: u32,
) -> io::Result<()> {
    stream.write_all(IAC_INIT).await?;

    // Single-byte pushback slot owned by this session. Lets
    // `read_telnet_line` peek at the byte after a CR without
    // consuming it if it isn't a trailer (see `try_consume_cr_trailer`).
    let mut pushback: Option<u8> = None;

    // Invoke AcceptConnection. The pool already moved the node to
    // `Connecting` atomically, so no other session can be active here.
    let mut session = Session::accept_connection(
        node_number,
        LogonChannel::Remote,
        0,
        SystemTime::now(),
        None,
    )
    .expect("freshly allocated node has no existing session");

    let banner = read_banner(bbs_path).await;
    stream.write_all(&banner).await?;
    stream.write_all(COPYRIGHT_LINES).await?;

    session
        .prompt_for_name()
        .expect("connecting -> identifying");

    loop {
        stream.write_all(NAME_PROMPT).await?;
        stream.flush().await?;
        let Some(typed) = read_telnet_line(stream, &mut pushback, EchoMode::Visible).await? else {
            return Ok(()); // EOF before name typed
        };

        let outcome =
            session_flow::name_typed(&mut session, typed.trim(), user_repo, SystemTime::now())
                .expect("session is in identifying");
        match outcome {
            NameTypedOutcome::Authenticated => break,
            NameTypedOutcome::NotFound => {
                stream.write_all(UNKNOWN_USER_LINE).await?;
            }
            NameTypedOutcome::NewUserRejected => {
                stream.write_all(NEW_NOT_SUPPORTED_LINE).await?;
            }
            NameTypedOutcome::SessionEnded => {
                stream.write_all(TOO_MANY_RETRIES_LINE).await?;
                stream.flush().await?;
                return Ok(());
            }
        }
    }

    // Password verification with retry, lockout and caller-log
    // (Slices 10 and 11).
    loop {
        stream.write_all(PASSWORD_PROMPT).await?;
        stream.flush().await?;
        let Some(password) = read_telnet_line(stream, &mut pushback, EchoMode::Masked).await?
        else {
            return Ok(()); // EOF before password typed
        };

        let outcome = session_flow::verify_password(
            &mut session,
            password.trim(),
            user_repo,
            hasher,
            caller_log,
            max_password_failures,
            SystemTime::now(),
        )
        .expect("session is in authenticating with a user");
        match outcome {
            VerifyPasswordOutcome::Authenticated => {
                stream.write_all(AUTHENTICATED_LINE).await?;
                stream.flush().await?;
                break;
            }
            VerifyPasswordOutcome::NotMatching => {
                stream.write_all(WRONG_PASSWORD_LINE).await?;
                stream.flush().await?;
                continue;
            }
            VerifyPasswordOutcome::AccountLocked => {
                stream.write_all(b"Account locked. Goodbye.\r\n").await?;
                stream.flush().await?;
                return Ok(());
            }
            VerifyPasswordOutcome::TooManyFailures => {
                stream
                    .write_all(b"Too many password failures. Goodbye.\r\n")
                    .await?;
                stream.flush().await?;
                return Ok(());
            }
        }
    }

    // EnterMenu (Slice 12): increment user.times_called, transition
    // to Menu, write the logon caller-log line and display the menu.
    session_flow::enter_menu(&mut session, user_repo, caller_log, SystemTime::now())
        .expect("session is in onboarded with a user");

    // Menu loop (Slice 13). Phase 1 only implements `G` (goodbye).
    // Future slices add the rest of the legacy AmiExpress menu.
    loop {
        let menu = read_default_menu(bbs_path).await;
        stream.write_all(&menu).await?;
        stream.write_all(MENU_PROMPT).await?;
        stream.flush().await?;
        let Some(line) = read_telnet_line(stream, &mut pushback, EchoMode::Visible).await? else {
            return Ok(());
        };
        let cmd = line.trim().to_ascii_uppercase();
        if cmd == "G" {
            session.user_requests_logoff().expect("session is in menu");
            session_flow::finalise_logoff(&mut session, user_repo, caller_log, SystemTime::now())
                .expect("session is in logging_off");
            stream.write_all(GOODBYE_LINE).await?;
            stream.flush().await?;
            return Ok(());
        }
        stream.write_all(UNKNOWN_COMMAND_LINE).await?;
    }
}

/// How [`read_telnet_line`] should echo the bytes it accepts.
///
/// Because the listener advertises `IAC WILL ECHO` to the client at
/// connect time, well-behaved clients (SyncTerm, PuTTY, telnet(1))
/// suppress their local echo and rely on the server to reflect typed
/// characters. Mirrors the original AmiExpress behaviour:
/// - [`Visible`][Self::Visible] for ordinary line input
///   (`amiexpress/express.e:2342` echoes the typed char in `lineInput`).
/// - [`Masked`][Self::Masked] at the password prompt
///   (`amiexpress/express.e:1543` sends `*` over the wire instead of
///   the typed character in `getPass2`).
///
/// In both modes a single byte (`0x08` BS, `0x7F` DEL) is treated as
/// "delete the previous character" and echoed as `<BS><SPACE><BS>`,
/// the classic terminal triplet that erases one position in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EchoMode {
    /// Echo each accepted byte back to the client verbatim.
    Visible,
    /// Echo `*` instead of the accepted byte. Used at the password
    /// prompt so passwords don't appear on the user's terminal.
    Masked,
}

/// Reads one line of input from `stream`, stripping IAC sequences and
/// echoing typed bytes back to the client according to `echo`.
///
/// `pushback` is a one-byte slot owned by the caller and reused across
/// consecutive prompts. It lets us look at the byte that follows a CR
/// without committing to consuming it: if it turns out not to be the
/// expected LF/NUL trailer, we stash it in `pushback` so the next
/// invocation of this function sees it as the first byte of input.
/// Without this, a SyncTerm-style client that sends a bare CR for
/// `<Enter>` would force the user to press Enter twice (we'd block
/// waiting for a trailer that never arrives).
///
/// Returns `Ok(Some(line))` on success, `Ok(None)` on EOF before any
/// terminator was seen.
async fn read_telnet_line(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    echo: EchoMode,
) -> io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(64);
    loop {
        let Some(b) = read_one(stream, pushback).await? else {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
            };
        };
        match b {
            0xFF => {
                // IAC. Consume the command (and option byte for the
                // 3-byte negotiations).
                let Some(cmd) = read_one(stream, pushback).await? else {
                    return Ok(None);
                };
                if (0xFB..=0xFE).contains(&cmd) {
                    // WILL / WONT / DO / DONT — one option byte follows.
                    let _ = read_one(stream, pushback).await?;
                } else if cmd == 0xFA {
                    // SB ... IAC SE; consume until SE.
                    loop {
                        let Some(b1) = read_one(stream, pushback).await? else {
                            return Ok(None);
                        };
                        if b1 == 0xFF {
                            let Some(b2) = read_one(stream, pushback).await? else {
                                return Ok(None);
                            };
                            if b2 == 0xF0 {
                                break;
                            }
                        }
                    }
                }
            }
            b'\r' => {
                // RFC 854 says the network virtual-terminal newline
                // is CR+LF; RFC 1123 §3.3.1 also accepts CR+NUL;
                // SyncTerm and friends send a bare CR. Try to peek
                // the next byte non-blockingly: if it's an LF or NUL
                // trailer, swallow it; otherwise push it back so the
                // next prompt's `read_telnet_line` sees it.
                try_consume_cr_trailer(stream, pushback)?;
                stream.write_all(b"\r\n").await?;
                return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
            }
            b'\n' => {
                stream.write_all(b"\r\n").await?;
                return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
            }
            0x08 | 0x7F => {
                // Backspace / DEL: drop the previous byte if any and
                // erase one column on the user's terminal with the
                // classic <BS><SPACE><BS> triplet.
                if buf.pop().is_some() {
                    stream.write_all(b"\x08 \x08").await?;
                }
            }
            b if b >= 0x20 => {
                buf.push(b);
                let echoed = match echo {
                    EchoMode::Visible => b,
                    EchoMode::Masked => b'*',
                };
                stream.write_all(&[echoed]).await?;
            }
            // Other control bytes (Ctrl-* etc.): silently ignored,
            // matching `lineInput`'s `IF (ch>31)` guard.
            _ => {}
        }
    }
}

/// Returns one byte from `pushback` if any, otherwise blocks reading
/// from `stream`. `Ok(None)` means EOF.
async fn read_one(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<Option<u8>> {
    if let Some(b) = pushback.take() {
        return Ok(Some(b));
    }
    let mut byte = [0u8; 1];
    let n = stream.read(&mut byte).await?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(byte[0]))
    }
}

/// Inspects the next available byte non-blockingly. If it's `<LF>` or
/// `<NUL>` (the two canonical CR trailers per RFC 854 / RFC 1123), it
/// is consumed. If it's anything else (or there's nothing queued), it
/// is left for a subsequent read — non-trailer bytes are stashed in
/// `pushback` so they aren't lost.
fn try_consume_cr_trailer(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<()> {
    if let Some(b) = pushback.take() {
        if b != b'\n' && b != 0 {
            *pushback = Some(b);
        }
        return Ok(());
    }
    let mut byte = [0u8; 1];
    match stream.try_read(&mut byte) {
        Ok(0) => {} // EOF
        Ok(_) => {
            if byte[0] != b'\n' && byte[0] != 0 {
                *pushback = Some(byte[0]);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

/// Reads `bbs_path/Screens/BBSTITLE.txt`, falling back to the built-in
/// banner if the file is missing or unreadable. Amiga `\b\n` line
/// endings in the file are translated to telnet `\r\n` so files
/// authored on the original system render correctly on
/// Linux / macOS / Windows clients.
async fn read_banner(bbs_path: &Path) -> Vec<u8> {
    let path = bbs_path.join("Screens").join("BBSTITLE.txt");
    match tokio::fs::read(&path).await {
        Ok(bytes) => translate_amiga_line_endings(&bytes),
        Err(_) => FALLBACK_BANNER.to_vec(),
    }
}

/// Reads `bbs_path/Conf02/Menu.txt`, normalising the Amiga-era `\b\n`
/// line terminator to telnet `\r\n`. Falls back to a built-in menu if
/// the file is missing or unreadable.
async fn read_default_menu(bbs_path: &Path) -> Vec<u8> {
    let path = bbs_path.join("Conf02").join("Menu.txt");
    let raw = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return FALLBACK_MENU.to_vec(),
    };
    translate_amiga_line_endings(&raw)
}

/// Replaces the Amiga `\b\n` (BS+LF) sequence with the telnet `\r\n`
/// (CR+LF). Other bytes — including ANSI escapes — pass through.
fn translate_amiga_line_endings(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() && input[i] == 0x08 && input[i + 1] == b'\n' {
            out.push(b'\r');
            out.push(b'\n');
            i += 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::domain::node::NodeStatus;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::User;

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
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                repo,
                test_hasher(),
                test_caller_log(),
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
        let _ = drain_until(&mut stream, b"Password: ").await;
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        assert!(
            contains(&buf, b"Password: "),
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        assert!(
            contains(&buf, b"Password: "),
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        assert!(
            contains(&buf, b"Password: "),
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        assert!(
            contains(&buf, b"Password: "),
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        let after_alice = find_subslice(&buf, b"alice")
            .map(|i| i + b"alice".len())
            .expect("'alice' echo present");
        let pwd_index = find_subslice(&buf, b"Password: ").expect("Password prompt present");
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
        let buf = drain_until(&mut stream, b"Password: ").await;
        assert!(
            contains(&buf, b"Password: "),
            "expected password prompt: {buf:?}"
        );
    }

    #[tokio::test]
    async fn correct_password_authenticates_user() {
        let addr = spawn_listener_with(repo_with(alice_with_password("secret"))).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, b"Password: ").await;
        stream.write_all(b"secret\r\n").await.unwrap();
        let buf = drain_until(&mut stream, b"Authenticated").await;
        assert!(
            contains(&buf, b"Authenticated"),
            "expected Authenticated line: {buf:?}"
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
    async fn new_keyword_is_rejected_and_re_prompts() {
        let addr = spawn_listener_with(repo_with_alice()).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"NEW\r\n").await.unwrap();
        // Read until we see BOTH the rejection and a fresh prompt.
        // (TCP may merge them into one read or split across reads.)
        let buf = drain_until_both(&mut stream, b"not yet supported", b"Enter your Name: ").await;
        assert!(
            contains(&buf, b"not yet supported"),
            "expected NEW rejection: {buf:?}"
        );
        assert!(
            contains(&buf, b"Enter your Name: "),
            "expected re-prompt: {buf:?}"
        );
    }

    #[test]
    fn translate_amiga_line_endings_replaces_bs_lf() {
        assert_eq!(translate_amiga_line_endings(b"foo\x08\nbar"), b"foo\r\nbar");
    }

    #[test]
    fn translate_amiga_line_endings_preserves_ansi_escapes() {
        let ansi = b"\x1b[31mRED\x1b[0m\x08\n";
        assert_eq!(
            translate_amiga_line_endings(ansi),
            b"\x1b[31mRED\x1b[0m\r\n"
        );
    }

    #[test]
    fn translate_amiga_line_endings_leaves_other_bytes_alone() {
        assert_eq!(translate_amiga_line_endings(b"hello"), b"hello");
        assert_eq!(translate_amiga_line_endings(b"a\x08b"), b"a\x08b");
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
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                config,
                Arc::new(InMemoryUserRepository::new(vec![alice_with_password(
                    "secret",
                )])),
                test_hasher(),
                log.clone() as SharedCallerLog,
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
        let _ = drain_until(&mut stream, b"Password: ").await;
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
                Ok(0) => break,
                Ok(n) => tail.extend_from_slice(&chunk[..n]),
                Err(_) => break,
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
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(
            dir.path().join("Conf02").join("Menu.txt"),
            b"MENU CONTENT\x08\n",
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
                Arc::new(InMemoryUserRepository::new(vec![alice_with_password(
                    "secret",
                )])),
                test_hasher(),
                test_caller_log(),
            )
            .await
            .unwrap(),
        );
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.run().await });

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let _ = drain_until(&mut stream, b"Enter your Name: ").await;
        stream.write_all(b"alice\r\n").await.unwrap();
        let _ = drain_until(&mut stream, b"Password: ").await;
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
    /// accumulated buffer (or EOF).
    async fn drain_until_both(stream: &mut TcpStream, a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 256];
        for _ in 0..200 {
            let n = stream.read(&mut chunk).await.unwrap_or(0);
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
}
