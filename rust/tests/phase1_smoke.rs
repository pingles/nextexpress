//! Phase 1 binary smoke test (Slice 13a).
//!
//! Spawns the compiled `nextexpress` binary as a subprocess against a
//! temporary TOML config (`port = 0`, single-node), parses the
//! `Listening on <addr>` line from its stdout, opens a real telnet
//! socket and walks the full sign-in → menu → goodbye flow with the
//! seeded `sysop`/`sysop` credentials. The point is to prove that the
//! library-level slice tests aren't lying: a fresh `cargo run` actually
//! delivers the Phase 1 headline value.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn binary_serves_signin_menu_goodbye_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");

    // Slice 34a: a session that never reaches a granted conference
    // hits `no_conference_access` immediately after authentication.
    // Drop a `Conf01/conference.toml` into the tempdir so the seeded
    // sysop's auto-rejoin succeeds and the menu loop engages —
    // matching what a fresh `cargo run` against the repo root sees.
    let conf01 = dir.path().join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    std::fs::write(
        conf01.join("conference.toml"),
        b"number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write conference.toml");

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_signin_loop(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

/// TOML-encodes a path as a basic string. The path is expected to be
/// the OS's tempdir, so we don't worry about embedded quotes.
fn toml_string(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Reads stdout one line at a time until a `Listening on <addr>` line
/// arrives, returning the address. Times out after `STARTUP_DEADLINE`.
fn read_listen_addr(child: &mut Child) -> Result<String, String> {
    let stdout = child.stdout.take().ok_or("stdout not piped")?;
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + STARTUP_DEADLINE;
    while Instant::now() < deadline {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("reading stdout: {e}"))?;
        if n == 0 {
            return Err("binary exited before printing 'Listening on ...'".to_string());
        }
        if let Some(addr) = line.trim().strip_prefix("Listening on ") {
            return Ok(addr.to_string());
        }
    }
    Err("timed out waiting for 'Listening on ...'".to_string())
}

/// Walks the full Phase 1 path: login → password → menu → G → goodbye.
///
/// Also verifies the echo contract our `IAC WILL ECHO` advertisement
/// promises: visible characters at the name and menu prompts, asterisk
/// masking at the password prompt.
fn walk_signin_loop(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ")
        .map_err(|e| format!("Graphics prompt: {e}"))?;
    write_line(&mut stream, b"Y")?;
    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    let between_handle_and_password = drain_until_capturing(&mut stream, b"PassWord: ")
        .map_err(|e| format!("Password prompt: {e}"))?;
    if !contains(&between_handle_and_password, b"sysop") {
        return Err(format!(
            "expected 'sysop' echoed back after typing the handle, got {:?}",
            String::from_utf8_lossy(&between_handle_and_password)
        ));
    }

    write_line(&mut stream, b"sysop")?;

    let between_password_and_menu = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt: {e}"))?;
    if !contains(&between_password_and_menu, b"*****") {
        return Err(format!(
            "expected at least five '*' echoes for the masked password, got {:?}",
            String::from_utf8_lossy(&between_password_and_menu)
        ));
    }
    if contains_password_plaintext(&between_password_and_menu) {
        return Err(format!(
            "password 'sysop' must NEVER appear in plaintext echo: {:?}",
            String::from_utf8_lossy(&between_password_and_menu)
        ));
    }

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// True when the bytes returned to the client between sending the
/// password and seeing the next prompt contain the literal handle
/// `sysop` — which would mean the server didn't mask the password
/// (the seed user happens to share the literal `sysop` in both fields,
/// so we have to be careful: the handle was already echoed in the
/// previous segment, but the password segment must not contain it).
fn contains_password_plaintext(segment: &[u8]) -> bool {
    contains(segment, b"sysop")
}

fn write_line(stream: &mut TcpStream, body: &[u8]) -> Result<(), String> {
    stream
        .write_all(body)
        .map_err(|e| format!("write body: {e}"))?;
    stream
        .write_all(b"\r\n")
        .map_err(|e| format!("write CRLF: {e}"))?;
    stream.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

/// Reads from `stream` until `needle` appears in the accumulated buffer
/// or `NEEDLE_DEADLINE` elapses.
fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Result<(), String> {
    drain_until_capturing(stream, needle).map(|_| ())
}

/// Like [`drain_until`] but returns the bytes consumed up to and
/// including the needle. The caller can then assert echo invariants
/// on the captured segment.
fn drain_until_capturing(stream: &mut TcpStream, needle: &[u8]) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + NEEDLE_DEADLINE;
    let mut buf = [0u8; 256];
    let mut acc = Vec::new();
    while Instant::now() < deadline {
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => 0,
            Err(error) => return Err(format!("read: {error}")),
        };
        if n > 0 {
            acc.extend_from_slice(&buf[..n]);
            if acc.windows(needle.len()).any(|w| w == needle) {
                return Ok(acc);
            }
        }
    }
    Err(format!(
        "needle {:?} not found within {:?}; got {:?}",
        std::str::from_utf8(needle).unwrap_or("<bin>"),
        NEEDLE_DEADLINE,
        String::from_utf8_lossy(&acc)
    ))
}
