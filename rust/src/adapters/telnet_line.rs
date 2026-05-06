//! Telnet line input codec.
//!
//! Handles byte-oriented line input for the telnet adapter: strips IAC
//! negotiation sequences, accepts common CR/LF variants, and performs
//! server-side visible or masked echo.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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
                // IAC. Consume the command (and option byte for the
                // 3-byte negotiations).
                let Some(cmd) = read_one(stream, pushback).await? else {
                    return Ok(None);
                };
                if (0xFB..=0xFE).contains(&cmd) {
                    // WILL / WONT / DO / DONT: one option byte follows.
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
/// is left for a subsequent read; non-trailer bytes are stashed in
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
