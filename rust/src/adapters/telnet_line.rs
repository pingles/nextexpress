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
    /// Echo nothing at all — no per-character echo, no `\r\n` on
    /// Enter, no BS-SP-BS erase. Used by the `NextScan` pager's
    /// sub-prompts (slice D2), whose handlers emit every captured
    /// echo byte themselves.
    Silent,
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
/// Returns `Ok(Some(line))` on success, `Ok(None)` on EOF before any
/// terminator was seen.
pub(crate) async fn read_telnet_line(
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
                if echo != EchoMode::Silent {
                    stream.write_all(b"\r\n").await?;
                }
                return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
            }
            b'\n' => {
                if echo != EchoMode::Silent {
                    stream.write_all(b"\r\n").await?;
                }
                return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
            }
            0x08 | 0x7F
                // Backspace / DEL: drop the previous byte if any and
                // erase one column on the user's terminal with the
                // classic <BS><SPACE><BS> triplet.
                if buf.pop().is_some() =>
            {
                if echo != EchoMode::Silent {
                    stream.write_all(b"\x08 \x08").await?;
                }
            }
            b if b >= 0x20 => {
                if buf.len() >= MAX_TERMINAL_LINE_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "terminal input line exceeds maximum length",
                    ));
                }
                buf.push(b);
                let echoed = match echo {
                    EchoMode::Visible => b,
                    EchoMode::Masked => b'*',
                    EchoMode::Silent => {
                        continue;
                    }
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
    async fn silent_mode_is_silent_for_bare_lf_terminators() {
        // The `\n` arm has its own echo guard: a client that ends
        // lines with a lone LF must also get zero bytes back in
        // Silent mode.
        use tokio::io::AsyncReadExt;

        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"ok\n").await.unwrap();
        let mut pushback = None;

        let line = read_telnet_line(&mut server, &mut pushback, EchoMode::Silent)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "ok");

        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, b"", "Silent read must write zero bytes");
    }

    #[tokio::test]
    async fn silent_mode_accepts_a_line_while_echoing_nothing() {
        // Slice D2: the NextScan pager's sub-prompts emit every echo
        // byte from the handler itself, so the adapter read must be
        // byte-silent — no per-char echo, no `\r\n` on Enter, no
        // BS-SP-BS triplet.
        use tokio::io::AsyncReadExt;

        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"ab\x08c\r").await.unwrap();
        let mut pushback = None;

        let line = read_telnet_line(&mut server, &mut pushback, EchoMode::Silent)
            .await
            .unwrap()
            .expect("line");
        assert_eq!(line, "ac");

        // Close the server side so the drain below terminates: the
        // client must have received zero echo bytes.
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, b"", "Silent read must write zero bytes");
    }

    #[tokio::test]
    async fn accepts_a_line_at_the_terminal_byte_limit() {
        let (mut server, mut client) = connected_pair().await;
        let input = vec![b'a'; MAX_TERMINAL_LINE_BYTES];
        client.write_all(&input).await.unwrap();
        client.write_all(b"\r").await.unwrap();
        let mut pushback = None;

        let line = read_telnet_line(&mut server, &mut pushback, EchoMode::Visible)
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

        let err = read_telnet_line(&mut server, &mut pushback, EchoMode::Visible)
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
