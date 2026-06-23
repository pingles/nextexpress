//! Cross-cutting wire/onboarding primitives shared across the BBS
//! workflow.
//!
//! Per-command and per-flow user-facing text now lives beside the module
//! that emits it; what remains here is the small set of genuinely
//! cross-cutting primitives — the line terminator, the connect-time ANSI
//! prompt, the onboarding-failure goodbye lines, and the shared
//! invalid-message-number notice — that no single command owns.
//!
//! Each constant's doc comment cross-references the legacy `AmiExpress`
//! source so spec-driven changes can be traced back to the original.

/// The telnet line terminator (`\r\n`) — the one newline primitive the
/// whole wire is built from. Standalone newline writes go through
/// [`MenuFlow::write_newline`](crate::app::menu_flow); this constant is
/// for composing it into larger byte sequences.
pub(crate) const CRLF: &[u8] = b"\r\n";

/// Prompt asking whether the user wants ANSI graphics, asked at connect
/// before the name prompt. Simplified from
/// `amiexpress/express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?` —
/// RIP is dropped, so the choice collapses to ANSI (default) vs. ASCII.
/// An answer beginning `n`/`N` selects ASCII and turns the terminal's
/// live colour mode off, so subsequent screens render with ANSI SGR
/// stripped.
pub(crate) const ANSI_PROMPT: &[u8] = b"ANSI Graphics (Y/n)? ";

/// Sent immediately before the connection closes on idle timeout.
pub(crate) const IDLE_TIMEOUT_LINE: &[u8] = b"Idle timeout. Goodbye.\r\n";

/// Sent when the post-auth cluster rejects the logon for insufficient
/// access.
pub(crate) const LOGON_REJECTED_LINE: &[u8] = b"Logon rejected. Goodbye.\r\n";

/// Sent when `R <something>` cannot be parsed as a message number.
pub(crate) const INVALID_MESSAGE_NUMBER_LINE: &[u8] = b"\r\nInvalid message number.\r\n";
