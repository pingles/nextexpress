//! Wire-format byte constants the BBS workflow writes to terminals.
//!
//! Lifting these out of `session_driver` so:
//!   1. the workflow file shrinks to the orchestration that earns its
//!      keep, and
//!   2. future I18N / theming / ANSI-rendering work has a single place
//!      to plug.
//!
//! Each constant's doc comment cross-references the legacy `AmiExpress`
//! source so spec-driven changes can be traced back to the original.

/// Prompt sent before reading the user's handle. Mirrors the original
/// `AmiExpress` wire format: a CRLF prefix and trailing space around the
/// default `NAME_PROMPT` of `Enter your Name:` (see
/// `amiexpress/express.e:29571` and `:31774`).
pub(crate) const NAME_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's password.
pub(crate) const PASSWORD_PROMPT: &[u8] = b"PassWord: ";

/// Prompt asking a registering user for the handle they want.
/// Mirrors the wire format of [`NAME_PROMPT`] (CRLF prefix, trailing
/// space) — `amiexpress/express.e:30141`.
pub(crate) const REGISTRATION_HANDLE_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's location during registration. Verbatim from
/// `amiexpress/express.e:30194`.
pub(crate) const LOCATION_PROMPT: &[u8] = b"City, State: ";

/// Prompt for the user's phone number during registration. Verbatim
/// from `amiexpress/express.e:30204`.
pub(crate) const PHONE_PROMPT: &[u8] = b"Phone Number: ";

/// Prompt for the user's email address during registration. Verbatim
/// from `amiexpress/express.e:30215`.
pub(crate) const EMAIL_PROMPT: &[u8] = b"E-Mail Address: ";

/// First password prompt during registration. Verbatim from
/// `amiexpress/express.e:30227`.
pub(crate) const REGISTRATION_PASSWORD_PROMPT: &[u8] = b"Enter a PassWord: ";

/// Confirmation password prompt during registration. Verbatim from
/// `amiexpress/express.e:30233`.
pub(crate) const REGISTRATION_PASSWORD_CONFIRM_PROMPT: &[u8] = b"Reenter the PassWord: ";

/// Prompt asking the user for their preferred line length. Simplified
/// from `amiexpress/express.e:11307` (which streams a 70..2 ladder
/// before asking).
pub(crate) const LINE_LENGTH_PROMPT: &[u8] = b"Enter line length (or 0 for Auto): ";

/// Prompt asking whether the user wants ANSI graphics. Simplified from
/// `amiexpress/express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?`
/// — RIP rendering lands in a future toggles slice.
pub(crate) const ANSI_PROMPT: &[u8] = b"Use ANSI graphics? (Y/n) ";

/// Prompt for the sysop-set new-user password gate. Verbatim from
/// `amiexpress/express.e:30018`.
pub(crate) const NEW_USER_PASSWORD_PROMPT: &[u8] = b"Enter New User Password: ";

/// Prompt printed after each menu screen, awaiting a command.
pub(crate) const MENU_PROMPT: &[u8] = b"Command: ";

/// Two-line copyright block printed on every accepted connection,
/// directly after the BBS title banner. The `NextExpress` line sits
/// above the `AmiExpress` line to make the lineage obvious; the
/// `AmiExpress` line mirrors the original BBS's banner verbatim
/// (`amiexpress/express.e:25690`, modulo the legacy file's mojibake of
/// the © glyph).
pub(crate) const COPYRIGHT_LINES: &[u8] = concat!(
    "NextExpress ",
    env!("CARGO_PKG_VERSION"),
    " Copyright \u{00A9}2026\r\n",
    "AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n",
)
.as_bytes();

/// Sent after a not-found name lookup to invite a retry.
pub(crate) const UNKNOWN_USER_LINE: &[u8] = b"Unknown user.\r\n";

/// Sent when the typed handle is `NEW` (reserved) or already taken
/// during registration. Followed by a fresh handle prompt.
pub(crate) const HANDLE_TAKEN_LINE: &[u8] = b"That name is taken. Try another.\r\n";

/// Sent when the user has burned through five handle retries during
/// registration.
pub(crate) const REGISTRATION_RETRIES_EXHAUSTED_LINE: &[u8] =
    b"Too many failed registration attempts. Goodbye.\r\n";

/// Sent when the two registration passwords don't match. Verbatim from
/// `amiexpress/express.e:30237`.
pub(crate) const PASSWORDS_DO_NOT_MATCH_LINE: &[u8] =
    b"\r\nPasswords do not match, try again..\r\n";

/// Sent when the line-length input doesn't parse as a number in
/// `0..=255`.
pub(crate) const INVALID_LINE_LENGTH_LINE: &[u8] = b"Invalid line length.\r\n";

/// Sent after the registration succeeds; immediately followed by the
/// menu sequence inherited by every authenticated session.
pub(crate) const REGISTRATION_COMPLETE_LINE: &[u8] = b"\r\nWelcome aboard!\r\n";

/// Sent after each failed new-user password attempt. Verbatim from
/// `amiexpress/express.e:30036`.
pub(crate) const NEW_USER_INVALID_PASSWORD_LINE: &[u8] = b"Invalid PassWord\r\n";

/// Sent when the gate's retry budget is exhausted. Verbatim from
/// `amiexpress/express.e:30039`. Followed by a goodbye line.
pub(crate) const NEW_USER_EXCESSIVE_FAILURES_LINE: &[u8] =
    b"\r\nExcessive Password Failure\r\nGoodbye.\r\n";

/// Sent on a successful gate match. Verbatim from
/// `amiexpress/express.e:30046`.
pub(crate) const NEW_USER_PASSWORD_OK_LINE: &[u8] = b"Correct\r\n";

/// Sent when the user has burned through all five name retries.
pub(crate) const TOO_MANY_RETRIES_LINE: &[u8] = b"Too many failed login attempts. Goodbye.\r\n";

/// Sent after a successful authentication.
pub(crate) const AUTHENTICATED_LINE: &[u8] = b"Authenticated.\r\n";

/// Sent when the password didn't match.
pub(crate) const WRONG_PASSWORD_LINE: &[u8] = b"Incorrect password.\r\n";

/// Sent for unrecognised menu commands.
pub(crate) const UNKNOWN_COMMAND_LINE: &[u8] = b"Unknown command. Type G to log off.\r\n";

/// Sent immediately before the connection closes on a normal logoff.
pub(crate) const GOODBYE_LINE: &[u8] = b"Goodbye!\r\n";

/// Sent immediately before the connection closes on idle timeout.
pub(crate) const IDLE_TIMEOUT_LINE: &[u8] = b"Idle timeout. Goodbye.\r\n";

/// Sent when the post-auth cluster locks the account.
pub(crate) const ACCOUNT_LOCKED_LINE: &[u8] = b"Account locked. Goodbye.\r\n";

/// Sent when the per-session retry budget is exhausted at the password
/// prompt.
pub(crate) const TOO_MANY_PASSWORD_FAILURES_LINE: &[u8] =
    b"Too many password failures. Goodbye.\r\n";

/// Sent when the post-auth cluster rejects the logon for insufficient
/// access.
pub(crate) const LOGON_REJECTED_LINE: &[u8] = b"Logon rejected. Goodbye.\r\n";

/// Sent when the auto-rejoin / explicit-join flow can't find any
/// conference the user has access to (Slice 30 / Slice 34a). The
/// session terminates with `LogoffReason::NoConferenceAccess`.
pub(crate) const NO_CONFERENCE_ACCESS_LINE: &[u8] = b"\r\nNo accessible conferences. Goodbye.\r\n";

/// Sent when the user types `J <num>` for a conference they don't
/// have access to; the session falls through to the first accessible
/// conference but the listener surfaces this notice first
/// (`amiexpress/express.e:25157`).
pub(crate) const NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE: &[u8] =
    b"\r\nYou do not have access to the requested conference\r\n\r\n";

/// Sent when the user types `J` without a target conference number.
/// The simplified Phase-4 wiring rejects the no-arg form rather than
/// running the `JoinConf` prompt sub-flow; future slices may refine
/// this when the `JoinConf` prompt arrives.
pub(crate) const JOIN_REQUIRES_NUMBER_LINE: &[u8] = b"\r\nUsage: J <conference-number>\r\n";

/// Sent when `J <something>` cannot be parsed as a conference
/// number.
pub(crate) const INVALID_CONFERENCE_NUMBER_LINE: &[u8] = b"\r\nInvalid conference number.\r\n";
