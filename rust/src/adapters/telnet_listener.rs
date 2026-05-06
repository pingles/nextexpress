//! Telnet listener and per-session task (Slice 8 / Slice 9).
//!
//! Boots a [`tokio::net::TcpListener`], allocates a node from the
//! application [`NodePool`] for every accepted connection, invokes the
//! `AcceptConnection`, `PromptForName` and `NameTyped` rules.
//! Subsequent slices extend the per-session task with the
//! `VerifyPassword` and menu rules.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use std::collections::BTreeSet;

use crate::app::config::Config;
use crate::app::node_pool::NodePool;
use crate::app::screens::ScreenRepository;
use crate::app::session_flow::{self, DefaultRatio, NewUserGateConfig, NewUserProfile};
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::PasswordHasher;
use crate::domain::session::{
    LogonChannel, NameTypedOutcome, NewUserPasswordOutcome, Session, SessionPolicy, SessionState,
    VerifyPasswordOutcome,
};
use crate::domain::user_repository::{NameLookupResult, UserRepository};

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

/// Prompt asking a registering user for the handle they want.
/// Mirrors the wire format of [`NAME_PROMPT`] (CRLF prefix, trailing
/// space) — `amiexpress/express.e:30141`.
const REGISTRATION_HANDLE_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Sent when the typed handle is `NEW` (reserved) or already taken
/// during registration. Followed by a fresh handle prompt.
const HANDLE_TAKEN_LINE: &[u8] = b"That name is taken. Try another.\r\n";

/// Sent when the user has burned through five handle retries during
/// registration.
const REGISTRATION_RETRIES_EXHAUSTED_LINE: &[u8] =
    b"Too many failed registration attempts. Goodbye.\r\n";

/// Prompt for the user's location during registration. Verbatim from
/// `amiexpress/express.e:30194`.
const LOCATION_PROMPT: &[u8] = b"City, State: ";

/// Prompt for the user's phone number during registration. Verbatim
/// from `amiexpress/express.e:30204`.
const PHONE_PROMPT: &[u8] = b"Phone Number: ";

/// Prompt for the user's email address during registration. Verbatim
/// from `amiexpress/express.e:30215`.
const EMAIL_PROMPT: &[u8] = b"E-Mail Address: ";

/// First password prompt during registration. Verbatim from
/// `amiexpress/express.e:30227`.
const REGISTRATION_PASSWORD_PROMPT: &[u8] = b"Enter a PassWord: ";

/// Confirmation password prompt during registration. Verbatim from
/// `amiexpress/express.e:30233`.
const REGISTRATION_PASSWORD_CONFIRM_PROMPT: &[u8] = b"Reenter the PassWord: ";

/// Sent when the two registration passwords don't match. Verbatim from
/// `amiexpress/express.e:30237`.
const PASSWORDS_DO_NOT_MATCH_LINE: &[u8] = b"\r\nPasswords do not match, try again..\r\n";

/// Prompt asking the user for their preferred line length. Simplified
/// from `amiexpress/express.e:11307` (which streams a 70..2 ladder
/// before asking).
const LINE_LENGTH_PROMPT: &[u8] = b"Enter line length (or 0 for Auto): ";

/// Sent when the line-length input doesn't parse as a number in
/// `0..=255`.
const INVALID_LINE_LENGTH_LINE: &[u8] = b"Invalid line length.\r\n";

/// Prompt asking whether the user wants ANSI graphics. Simplified from
/// `amiexpress/express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?`
/// — RIP rendering lands in a future toggles slice.
const ANSI_PROMPT: &[u8] = b"Use ANSI graphics? (Y/n) ";

/// Sent after the registration succeeds; immediately followed by the
/// menu sequence inherited by every authenticated session.
const REGISTRATION_COMPLETE_LINE: &[u8] = b"\r\nWelcome aboard!\r\n";

/// Prompt for the sysop-set new-user password gate. Verbatim from
/// `amiexpress/express.e:30018`.
const NEW_USER_PASSWORD_PROMPT: &[u8] = b"Enter New User Password: ";

/// Sent after each failed new-user password attempt. Verbatim from
/// `amiexpress/express.e:30036`.
const NEW_USER_INVALID_PASSWORD_LINE: &[u8] = b"Invalid PassWord\r\n";

/// Sent when the gate's retry budget is exhausted. Verbatim from
/// `amiexpress/express.e:30039`. Followed by a goodbye line.
const NEW_USER_EXCESSIVE_FAILURES_LINE: &[u8] = b"\r\nExcessive Password Failure\r\nGoodbye.\r\n";

/// Sent on a successful gate match. Verbatim from
/// `amiexpress/express.e:30046`.
const NEW_USER_PASSWORD_OK_LINE: &[u8] = b"Correct\r\n";

/// Sent when the user has burned through all five name retries.
const TOO_MANY_RETRIES_LINE: &[u8] = b"Too many failed login attempts. Goodbye.\r\n";

/// Prompt for the user's password.
const PASSWORD_PROMPT: &[u8] = b"PassWord: ";

/// Sent after a successful authentication. The conference menu lands
/// in Slice 12; until then we acknowledge and idle.
const AUTHENTICATED_LINE: &[u8] = b"Authenticated.\r\n";

/// Sent when the password didn't match. Slice 11 elaborates this with
/// retry counts and lockout; for now we just inform and close.
const WRONG_PASSWORD_LINE: &[u8] = b"Incorrect password.\r\n";

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

/// Type alias for the screen repository the listener reads from.
type SharedScreens = Arc<dyn ScreenRepository + Send + Sync + 'static>;

/// Telnet listener and the application state it drives.
pub struct TelnetListener {
    listener: TcpListener,
    pool: Arc<NodePool>,
    session_policy: SessionPolicy,
    default_ratio: DefaultRatio,
    new_user_gate: NewUserGateConfig,
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
    screens: SharedScreens,
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
        let screens: SharedScreens = Arc::new(FileScreenRepository::new(config.bbs_path.clone()));
        Self::bind_with_screens(addr, config, user_repo, hasher, caller_log, screens).await
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
        Ok(Self {
            listener,
            pool,
            session_policy: config.session_policy(),
            default_ratio,
            new_user_gate,
            user_repo,
            hasher,
            caller_log,
            screens,
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
            let repo = self.user_repo.clone();
            let hasher = self.hasher.clone();
            let log = self.caller_log.clone();
            let screens = self.screens.clone();
            let session_policy = self.session_policy;
            let default_ratio = self.default_ratio;
            let new_user_gate = self.new_user_gate.clone();
            tokio::spawn(async move {
                let _ = handle_connection(
                    stream,
                    pool,
                    repo,
                    hasher,
                    log,
                    screens,
                    session_policy,
                    default_ratio,
                    new_user_gate,
                )
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
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
    screens: SharedScreens,
    session_policy: SessionPolicy,
    default_ratio: DefaultRatio,
    new_user_gate: NewUserGateConfig,
) -> io::Result<()> {
    let Some(node_number) = pool.allocate().await else {
        stream.write_all(BUSY_LINE).await?;
        stream.flush().await?;
        return Ok(());
    };

    let result = run_session(
        &mut stream,
        node_number,
        user_repo.as_ref(),
        hasher.as_ref(),
        caller_log.as_ref(),
        screens.as_ref(),
        session_policy,
        default_ratio,
        &new_user_gate,
    )
    .await;
    let _ = pool.release(node_number).await;
    result
}

/// Outcome of a bounded read in a per-session loop.
enum ReadOutcome {
    /// A line was received from the client.
    Line(String),
    /// The peer closed the connection cleanly without sending a line.
    Eof,
    /// `core/config.input_timeout` elapsed without input
    /// (`session.allium:IdleTimeout`, Slice 17).
    IdleTimedOut,
}

/// Reads a single line from `stream`, bounded by `timeout`. On a
/// successful read updates `session.last_input_at` to satisfy the
/// "Telnet adapter resets last_input_at on every input chunk" wire
/// obligation (Slice 17, see `SLICES.md` adapter checklist).
async fn read_line_with_idle_timeout(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    echo: EchoMode,
    timeout: Duration,
    session: &mut Session,
) -> io::Result<ReadOutcome> {
    match tokio::time::timeout(timeout, read_telnet_line(stream, pushback, echo)).await {
        Ok(result) => match result? {
            Some(line) => {
                session.record_input(SystemTime::now());
                Ok(ReadOutcome::Line(line))
            }
            None => Ok(ReadOutcome::Eof),
        },
        Err(_elapsed) => Ok(ReadOutcome::IdleTimedOut),
    }
}

/// Sent immediately before the connection closes on idle timeout.
const IDLE_TIMEOUT_LINE: &[u8] = b"Idle timeout. Goodbye.\r\n";

/// Applies `session.allium:IdleTimeout` (Slice 17), finalises the
/// session, writes the goodbye line, and flushes the socket. The
/// per-session loop then returns. The rule's `treat_timeout_as_logoff`
/// branch is selected by the configured [`SessionPolicy`].
async fn handle_idle_timeout(
    stream: &mut TcpStream,
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    session_policy: SessionPolicy,
) -> io::Result<()> {
    session
        .apply_idle_timeout(session_policy.treat_timeout_as_logoff())
        .expect("idle-permitted state when read times out");
    // FinaliseLogoff handles unauthenticated sessions too — it
    // skips the user mutation and emits a "?" handle in the log
    // line, which is what the spec's
    // `FinaliseUnauthenticatedLogoff` rule prescribes for the
    // bare-state case.
    session_flow::finalise_logoff(session, user_repo, caller_log, SystemTime::now())
        .expect("logging_off can finalise");
    stream.write_all(IDLE_TIMEOUT_LINE).await?;
    stream.flush().await?;
    Ok(())
}

/// Applies `session.allium:CarrierLost` (Slice 18) when the peer has
/// closed the TCP connection mid-prompt, then finalises the session.
/// No goodbye line is written — the connection is already gone.
fn handle_carrier_loss(
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
) {
    session
        .apply_carrier_loss()
        .expect("carrier-permitted state when peer closes");
    session_flow::finalise_logoff(session, user_repo, caller_log, SystemTime::now())
        .expect("logging_off can finalise");
}

/// Maximum handle attempts during registration before the session
/// bails. Mirrors the original AmiExpress `doNewUser` retry budget at
/// `amiexpress/express.e:30150`.
const MAX_REGISTRATION_HANDLE_ATTEMPTS: u32 = 5;

/// Writes `prompt`, reads one line, and either returns `Some(line)` or
/// finalises the session (idle / carrier loss) and returns `None`. The
/// helper keeps the registration sub-flow readable: every prompt has
/// the same idle-timeout and EOF semantics as the rest of the listener.
#[allow(clippy::too_many_arguments)]
async fn prompt_for_line(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    prompt: &[u8],
    echo: EchoMode,
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    session_policy: SessionPolicy,
) -> io::Result<Option<String>> {
    stream.write_all(prompt).await?;
    stream.flush().await?;
    match read_line_with_idle_timeout(
        stream,
        pushback,
        echo,
        session_policy.input_timeout(),
        session,
    )
    .await?
    {
        ReadOutcome::Line(line) => Ok(Some(line)),
        ReadOutcome::Eof => {
            handle_carrier_loss(session, user_repo, caller_log);
            Ok(None)
        }
        ReadOutcome::IdleTimedOut => {
            handle_idle_timeout(stream, session, user_repo, caller_log, session_policy).await?;
            Ok(None)
        }
    }
}

/// Drives the new-user registration sub-flow
/// (`session.allium:CompleteNewUserRegistration`, Slice 20).
///
/// Displays the NEWUSERPW screen, runs the new-user password gate
/// (`session.allium:VerifyNewUserPassword`, Slice 20a) when
/// `password_required` is `true`, then collects handle, location,
/// phone, email, password (with confirmation), line length and ANSI
/// preference. On success the session has reached
/// [`SessionState::Onboarded`] and the function returns `Ok(true)` so
/// the caller falls through into the menu loop. On failure
/// (idle / carrier / repeated handle collisions / gate exhausted)
/// the function ends the session inline and returns `Ok(false)`.
#[allow(clippy::too_many_arguments)]
async fn run_new_user_registration(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    hasher: &(dyn PasswordHasher + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    screens: &(dyn ScreenRepository + Send + Sync),
    session_policy: SessionPolicy,
    default_ratio: DefaultRatio,
    new_user_gate: &NewUserGateConfig,
    password_required: bool,
) -> io::Result<bool> {
    let screen = screens.new_user_password().await;
    stream.write_all(&screen).await?;

    if password_required
        && !run_new_user_password_gate(
            stream,
            pushback,
            session,
            user_repo,
            caller_log,
            session_policy,
            new_user_gate,
        )
        .await?
    {
        return Ok(false);
    }

    let mut attempts: u32 = 0;
    let handle = loop {
        if attempts >= MAX_REGISTRATION_HANDLE_ATTEMPTS {
            stream
                .write_all(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                .await?;
            stream.flush().await?;
            handle_carrier_loss(session, user_repo, caller_log);
            return Ok(false);
        }
        let Some(typed) = prompt_for_line(
            stream,
            pushback,
            REGISTRATION_HANDLE_PROMPT,
            EchoMode::Visible,
            session,
            user_repo,
            caller_log,
            session_policy,
        )
        .await?
        else {
            return Ok(false);
        };
        let trimmed = typed.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NEW") {
            stream.write_all(HANDLE_TAKEN_LINE).await?;
            attempts += 1;
            continue;
        }
        match user_repo.find_by_handle(trimmed) {
            NameLookupResult::Found(_) | NameLookupResult::UserTypedNew => {
                stream.write_all(HANDLE_TAKEN_LINE).await?;
                attempts += 1;
                continue;
            }
            NameLookupResult::NotFound => break trimmed.to_string(),
        }
    };

    let location = read_optional_field(
        stream,
        pushback,
        LOCATION_PROMPT,
        session,
        user_repo,
        caller_log,
        session_policy,
    )
    .await?;
    let location = match location {
        Some(value) => value,
        None => return Ok(false),
    };

    let phone_number = read_optional_field(
        stream,
        pushback,
        PHONE_PROMPT,
        session,
        user_repo,
        caller_log,
        session_policy,
    )
    .await?;
    let phone_number = match phone_number {
        Some(value) => value,
        None => return Ok(false),
    };

    let email = read_optional_field(
        stream,
        pushback,
        EMAIL_PROMPT,
        session,
        user_repo,
        caller_log,
        session_policy,
    )
    .await?;
    let email = match email {
        Some(value) => value,
        None => return Ok(false),
    };

    let password = loop {
        let Some(p1) = prompt_for_line(
            stream,
            pushback,
            REGISTRATION_PASSWORD_PROMPT,
            EchoMode::Masked,
            session,
            user_repo,
            caller_log,
            session_policy,
        )
        .await?
        else {
            return Ok(false);
        };
        if p1.trim().is_empty() {
            // Empty password loops back without bothering with the
            // confirmation step. Mirrors the legacy `JUMP jLoop4`
            // behaviour at `amiexpress/express.e:30230`.
            continue;
        }
        let Some(p2) = prompt_for_line(
            stream,
            pushback,
            REGISTRATION_PASSWORD_CONFIRM_PROMPT,
            EchoMode::Masked,
            session,
            user_repo,
            caller_log,
            session_policy,
        )
        .await?
        else {
            return Ok(false);
        };
        if p1 == p2 {
            break p1;
        }
        stream.write_all(PASSWORDS_DO_NOT_MATCH_LINE).await?;
    };

    let line_length = loop {
        let Some(typed) = prompt_for_line(
            stream,
            pushback,
            LINE_LENGTH_PROMPT,
            EchoMode::Visible,
            session,
            user_repo,
            caller_log,
            session_policy,
        )
        .await?
        else {
            return Ok(false);
        };
        let trimmed = typed.trim();
        if trimmed.is_empty() {
            break 0;
        }
        match trimmed.parse::<u32>() {
            Ok(value) if value <= 255 => break value,
            _ => {
                stream.write_all(INVALID_LINE_LENGTH_LINE).await?;
            }
        }
    };

    let Some(ansi_typed) = prompt_for_line(
        stream,
        pushback,
        ANSI_PROMPT,
        EchoMode::Visible,
        session,
        user_repo,
        caller_log,
        session_policy,
    )
    .await?
    else {
        return Ok(false);
    };
    // Default ANSI on unless the user explicitly says No (mirrors
    // `amiexpress/express.e:29528`'s default to ANSI on).
    let ansi_colour = !ansi_typed.trim().eq_ignore_ascii_case("N");

    let profile = NewUserProfile {
        handle,
        location,
        phone_number,
        email,
        password,
        line_length,
        ansi_colour,
        flags: BTreeSet::new(),
    };
    if session_flow::complete_new_user_registration(
        session,
        profile,
        user_repo,
        hasher,
        caller_log,
        default_ratio,
        session_policy,
        SystemTime::now(),
    )
    .is_err()
    {
        // The handle-collision path is filtered above; any remaining
        // failure (hasher fault, repository fault) bails the session.
        stream
            .write_all(REGISTRATION_RETRIES_EXHAUSTED_LINE)
            .await?;
        stream.flush().await?;
        handle_carrier_loss(session, user_repo, caller_log);
        return Ok(false);
    }
    stream.write_all(REGISTRATION_COMPLETE_LINE).await?;
    stream.flush().await?;
    Ok(true)
}

/// Drives the new-user password gate
/// (`session.allium:VerifyNewUserPassword`). Returns `Ok(true)` when
/// the gate has passed and the registration form should be offered,
/// `Ok(false)` when the session has been ended (idle, carrier,
/// retry budget exhausted) and the caller should bow out.
#[allow(clippy::too_many_arguments)]
async fn run_new_user_password_gate(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    session_policy: SessionPolicy,
    new_user_gate: &NewUserGateConfig,
) -> io::Result<bool> {
    loop {
        let Some(typed) = prompt_for_line(
            stream,
            pushback,
            NEW_USER_PASSWORD_PROMPT,
            EchoMode::Masked,
            session,
            user_repo,
            caller_log,
            session_policy,
        )
        .await?
        else {
            return Ok(false);
        };
        let outcome = session_flow::verify_new_user_password(
            session,
            typed.trim(),
            new_user_gate,
            caller_log,
            SystemTime::now(),
        )
        .expect("session is in new_user_registering and gate is configured");
        match outcome {
            NewUserPasswordOutcome::Verified => {
                stream.write_all(NEW_USER_PASSWORD_OK_LINE).await?;
                stream.flush().await?;
                return Ok(true);
            }
            NewUserPasswordOutcome::Mismatch => {
                stream.write_all(NEW_USER_INVALID_PASSWORD_LINE).await?;
                stream.flush().await?;
            }
            NewUserPasswordOutcome::TooManyFailures => {
                stream.write_all(NEW_USER_EXCESSIVE_FAILURES_LINE).await?;
                stream.flush().await?;
                // Session already in LoggingOff via the rule. Finalise.
                session_flow::finalise_logoff(session, user_repo, caller_log, SystemTime::now())
                    .expect("session is in logging_off");
                return Ok(false);
            }
        }
    }
}

/// Reads one optional registration field (location, phone, email).
/// Returns `Some(None)` for an empty input, `Some(Some(value))` for a
/// non-empty input, and `None` when the session has been ended by an
/// idle timeout or carrier loss.
#[allow(clippy::too_many_arguments)]
async fn read_optional_field(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    prompt: &[u8],
    session: &mut Session,
    user_repo: &(dyn UserRepository + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    session_policy: SessionPolicy,
) -> io::Result<Option<Option<String>>> {
    let Some(typed) = prompt_for_line(
        stream,
        pushback,
        prompt,
        EchoMode::Visible,
        session,
        user_repo,
        caller_log,
        session_policy,
    )
    .await?
    else {
        return Ok(None);
    };
    let trimmed = typed.trim();
    Ok(Some(if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }))
}

/// Inner per-session loop. Drives the session from the IAC handshake
/// through banner, name prompt, password and (in later slices) menu.
#[allow(clippy::too_many_arguments)]
async fn run_session(
    stream: &mut TcpStream,
    node_number: u32,
    user_repo: &(dyn UserRepository + Send + Sync),
    hasher: &(dyn PasswordHasher + Send + Sync),
    caller_log: &(dyn CallerLogAppender + Send + Sync),
    screens: &(dyn ScreenRepository + Send + Sync),
    session_policy: SessionPolicy,
    default_ratio: DefaultRatio,
    new_user_gate: &NewUserGateConfig,
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

    let banner = screens.banner().await;
    stream.write_all(&banner).await?;
    stream.write_all(COPYRIGHT_LINES).await?;

    session
        .prompt_for_name()
        .expect("connecting -> identifying");

    loop {
        stream.write_all(NAME_PROMPT).await?;
        stream.flush().await?;
        let typed = match read_line_with_idle_timeout(
            stream,
            &mut pushback,
            EchoMode::Visible,
            session_policy.input_timeout(),
            &mut session,
        )
        .await?
        {
            ReadOutcome::Line(line) => line,
            ReadOutcome::Eof => {
                handle_carrier_loss(&mut session, user_repo, caller_log);
                return Ok(()); // EOF before name typed
            }
            ReadOutcome::IdleTimedOut => {
                handle_idle_timeout(stream, &mut session, user_repo, caller_log, session_policy)
                    .await?;
                return Ok(());
            }
        };

        let outcome = session_flow::name_typed(
            &mut session,
            typed.trim(),
            user_repo,
            new_user_gate,
            SystemTime::now(),
        )
        .expect("session is in identifying");
        match outcome {
            NameTypedOutcome::Authenticated => break,
            NameTypedOutcome::NotFound => {
                stream.write_all(UNKNOWN_USER_LINE).await?;
            }
            NameTypedOutcome::NewUserRegistering { password_required } => {
                if !run_new_user_registration(
                    stream,
                    &mut pushback,
                    &mut session,
                    user_repo,
                    hasher,
                    caller_log,
                    screens,
                    session_policy,
                    default_ratio,
                    new_user_gate,
                    password_required,
                )
                .await?
                {
                    return Ok(());
                }
                break;
            }
            NameTypedOutcome::NewUserRegistrationDisallowed => {
                // Session has already moved to LoggingOff via
                // RejectDisallowedRegistration. Render the NONEWUSERS
                // screen (or built-in fall-back), finalise, exit.
                let screen = screens.no_new_users().await;
                stream.write_all(&screen).await?;
                stream.flush().await?;
                session_flow::finalise_logoff(
                    &mut session,
                    user_repo,
                    caller_log,
                    SystemTime::now(),
                )
                .expect("session is in logging_off");
                return Ok(());
            }
            NameTypedOutcome::SessionEnded => {
                stream.write_all(TOO_MANY_RETRIES_LINE).await?;
                stream.flush().await?;
                return Ok(());
            }
        }
    }

    // Skip the password prompt for sessions that arrived via the
    // new-user registration sub-flow (already in Onboarded by the
    // time we get here).
    if session.state() == SessionState::Authenticating {
        // Password verification with retry, lockout and caller-log
        // (Slices 10 and 11).
        loop {
            stream.write_all(PASSWORD_PROMPT).await?;
            stream.flush().await?;
            let password = match read_line_with_idle_timeout(
                stream,
                &mut pushback,
                EchoMode::Masked,
                session_policy.input_timeout(),
                &mut session,
            )
            .await?
            {
                ReadOutcome::Line(line) => line,
                ReadOutcome::Eof => {
                    handle_carrier_loss(&mut session, user_repo, caller_log);
                    return Ok(()); // EOF before password typed
                }
                ReadOutcome::IdleTimedOut => {
                    handle_idle_timeout(
                        stream,
                        &mut session,
                        user_repo,
                        caller_log,
                        session_policy,
                    )
                    .await?;
                    return Ok(());
                }
            };

            let outcome = session_flow::verify_password(
                &mut session,
                password.trim(),
                user_repo,
                hasher,
                caller_log,
                session_policy,
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
                VerifyPasswordOutcome::LogonRejected => {
                    // RejectLockedOrInsufficientAccess (Slice 16): the
                    // session has already moved to LoggingOff with the
                    // appropriate reason; the caller log carries the
                    // spec's rejection entry. Greet the user and leave.
                    stream.write_all(b"Logon rejected. Goodbye.\r\n").await?;
                    stream.flush().await?;
                    return Ok(());
                }
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
        let menu = screens.default_menu().await;
        stream.write_all(&menu).await?;
        stream.write_all(MENU_PROMPT).await?;
        stream.flush().await?;
        let line = match read_line_with_idle_timeout(
            stream,
            &mut pushback,
            EchoMode::Visible,
            session_policy.input_timeout(),
            &mut session,
        )
        .await?
        {
            ReadOutcome::Line(line) => line,
            ReadOutcome::Eof => {
                handle_carrier_loss(&mut session, user_repo, caller_log);
                return Ok(());
            }
            ReadOutcome::IdleTimedOut => {
                handle_idle_timeout(stream, &mut session, user_repo, caller_log, session_policy)
                    .await?;
                return Ok(());
            }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
            TelnetListener::bind("127.0.0.1:0", config, repo, test_hasher(), shared_log)
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
    async fn full_new_user_registration_lands_in_menu() {
        // Slice 20: typing NEW, completing every prompt, lands the
        // session in the menu loop with the freshly created account.
        let log = Arc::new(InMemoryCallerLog::new());
        let repo: SharedUserRepo = Arc::new(InMemoryUserRepository::default());
        let listener = Arc::new(
            TelnetListener::bind(
                "127.0.0.1:0",
                test_config(1),
                repo.clone(),
                test_hasher(),
                log.clone() as SharedCallerLog,
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
        let buf = drain_until(&mut stream, b"Command: ").await;
        assert!(
            contains(&buf, b"Welcome aboard"),
            "expected registration-complete line: {buf:?}"
        );
        assert!(
            contains(&buf, b"Command: "),
            "expected to reach menu prompt: {buf:?}"
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
            }
            other => panic!("expected newbie to be created, got {other:?}"),
        }

        // Logon caller-log entry recorded for the new user.
        let entries = log.entries();
        assert!(
            entries
                .iter()
                .any(|e| e.text.contains("Logon:") && e.text.contains("newbie")),
            "expected logon entry for new user: {entries:?}"
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
