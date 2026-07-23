//! Telnet line input codec.
//!
//! Handles byte-oriented line input for the telnet adapter: strips IAC
//! negotiation sequences, accepts common CR/LF variants, and performs
//! server-side visible or masked echo.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::app::input_limits::MAX_TERMINAL_LINE_BYTES;
use crate::app::terminal::KeyEvent;

/// How [`read_telnet_line`] should echo the bytes it accepts.
///
/// Because the listener advertises `IAC WILL ECHO` to the client at
/// connect time, well-behaved clients (`SyncTerm`, `PuTTY`, telnet(1))
/// suppress their local echo and rely on the server to reflect typed
/// characters. Mirrors the original `AmiExpress` behaviour:
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
pub(crate) enum EchoMode {
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
/// `<Enter>` would force the user to press Enter twice.
///
/// `buf` is the in-progress line, also caller-owned (July 2026 review,
/// item 26): because this future can be raced against a session-signal
/// delivery and dropped mid-line, the half-typed bytes must live
/// outside it — a resumed call picks up exactly where the cancelled
/// one stopped (the per-byte echo has already been written). The
/// buffer is drained (`mem::take`) whenever a line is returned.
///
/// High bytes (`0x80..=0xFF`) are handled so the echoed stream stays
/// valid UTF-8, honouring the project's always-valid-UTF-8 wire
/// contract even for an 8-bit client. A well-formed UTF-8 multibyte
/// character is assembled across reads and echoed once whole; a byte
/// that cannot be part of a valid sequence is re-encoded as Latin-1
/// (`0xA9` `©` → `0xC2 0xA9`), matching the outbound wire-encoding rule.
/// A modern UTF-8 client's accented input round-trips; a legacy 8-bit
/// client's Latin-1 byte is restated as the same code point — a
/// deliberate departure from the legacy board, which passes raw 8-bit
/// bytes straight through (`COMMAND_PARITY.md`).
///
/// Returns `Ok(Some(line))` on success, `Ok(None)` on EOF before any
/// terminator was seen.
pub(crate) async fn read_telnet_line(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
    buf: &mut Vec<u8>,
    echo: EchoMode,
) -> io::Result<Option<String>> {
    // Bytes of an in-progress UTF-8 multibyte character: filled as
    // continuation bytes arrive, committed once the sequence is valid,
    // and re-encoded as Latin-1 if it turns out it never can be. Empty
    // for all-ASCII input (the common case), so no allocation happens
    // until a high byte is typed.
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let Some(b) = read_one(stream, pushback).await? else {
            // EOF: restate any half-typed high byte as Latin-1 so it is
            // not silently dropped (no echo — the peer has closed).
            for pb in std::mem::take(&mut pending) {
                buf.extend_from_slice(&latin1_to_utf8(pb));
            }
            return if buf.is_empty() {
                Ok(None)
            } else {
                Ok(Some(take_line(buf)))
            };
        };

        // Mid-character: route UTF-8 continuation bytes into `pending`.
        if !pending.is_empty() {
            if (0x80..=0xBF).contains(&b) {
                pending.push(b);
                match std::str::from_utf8(&pending) {
                    // Sequence complete and valid — commit the character.
                    Ok(_) => {
                        let done = std::mem::take(&mut pending);
                        commit_accepted(stream, buf, &done, echo).await?;
                    }
                    // Still a valid prefix — wait for the next byte.
                    Err(error) if error.error_len().is_none() => {}
                    // Can never become valid (overlong / bad continuation).
                    Err(_) => flush_pending_latin1(stream, buf, &mut pending, echo).await?,
                }
                continue;
            }
            // A non-continuation byte truncates the sequence: emit what
            // we held as Latin-1, then process `b` below on its own.
            flush_pending_latin1(stream, buf, &mut pending, echo).await?;
        }

        match b {
            0xFF => {
                // IAC. Delegate to the shared helper that handles
                // 3-byte negotiations and SB…IAC SE subnegotiations.
                if !skip_iac(stream, pushback).await? {
                    return Ok(None);
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
                return Ok(Some(take_line(buf)));
            }
            b'\n' => {
                stream.write_all(b"\r\n").await?;
                return Ok(Some(take_line(buf)));
            }
            0x08 | 0x7F
                // Backspace / DEL: drop the previous whole character if
                // any and erase one column on the user's terminal with
                // the classic <BS><SPACE><BS> triplet.
                if pop_last_char(buf) =>
            {
                stream.write_all(b"\x08 \x08").await?;
            }
            // Plain ASCII: accept and echo verbatim (or masked).
            0x20..=0x7F => commit_accepted(stream, buf, &[b], echo).await?,
            // High byte: either the lead of a UTF-8 character (buffer it,
            // echo nothing until it completes) or a lone/invalid byte we
            // read as Latin-1 right away.
            0x80..=0xFE => match std::str::from_utf8(&[b]) {
                Err(error) if error.error_len().is_none() => pending.push(b),
                _ => commit_accepted(stream, buf, &latin1_to_utf8(b), echo).await?,
            },
            // Other control bytes (Ctrl-* etc.): silently ignored,
            // matching `lineInput`'s `IF (ch>31)` guard.
            _ => {}
        }
    }
}

/// Drains the caller-owned line buffer into the returned line.
fn take_line(buf: &mut Vec<u8>) -> String {
    String::from_utf8_lossy(&std::mem::take(buf)).into_owned()
}

/// Re-encodes one high byte (`0x80..=0xFF`) as its two-byte UTF-8 form,
/// reading the byte as an ISO-8859-1 (Latin-1) code point — `0xA9` `©`
/// becomes `0xC2 0xA9`. This is the inbound analogue of the outbound
/// wire-encoding rule (AGENTS.md "Wire encoding"): the legacy Amiga
/// board's world is Latin-1, so a lone high byte the client typed is a
/// Latin-1 character, restated in UTF-8 so the wire stays valid.
///
/// Mutation note: `cargo mutants` leaves the two `| -> ^` mutants here
/// alive; they are equivalent. `0xC0` shares no set bit with `b >> 6`
/// (which occupies bits 0–1) and `0x80` shares none with `b & 0x3F`
/// (bits 0–5), so `^` produces byte-identical output to `|`.
fn latin1_to_utf8(b: u8) -> [u8; 2] {
    [0xC0 | (b >> 6), 0x80 | (b & 0x3F)]
}

/// Pushes `bytes` onto the line buffer (enforcing the length cap) and
/// echoes them: the bytes verbatim under [`EchoMode::Visible`], or a
/// single `*` for the whole unit under [`EchoMode::Masked`] (one masked
/// column per typed character, multibyte included).
///
/// # Errors
/// Returns [`io::ErrorKind::InvalidData`] when the addition would push
/// the line past [`MAX_TERMINAL_LINE_BYTES`], or the underlying
/// transport error on echo failure.
async fn commit_accepted(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    bytes: &[u8],
    echo: EchoMode,
) -> io::Result<()> {
    if buf.len() + bytes.len() > MAX_TERMINAL_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal input line exceeds maximum length",
        ));
    }
    buf.extend_from_slice(bytes);
    match echo {
        EchoMode::Visible => stream.write_all(bytes).await,
        EchoMode::Masked => stream.write_all(b"*").await,
    }
}

/// Emits an in-progress multibyte sequence that can no longer become
/// valid UTF-8, re-encoding each held byte as Latin-1. Leaves `pending`
/// empty.
async fn flush_pending_latin1(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    pending: &mut Vec<u8>,
    echo: EchoMode,
) -> io::Result<()> {
    for b in std::mem::take(pending) {
        commit_accepted(stream, buf, &latin1_to_utf8(b), echo).await?;
    }
    Ok(())
}

/// Removes the last whole UTF-8 character from a valid-UTF-8 line
/// buffer, walking back over continuation bytes (`0x80..=0xBF`).
/// Returns `false` (a no-op) when the buffer is empty.
///
/// Mutation note: `cargo mutants` leaves the `start > 0` -> `start >= 0`
/// mutant alive; it is equivalent. `buf` only ever holds valid UTF-8
/// (every byte enters through [`commit_accepted`] as ASCII, a complete
/// character, or a Latin-1 re-encoding), so `buf[0]` is never a lone
/// continuation byte — the `(0x80..=0xBF)` check therefore fails at
/// `start == 0` regardless of the bound, and the `start -= 1` underflow
/// the `> 0` guard prevents is unreachable.
fn pop_last_char(buf: &mut Vec<u8>) -> bool {
    let Some(mut start) = buf.len().checked_sub(1) else {
        return false;
    };
    while start > 0 && (0x80..=0xBF).contains(&buf[start]) {
        start -= 1;
    }
    buf.truncate(start);
    true
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
/// is left for a subsequent read; non-trailer bytes are stashed in
/// `pushback` so they aren't lost.
fn try_consume_cr_trailer(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<()> {
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

/// Consumes the remainder of an IAC sequence whose `0xFF` has already
/// been read: 3-byte negotiations (`WILL`/`WONT`/`DO`/`DONT` + option),
/// and `SB … IAC SE` subnegotiation.
///
/// # Parameters
/// - `stream`: the telnet TCP stream.
/// - `pushback`: one-byte look-ahead slot shared with the caller.
///
/// # Returns
/// `Ok(true)` when the sequence was consumed cleanly; `Ok(false)` when
/// EOF arrived mid-sequence.
///
/// # Errors
/// Returns the underlying [`io::Error`] on transport failure.
async fn skip_iac(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<bool> {
    let Some(cmd) = read_one(stream, pushback).await? else {
        return Ok(false);
    };
    if (0xFB..=0xFE).contains(&cmd) {
        // WILL / WONT / DO / DONT: one option byte follows.
        let _ = read_one(stream, pushback).await?;
    } else if cmd == 0xFA {
        // SB … IAC SE: consume until the `IAC SE` (0xFF 0xF0) pair.
        loop {
            let Some(b1) = read_one(stream, pushback).await? else {
                return Ok(false);
            };
            if b1 == 0xFF {
                let Some(b2) = read_one(stream, pushback).await? else {
                    return Ok(false);
                };
                if b2 == 0xF0 {
                    break;
                }
            }
        }
    }
    Ok(true)
}

/// Reads one keystroke in IAC-aware hot-key mode. Echoes nothing — hot-key
/// echo is the handler's responsibility (`express.e:5154-5179`).
///
/// CR (with optional LF/NUL trailer) → [`KeyEvent::Enter`]; a bare LF is
/// swallowed with no event (probe P2, `ae_tierd_probes.txt:140-175`); a
/// buffered `ESC[…` sequence is collapsed into one [`KeyEvent::Other`] so
/// an arrow press cannot fire several pager verbs. `Ok(None)` = EOF.
///
/// # Parameters
/// - `stream`: the telnet TCP stream.
/// - `pushback`: one-byte look-ahead slot shared with the caller.
///
/// # Returns
/// `Ok(Some(key))` on a keystroke, `Ok(None)` on EOF.
///
/// # Errors
/// Returns the underlying [`io::Error`] on transport failure.
pub(crate) async fn read_telnet_key(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
) -> io::Result<Option<KeyEvent>> {
    loop {
        let Some(b) = read_one(stream, pushback).await? else {
            return Ok(None);
        };
        match b {
            0xFF => {
                if !skip_iac(stream, pushback).await? {
                    return Ok(None);
                }
            }
            b'\r' => {
                try_consume_cr_trailer(stream, pushback)?;
                return Ok(Some(KeyEvent::Enter));
            }
            // Bare LF: the board swallows it — no event, not even
            // Other (which would advance the pager). Probe P2,
            // ae_tierd_probes.txt:140-175.
            b'\n' => {}
            0x08 | 0x7F => return Ok(Some(KeyEvent::Backspace)),
            // Ctrl-C — the pager's `**Break` verb needs the raw byte
            // as its own event (ae_tierd_help_audit.txt PCC).
            0x03 => return Ok(Some(KeyEvent::CtrlC)),
            0x1b => {
                swallow_buffered_csi(stream, pushback)?;
                return Ok(Some(KeyEvent::Other));
            }
            b if (0x20..=0x7E).contains(&b) => return Ok(Some(KeyEvent::Char(b))),
            _ => return Ok(Some(KeyEvent::Other)),
        }
    }
}

/// Best-effort, non-blocking swallow of an already-buffered CSI remainder
/// (`[ <params> <final>`). A full arrow/function sequence arrives in one
/// packet so its bytes are queued; a lone ESC press has nothing queued
/// and is left alone. Bounded at 8 bytes to avoid unbounded consumption.
///
/// **Single-packet assumption:** this function only works correctly when
/// the entire CSI sequence (`ESC`, `[`, params, and final byte) arrives
/// in a single TCP segment. If the delivery is split (ESC in one segment,
/// the `[…` body in a later one), the body is NOT swallowed: the ESC maps
/// to one `Other` and the body bytes then arrive as individual `Char`
/// events — accepted, because real arrow presses ship in one segment and
/// the pager treats stray printables as harmless continue verbs.
///
/// # Parameters
/// - `stream`: the telnet TCP stream.
/// - `pushback`: one-byte look-ahead slot shared with the caller.
///
/// # Errors
/// Returns the underlying [`io::Error`] on transport failure (not
/// `WouldBlock`, which is treated as "nothing buffered").
///
/// Mutation note: `cargo mutants` leaves the `n > 0`/`>=` count guards
/// and the `WouldBlock` error-kind guards here alive. The count-guard
/// survivors are equivalent — the `byte[0] == b'['` and
/// `(0x40..=0x7E)` sub-checks absorb the `n == 0` (EOF) case, so the
/// outcome is unchanged. The `WouldBlock` survivors are deferred: they
/// need an injected transport error / mid-stream EOF, which is not
/// deterministic against a real `TcpStream`. The swallow *contract*
/// (a buffered `ESC[…` collapses to one event; a non-`[` byte is
/// pushed back) is pinned by the `read_key_*` content tests below.
fn swallow_buffered_csi(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<()> {
    // If pushback holds a byte it came from a previous look-ahead, not a
    // buffered CSI continuation — leave it for the next read.
    if pushback.is_some() {
        return Ok(());
    }
    let mut byte = [0u8; 1];
    match stream.try_read(&mut byte) {
        Ok(n) if n > 0 && byte[0] == b'[' => {}
        Ok(n) if n > 0 => {
            *pushback = Some(byte[0]);
            return Ok(());
        }
        Ok(_) => return Ok(()), // EOF
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(e) => return Err(e),
    }
    for _ in 0..8 {
        match stream.try_read(&mut byte) {
            Ok(n) if n > 0 => {
                if (0x40..=0x7E).contains(&byte[0]) {
                    return Ok(()); // final byte — sequence complete
                }
                // Parameter/intermediate byte: keep consuming.
            }
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::app::terminal::KeyEvent;

    async fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn visible_echo_of_a_latin1_byte_stays_valid_utf8() {
        // A Latin-1 client types `©` (lone byte 0xA9). The wire must
        // carry valid UTF-8 (0xC2 0xA9), and the returned line must be
        // the `©` code point, not U+FFFD.
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\xa9\r").await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();
        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "\u{a9}");
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert!(
            std::str::from_utf8(&echoed).is_ok(),
            "echo must be valid UTF-8, got {echoed:?}"
        );
        assert_eq!(echoed, "\u{a9}\r\n".as_bytes());
    }

    #[tokio::test]
    async fn visible_echo_of_a_utf8_multibyte_char_round_trips() {
        // A modern UTF-8 client types `é` (0xC3 0xA9): it must round-trip
        // to the line and echo whole (never a lone lead byte on the wire).
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all("é\r".as_bytes()).await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();
        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "é");
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, "é\r\n".as_bytes());
    }

    #[tokio::test]
    async fn multibyte_char_assembles_across_split_segments() {
        // The three bytes of `€` (0xE2 0x82 0xAC) arrive in separate
        // segments, so assembly must span several awaited reads.
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        let mut pushback = None;
        let mut buf = Vec::new();
        let read = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible);
        let write = async {
            for chunk in [&b"\xe2"[..], b"\x82", b"\xac", b"\r"] {
                client.write_all(chunk).await.unwrap();
                client.flush().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            client
        };
        let (line, mut client) = tokio::join!(read, write);
        assert_eq!(line.unwrap().expect("line"), "€");
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, "€\r\n".as_bytes());
    }

    #[tokio::test]
    async fn backspace_removes_the_whole_multibyte_char() {
        // BS after `é` must erase the whole character — not leave a
        // dangling lead byte that take_line would turn into U+FFFD.
        let (mut server, mut client) = connected_pair().await;
        client.write_all("é\x08x\r".as_bytes()).await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();
        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "x");
    }

    #[tokio::test]
    async fn masked_mode_emits_one_star_per_character() {
        // Masked mode keeps the real characters in the line (the password
        // is compared as a string) but echoes exactly one `*` per typed
        // character, a multibyte character included.
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all("aé\r".as_bytes()).await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();
        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Masked)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "aé");
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, b"**\r\n");
    }

    #[tokio::test]
    async fn a_truncated_lead_byte_falls_back_to_latin1() {
        // A lead byte (0xC3) immediately followed by ASCII can never
        // complete, so the lead is re-read as Latin-1 (`Ã`) and the ASCII
        // byte accepted after it; the wire stays valid UTF-8.
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\xc3A\r").await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();
        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "ÃA");
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert!(std::str::from_utf8(&echoed).is_ok());
        assert_eq!(echoed, "ÃA\r\n".as_bytes());
    }

    #[tokio::test]
    async fn a_lone_latin1_byte_echoes_at_the_keypress() {
        // The Latin-1 re-encoding happens the moment a byte is known not
        // to be a UTF-8 lead — not deferred until a terminator. A lone
        // 0xA9 echoes 0xC2 0xA9 before any CR is sent.
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        let mut pushback = None;
        let mut buf = Vec::new();
        let read = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible);
        let drive = async {
            client.write_all(b"\xa9").await.unwrap();
            client.flush().await.unwrap();
            let mut echo = [0u8; 2];
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                client.read_exact(&mut echo),
            )
            .await
            .expect("echo must arrive before any terminator")
            .unwrap();
            client.write_all(b"\r").await.unwrap();
            client.flush().await.unwrap();
            echo
        };
        let (line, echo) = tokio::join!(read, drive);
        assert_eq!(&echo, "\u{a9}".as_bytes());
        assert_eq!(line.unwrap().expect("line"), "\u{a9}");
    }

    #[tokio::test]
    async fn an_invalid_continuation_flushes_at_the_offending_byte() {
        // 0xE0 then 0x80 is overlong: the moment 0x80 makes the sequence
        // un-completable, both held bytes flush as Latin-1 — they are not
        // held waiting for a third byte (which would defer the echo).
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        let mut pushback = None;
        let mut buf = Vec::new();
        let read = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible);
        let drive = async {
            client.write_all(b"\xe0\x80").await.unwrap();
            client.flush().await.unwrap();
            let mut echo = [0u8; 4]; // à (C3 A0) + U+0080 (C2 80)
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                client.read_exact(&mut echo),
            )
            .await
            .expect("invalid sequence must flush at the offending byte")
            .unwrap();
            client.write_all(b"\r").await.unwrap();
            client.flush().await.unwrap();
            echo
        };
        let (line, echo) = tokio::join!(read, drive);
        assert_eq!(&echo, "\u{e0}\u{80}".as_bytes());
        assert_eq!(line.unwrap().expect("line"), "\u{e0}\u{80}");
    }

    #[tokio::test]
    async fn accepts_a_line_at_the_terminal_byte_limit() {
        let (mut server, mut client) = connected_pair().await;
        let input = vec![b'a'; MAX_TERMINAL_LINE_BYTES];
        client.write_all(&input).await.unwrap();
        client.write_all(b"\r").await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();

        let line = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .unwrap()
            .expect("line");

        assert_eq!(line.len(), MAX_TERMINAL_LINE_BYTES);
        assert!(line.bytes().all(|b| b == b'a'));
    }

    #[tokio::test]
    async fn rejects_a_line_over_the_terminal_byte_limit() {
        let (mut server, mut client) = connected_pair().await;
        let input = vec![b'a'; MAX_TERMINAL_LINE_BYTES + 1];
        client.write_all(&input).await.unwrap();
        let mut pushback = None;
        let mut buf = Vec::new();

        let err = read_telnet_line(&mut server, &mut pushback, &mut buf, EchoMode::Visible)
            .await
            .expect_err("overlong line must fail");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn read_key_maps_printables_enter_variants_and_backspace() {
        // Bare LF is swallowed with no event — the board drops it
        // entirely (probe P2, ae_tierd_probes.txt:140-175) — so the
        // lone `\n` below yields nothing and the next event after `x`
        // is the Backspace.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"n\r\x00Q\r\nx\n\x08").await.unwrap();
        let mut pushback = None;
        let mut keys = Vec::new();
        for _ in 0..6 {
            keys.push(
                read_telnet_key(&mut server, &mut pushback)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        assert_eq!(
            keys,
            vec![
                KeyEvent::Char(b'n'),
                KeyEvent::Enter, // CR NUL
                KeyEvent::Char(b'Q'),
                KeyEvent::Enter, // CR LF
                KeyEvent::Char(b'x'),
                KeyEvent::Backspace, // the bare LF before it: no event
            ]
        );
    }

    #[tokio::test]
    async fn read_key_decodes_ctrl_c_distinctly() {
        // 0x03 must reach the pager as its own event (the `**Break`
        // verb, ae_tierd_help_audit.txt PCC) — not as `Other`, which
        // the pager treats as a resume.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\x03").await.unwrap();
        let mut pushback = None;
        let key = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(key, KeyEvent::CtrlC);
    }

    #[tokio::test]
    async fn read_key_swallows_a_csi_sequence_as_one_event() {
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\x1b[Ay").await.unwrap();
        let mut pushback = None;
        let first = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        let second = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, KeyEvent::Other, "arrow = one event");
        assert_eq!(second, KeyEvent::Char(b'y'));
    }

    #[tokio::test]
    async fn read_key_skips_iac_and_echoes_nothing() {
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all(&[0xFF, 0xFD, 0x01, b'n']).await.unwrap();
        let mut pushback = None;
        let key = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(key, KeyEvent::Char(b'n'));
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, b"", "key reads must write zero bytes");
    }

    #[tokio::test]
    async fn read_key_maps_unprintable_control_bytes_to_other() {
        // Ctrl-A (0x01) is below 0x20 and is not \r, \n, BS, or ESC —
        // it must produce Other, not Char. This pins the printable-range
        // guard so mutants that widen it to include control bytes are
        // caught.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(&[0x01, b'z']).await.unwrap();
        let mut pushback = None;
        let ctrl = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        let printable = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ctrl, KeyEvent::Other, "Ctrl-A must be Other");
        assert_eq!(printable, KeyEvent::Char(b'z'));
    }

    #[tokio::test]
    async fn read_key_pushes_back_a_non_bracket_byte_after_esc() {
        // ESC followed by a non-'[' byte: swallow_buffered_csi must push
        // the byte back rather than discard it.  The lone ESC maps to
        // Other; the pushed-back 'z' must surface as the next Char.
        // This kills the `byte[0] == b'['` guard mutants.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\x1bz").await.unwrap();
        // try_read inside swallow_buffered_csi needs both bytes already
        // queued in the kernel socket buffer before the first read_telnet_key
        // returns, so we flush and give the OS a moment to deliver them.
        client.flush().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let mut pushback = None;
        let first = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        let second = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, KeyEvent::Other, "lone ESC = Other");
        assert_eq!(second, KeyEvent::Char(b'z'), "pushed-back byte = Char");
    }

    #[tokio::test]
    async fn read_key_swallows_a_multi_param_csi_sequence() {
        // A Ctrl-Up style CSI with params (`ESC [ 1 ; 5 A`) followed by
        // a printable.  The entire sequence through the final byte 'A'
        // must be collapsed into one Other; 'w' must surface separately.
        // This kills the inner-loop n>0 and final-byte-range mutants.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\x1b[1;5Aw").await.unwrap();
        client.flush().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let mut pushback = None;
        let first = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        let second = read_telnet_key(&mut server, &mut pushback)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, KeyEvent::Other, "CSI sequence = one Other");
        assert_eq!(second, KeyEvent::Char(b'w'), "trailing byte = Char");
    }
}
