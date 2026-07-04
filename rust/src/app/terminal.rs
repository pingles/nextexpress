//! `Terminal` port — the transport-agnostic line interface the BBS
//! workflow consumes.
//!
//! Driving adapters (telnet today, ssh / rlogin / ws in future slices)
//! implement this trait against their wire protocol. The workflow
//! deliberately knows only "write some bytes" and "read a line with an
//! echo policy and a timeout"; everything below this trait is the
//! adapter's concern.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Future returned by [`Terminal`] operations.
pub(crate) type TerminalFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

/// A cross-task signal delivered into a live session's terminal (July
/// 2026 review, item 26). A session parked at a prompt is suspended
/// inside its terminal read; the transport races that read against its
/// per-session signal channel so another task — Tier E's `OLM`/page
/// senders, later Tier G's kick/suspend — can reach it. Starts with
/// exactly one variant; Tier G adds `Kick` (surfaced as a synthetic
/// EOF so the existing carrier-loss teardown runs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSignal {
    /// Write these bytes to the session's terminal, then resume the
    /// interrupted read with its input buffer and echo state intact.
    Deliver(Vec<u8>),
}

/// Echo policy requested by the BBS workflow when reading a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalEcho {
    /// Echo typed characters as they are entered.
    Visible,
    /// Hide the original characters and render masking characters.
    Masked,
}

/// Result of a bounded line read from a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalRead {
    /// A complete input line was received.
    Line(String),
    /// The peer disconnected cleanly.
    Eof,
    /// No input was received before the supplied timeout elapsed.
    IdleTimedOut,
}

/// A single keystroke read from the terminal in hot-key mode
/// (slice D2b — the `AquaScan` pager prompts act per key, no Enter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyEvent {
    /// A printable ASCII key (0x20..=0x7E).
    Char(u8),
    /// Enter — CR with an optional LF/NUL trailer. A bare LF is NOT
    /// Enter: the board swallows it entirely (probe P2,
    /// `ae_tierd_probes.txt:140-175`), so the adapter emits no event.
    Enter,
    /// Backspace (0x08) or DEL (0x7F).
    Backspace,
    /// Ctrl-C (0x03) — the pager's `**Break` quit
    /// (`ae_tierd_help_audit.txt` PCC). Decoded distinctly so the raw
    /// byte reaches the verb table; the fake terminal cannot stand in
    /// for this mapping (the first PCC replay shipped Ctrl-C as
    /// `Other` and the listing resumed instead of breaking).
    CtrlC,
    /// Anything else: other control bytes, bytes ≥ 0x80, or one
    /// swallowed `ESC[…` sequence (an arrow press is ONE event, so it
    /// cannot fire three pager verbs).
    Other,
}

/// Result of a bounded single-key read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyRead {
    /// One keystroke arrived.
    Key(KeyEvent),
    /// The peer disconnected cleanly.
    Eof,
    /// No key arrived before the supplied timeout elapsed.
    IdleTimedOut,
}

/// Application-facing terminal port.
///
/// Transport adapters implement this with protocol-specific byte IO.
/// The driver deliberately asks for only terminal concepts: write
/// bytes, flush output, and read a line with an echo policy and
/// timeout.
pub(crate) trait Terminal {
    /// Error type returned by the concrete terminal adapter.
    type Error;

    /// Writes raw rendered BBS bytes to the terminal.
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error>;

    /// Flushes any buffered terminal output.
    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error>;

    /// Reads one line, applying the requested echo mode and input
    /// timeout.
    fn read_line(
        &mut self,
        echo: TerminalEcho,
        timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error>;

    /// Reads one keystroke in hot-key mode. The adapter echoes
    /// NOTHING — the caller owns every user-visible byte (the door
    /// echoes verbs itself, `amiexpress/express.e:5154-5179` readChar).
    ///
    /// The default returns `Eof` so line-only test fakes need no
    /// override; transports and decorators MUST override (gated by
    /// the keystroke smoke in `tierd_hotkey_smoke.rs`).
    fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        Box::pin(async { Ok(KeyRead::Eof) })
    }

    /// Whether ANSI colour output is currently enabled on this terminal
    /// (Tier A quickwin A8). The default is `true` — adapters that
    /// don't model a colour mode always emit colour; the
    /// [`ColourTerminal`](crate::app::colour_terminal::ColourTerminal)
    /// decorator overrides this to track the live `M`-toggled state.
    fn ansi_colour(&self) -> bool {
        true
    }

    /// Sets the live ANSI colour mode (the `M` command's effect).
    /// A no-op by default; [`ColourTerminal`](crate::app::colour_terminal::ColourTerminal)
    /// overrides it to strip ANSI SGR escapes from output while colour
    /// is off.
    fn set_ansi_colour(&mut self, _enabled: bool) {}
}

/// Writes `prompt`, flushes it, then reads one terminal line using
/// `echo` and `timeout`.
///
/// # Errors
/// Returns the concrete terminal error if writing, flushing, or
/// reading fails.
pub(crate) async fn read_prompted<T>(
    terminal: &mut T,
    prompt: &[u8],
    echo: TerminalEcho,
    timeout: Duration,
) -> Result<TerminalRead, T::Error>
where
    T: Terminal,
{
    terminal.write(prompt).await?;
    terminal.flush().await?;
    terminal.read_line(echo, timeout).await
}

/// Writes `bytes` and flushes the terminal.
///
/// # Errors
/// Returns the concrete terminal error if writing or flushing fails.
pub(crate) async fn write_and_flush<T>(terminal: &mut T, bytes: &[u8]) -> Result<(), T::Error>
where
    T: Terminal,
{
    terminal.write(bytes).await?;
    terminal.flush().await
}
