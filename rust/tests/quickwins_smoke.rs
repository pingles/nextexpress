//! Tier A "quick wins" binary smoke test.
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path,
//! signs in as the seeded sysop, and drives one Tier A quickwin per
//! scenario. Each scenario asserts the verbatim `AmiExpress` wire text
//! so the binary really delivers the legacy literals.
//!
//! Scenarios land one-per-commit so each Tier A slice has its own
//! end-to-end gate. The wire-and-smoke closing slice
//! (`slices/cmds-quickwins.md` — Slice A-wire) collapses them into a
//! single composite walk later.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn binary_renders_legacy_t_command_over_telnet() {
    // Slice A1 — `T` (current date/time). Mirrors
    // `internalCommandT()` at `amiexpress/express.e:25622-25644`.
    // The exact `MM-DD-YY HH:MM:SS` payload depends on the wall
    // clock so the smoke pins only the surrounding literals: the
    // `It is ` prefix, the trailing CRLF, and the menu reappears.
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_minimal_conf01(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_show_time_command(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

/// Drives the bare minimum Tier A `T` scenario:
///
///   1. Sign in as the seeded `sysop` / `sysop`.
///   2. Send `T`.
///   3. Assert the response contains `It is ` (verbatim from
///      `amiexpress/express.e:25636`) and a `:` (date-time separator)
///      before the next menu prompt.
///   4. `G` ends the session.
fn walk_show_time_command(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    drain_until(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;

    // `T` should print exactly the legacy literal: `\r\nIt is ` then
    // `MM-DD-YY HH:MM:SS\r\n` then the menu prompt reappears.
    write_line(&mut stream, b"T")?;
    let post_t = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after T: {e}"))?;
    if !contains(&post_t, b"It is ") {
        return Err(format!(
            "expected legacy `It is ` prefix after T, got {:?}",
            String::from_utf8_lossy(&post_t)
        ));
    }
    // Structural check on the rendered time literal: `MM-DD-YY HH:MM:SS`
    // splits into three hyphen-separated date parts and three
    // colon-separated time parts. Anything else (e.g. a stub literal or
    // a swapped separator) fails the parse.
    let it_is_idx = find(&post_t, b"It is ").ok_or("It is prefix not found")?;
    let tail = &post_t[it_is_idx + b"It is ".len()..];
    let line_end = tail
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or("missing CRLF terminator after time line")?;
    let line =
        std::str::from_utf8(&tail[..line_end]).map_err(|e| format!("non-utf8 time line: {e}"))?;
    let (date, clock) = line
        .split_once(' ')
        .ok_or_else(|| format!("expected `<date> <time>`, got {line:?}"))?;
    let date_parts: Vec<&str> = date.split('-').collect();
    let clock_parts: Vec<&str> = clock.split(':').collect();
    if date_parts.len() != 3 || clock_parts.len() != 3 {
        return Err(format!(
            "expected `MM-DD-YY HH:MM:SS` after `It is `, got {line:?}",
        ));
    }

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}

/// Writes a single-conference Conf01 so the post-auth auto-rejoin
/// can attach the session somewhere. The `T` command itself reads no
/// conference state, but the menu loop runs inside a joined session.
fn seed_minimal_conf01(bbs_path: &Path) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    std::fs::write(
        conf01.join("conference.toml"),
        b"number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write Conf01/conference.toml");
    std::fs::write(conf01.join("menu.txt"), b"CONF1-MENU\r\n").expect("write Conf01/menu.txt");

    let msgbase = conf01.join("MsgBase");
    std::fs::create_dir_all(&msgbase).expect("create MsgBase");
}

fn toml_string(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
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

fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Result<(), String> {
    drain_until_capturing(stream, needle).map(|_| ())
}

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
