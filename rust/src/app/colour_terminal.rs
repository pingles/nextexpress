//! [`ColourTerminal`] — a [`Terminal`] decorator that strips ANSI SGR
//! colour escapes from output while ANSI colour is disabled.
//!
//! Tier A quickwin A8 (`M`). The legacy `internalCommandM`
//! (`amiexpress/express.e:25239`) toggles a live `ansiColour` flag; when
//! it is off the BBS suppresses the colour run it would otherwise emit.
//! `NextExpress` models this as a decorator over the transport
//! [`Terminal`]: every server-originated write — menu screens, the
//! menu prompt (via `read_prompted`), stats and join lines — passes
//! through `write`, so wrapping the transport terminal once at the
//! composition root applies the policy to all output.
//!
//! The live flag defaults on; the persisted `User.ansi_colour`
//! preference is *not* wired to it yet — that field defaults `false`
//! for `User::new`/the seeded sysop and there is no ANSI-detection or
//! registration-ANSI flow that sets it meaningfully, so initialising
//! the live flag from it would strip the colour the rest of the BBS
//! already emits. Defaulting on (and letting `M` toggle) matches the
//! current wire behaviour; wiring the preference awaits a slice that
//! establishes a sensible source for it.

use crate::app::terminal::{KeyRead, Terminal, TerminalEcho, TerminalFuture, TerminalRead};
use std::time::Duration;

/// Removes ANSI SGR (Select Graphic Rendition) escape sequences —
/// `ESC [ <params> m` — from `bytes`, leaving every other byte
/// (including non-SGR CSI sequences such as cursor moves) untouched.
///
/// Mirrors the legacy intent of suppressing the `[..m` colour runs when
/// `ansiColour` is off (`amiexpress/express.e:25241-25247`).
#[must_use]
pub(crate) fn strip_ansi_sgr(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            // Consume the CSI parameter bytes (`[0-9;]*`).
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            // Only an SGR sequence (final byte `m`) is a colour run.
            if bytes.get(j) == Some(&b'm') {
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// A [`Terminal`] decorator carrying a live ANSI-colour flag. While the
/// flag is off, [`Terminal::write`] strips SGR colour escapes via
/// [`strip_ansi_sgr`]; otherwise bytes pass through untouched. `flush`
/// and `read_line` always delegate to the wrapped terminal.
pub(crate) struct ColourTerminal<T> {
    inner: T,
    ansi_colour: bool,
}

impl<T> ColourTerminal<T> {
    /// Wraps `inner`, starting with `ansi_colour` as the live colour
    /// mode. The composition root passes `true` so a fresh connection
    /// renders colour until the user turns it off with `M`.
    pub(crate) fn new(inner: T, ansi_colour: bool) -> Self {
        Self { inner, ansi_colour }
    }
}

impl<T: Terminal + Send> Terminal for ColourTerminal<T> {
    type Error = T::Error;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        if self.ansi_colour {
            self.inner.write(bytes)
        } else {
            let stripped = strip_ansi_sgr(bytes);
            Box::pin(async move { self.inner.write(&stripped).await })
        }
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        self.inner.flush()
    }

    fn read_line(
        &mut self,
        echo: TerminalEcho,
        timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        self.inner.read_line(echo, timeout)
    }

    fn read_key(&mut self, timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        self.inner.read_key(timeout)
    }

    fn ansi_colour(&self) -> bool {
        self.ansi_colour
    }

    fn set_ansi_colour(&mut self, enabled: bool) {
        self.ansi_colour = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_sgr_removes_colour_runs_and_keeps_text() {
        // The `[32m` / `[33m` / `[0m` SGR runs from the legacy prompts
        // are dropped; the surrounding text survives intact.
        assert_eq!(
            strip_ansi_sgr(b"\x1b[32mJoining\x1b[33m:\x1b[0m Main"),
            b"Joining: Main"
        );
    }

    #[test]
    fn strip_ansi_sgr_is_a_no_op_without_escapes() {
        assert_eq!(strip_ansi_sgr(b"plain text\r\n"), b"plain text\r\n");
    }

    #[test]
    fn strip_ansi_sgr_preserves_non_sgr_csi_sequences() {
        // A non-SGR CSI (here a cursor-clear `ESC [ 2 J`) is not a
        // colour run and must pass through untouched.
        assert_eq!(strip_ansi_sgr(b"a\x1b[2Jb"), b"a\x1b[2Jb");
    }

    /// Minimal capture terminal: records everything written to it so a
    /// `ColourTerminal` test can assert what reached the inner adapter.
    #[derive(Default)]
    struct CaptureTerminal {
        written: Vec<u8>,
    }

    impl Terminal for CaptureTerminal {
        type Error = std::convert::Infallible;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            self.written.extend_from_slice(bytes);
            Box::pin(async { Ok(()) })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(
            &mut self,
            _echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async { Ok(TerminalRead::Eof) })
        }
    }

    #[test]
    fn terminal_default_colour_mode_is_on() {
        // Adapters that don't model a colour mode (here the capture
        // terminal) inherit the trait default of colour ON, so they
        // emit ANSI unless wrapped by a ColourTerminal that's off.
        let term = CaptureTerminal::default();
        assert!(term.ansi_colour());
    }

    #[tokio::test]
    async fn colour_terminal_passes_escapes_through_when_colour_on() {
        let mut term = ColourTerminal::new(CaptureTerminal::default(), true);
        term.write(b"\x1b[32mHi\x1b[0m").await.unwrap();
        assert_eq!(term.inner.written, b"\x1b[32mHi\x1b[0m");
        assert!(term.ansi_colour());
    }

    #[tokio::test]
    async fn colour_terminal_strips_escapes_when_colour_off() {
        let mut term = ColourTerminal::new(CaptureTerminal::default(), true);
        term.set_ansi_colour(false);
        assert!(!term.ansi_colour());
        term.write(b"\x1b[32mHi\x1b[0m").await.unwrap();
        assert_eq!(term.inner.written, b"Hi");
    }

    /// The trait default for `read_key` returns `Eof`; `CaptureTerminal`
    /// does NOT override it, so this exercises the default path.
    /// We also construct the remaining variants here so that Rust's
    /// dead-code lint stays quiet while the consuming code is still in
    /// the next task.
    #[tokio::test]
    async fn capture_terminal_default_read_key_returns_eof() {
        use crate::app::terminal::{KeyEvent, KeyRead};
        let mut term = CaptureTerminal::default();
        let result = term.read_key(Duration::from_secs(1)).await.unwrap();
        assert_eq!(result, KeyRead::Eof);
        // Mention all variants so the dead-code lint stays quiet until
        // the consuming code lands in the next task.
        let _ = KeyRead::Key(KeyEvent::Enter);
        let _ = KeyRead::Key(KeyEvent::Backspace);
        let _ = KeyRead::Key(KeyEvent::Other);
        let _ = KeyRead::IdleTimedOut;
    }

    #[tokio::test]
    async fn colour_terminal_delegates_read_key() {
        use crate::app::terminal::{KeyEvent, KeyRead};
        // A stub terminal that returns a scripted keystroke so we can
        // observe that ColourTerminal passes the call through rather
        // than answering it itself.
        struct OneKey;
        impl Terminal for OneKey {
            type Error = std::convert::Infallible;
            fn write<'a>(&'a mut self, _b: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
                Box::pin(async { Ok(()) })
            }
            fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
                Box::pin(async { Ok(()) })
            }
            fn read_line(
                &mut self,
                _echo: TerminalEcho,
                _timeout: Duration,
            ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
                Box::pin(async { Ok(TerminalRead::Eof) })
            }
            fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
                Box::pin(async { Ok(KeyRead::Key(KeyEvent::Char(b'q'))) })
            }
        }
        let mut term = ColourTerminal::new(OneKey, true);
        let key = term.read_key(Duration::from_secs(1)).await.unwrap();
        assert_eq!(key, KeyRead::Key(KeyEvent::Char(b'q')));
    }
}
