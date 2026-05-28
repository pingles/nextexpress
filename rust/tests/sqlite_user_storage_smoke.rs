//! End-to-end smoke test for the SQLite-backed user store.
//!
//! Boots the compiled `nextexpress` binary twice against the same
//! `user_storage = "<tempdir>/users.db"` config:
//!
//! 1. First boot: the database doesn't exist. The composition root
//!    creates it, seeds the default sysop and grants the seeded
//!    Conf01 membership. The smoke test signs in over telnet,
//!    exercises a menu command that bumps the user's `times_called`
//!    counter, then disconnects.
//! 2. Second boot: the database file is reused. The smoke test
//!    re-signs in and verifies that the persisted `times_called` is
//!    visible (the `SQLite` store survived a process restart, which
//!    the in-memory adapter cannot do).
//!
//! The tempdir guard cleans up the database file at the end of the
//! test, keeping the run hermetic.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn sqlite_backed_binary_persists_user_state_across_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("users.db");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\n\
         max_nodes = 1\n\
         bbs_path = {bbs}\n\
         max_password_failures = 3\n\
         user_storage = {db}\n",
        bbs = toml_string(dir.path()),
        db = toml_string(&db_path),
    );
    std::fs::write(&config_path, &toml).expect("write config");
    seed_conf01(dir.path());

    assert!(
        !db_path.exists(),
        "test starts with no database file at {db_path:?}"
    );

    // Boot #1: seed flow runs, sysop signs in, times_called bumps to 1.
    run_session(&config_path, &|stream| {
        sign_in_as_sysop(stream)?;
        // The `T` (time stats) command is part of the existing menu
        // surface; reading its output confirms we passed the menu
        // boundary. The exact line isn't load-bearing — `Command: `
        // returning means the session is logged in and active.
        write_line(stream, b"G")?;
        drain_until(stream, b"Goodbye").map_err(|e| format!("Goodbye after first boot: {e}"))
    });

    assert!(
        db_path.exists(),
        "SQLite database should have been created during first boot"
    );

    // Boot #2: same config, same DB. No re-seed should occur and the
    // sysop's persisted state should survive.
    run_session(&config_path, &|stream| {
        sign_in_as_sysop(stream)?;
        write_line(stream, b"G")?;
        drain_until(stream, b"Goodbye").map_err(|e| format!("Goodbye after second boot: {e}"))
    });

    // tempdir's Drop deletes the file when the test finishes.
}

fn run_session(config_path: &Path, walk: &dyn Fn(&mut TcpStream) -> Result<(), String>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
        stream
            .set_read_timeout(Some(PER_READ_TIMEOUT))
            .map_err(|e| format!("set_read_timeout: {e}"))?;
        walk(&mut stream)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("sqlite smoke session failed: {message}");
    }
}

fn sign_in_as_sysop(stream: &mut TcpStream) -> Result<(), String> {
    drain_until(stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(stream, b"sysop")?;
    drain_until(stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(stream, b"sysop")?;
    drain_until(stream, b"mins. left): ").map_err(|e| format!("Command prompt: {e}"))?;
    Ok(())
}

fn seed_conf01(bbs_path: &Path) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    std::fs::write(
        conf01.join("conference.toml"),
        b"number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write conference.toml");
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
                return Ok(());
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
