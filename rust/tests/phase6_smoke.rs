//! Phase 6 binary smoke test (Slice 41a).
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path
//! pre-populated with a `Conf01/` and `Conf01/MsgBase/0000001.json`,
//! then drives the Phase 6 read flow over real telnet:
//!
//!   1. Sign in as the seeded `sysop` / `sysop`.
//!   2. Auto-rejoin attaches the session to Conf01 and Slice 41 fires
//!      the auto mail-scan-on-join, surfacing the seeded unread
//!      message and rendering `SCREEN_MAILSCAN` plus a summary line.
//!   3. `R 1` invokes Slice 39's `ReadMail` rule, marks `received_at`,
//!      and renders the legacy header block + body.
//!   4. `N` rescans for new mail; the previous read advanced
//!      `last_scanned` past the only message, so the listener writes
//!      "No new mail." (the spec's empty-scan summary).
//!   5. `G` ends the session cleanly.
//!
//! Library-level slice tests assert each piece in isolation; this
//! test proves a fresh `cargo run` actually delivers the headline
//! "Messaging (read)" capability.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn binary_walks_phase6_mail_read_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_conf01_with_one_unread_message(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_phase6_read_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

/// Seeds a single-msgbase Conf01 with one message addressed to the
/// seeded sysop (slot 1). The JSON payload mirrors the
/// `FileMailStore` on-disk format exactly so the binary's startup
/// scan picks it up as `highest_message = 1` and lets us walk the R
/// / M / N flow against real data.
fn seed_conf01_with_one_unread_message(bbs_path: &Path) {
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
    // The seeded sysop is slot 1, handle "sysop". Address the
    // message to that slot so the ReadMail rule will mark it
    // received_at and so the auto-scan-on-join counts it as unread.
    let mail = r#"{
        "conference_number": 1,
        "msgbase_number": 1,
        "number": 1,
        "visibility": "public",
        "from_name": "Sysop",
        "to_name": "sysop",
        "broadcast_to": "none",
        "subject": "Welcome to NextExpress",
        "posted_at": "1970-01-01T00:00:01Z",
        "received_at": null,
        "author_slot": 1,
        "addressee_slot": 1,
        "body": "Hello sysop, this is your first message.\n"
    }"#;
    std::fs::write(msgbase.join("0000001.json"), mail).expect("write 0000001.json");
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

/// Walks Phase 6: sign-in → auto-rejoin + auto-scan → R 1 → N → G.
fn walk_phase6_read_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    // After auth + auto-rejoin: the binary fires Slice 41's
    // auto-scan-on-join. It must surface the seeded unread message
    // (SCREEN_MAILSCAN screen + "You have 1 new message" summary)
    // before the menu prompt.
    let post_auth = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;
    if !contains(&post_auth, b"New mail") {
        return Err(format!(
            "expected SCREEN_MAILSCAN fallback after auto-scan, got {:?}",
            String::from_utf8_lossy(&post_auth)
        ));
    }
    if !contains(&post_auth, b"You have 1 new message") {
        return Err(format!(
            "expected scan summary after auto-scan, got {:?}",
            String::from_utf8_lossy(&post_auth)
        ));
    }

    // R 1 invokes Slice 39's ReadMail. Expect: legacy header block
    // (From, To, Subject, Conf), the body line, and a return to
    // the menu prompt.
    write_line(&mut stream, b"R 1")?;
    let post_r = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after R 1: {e}"))?;
    if !contains(&post_r, b"Subject") || !contains(&post_r, b"Welcome to NextExpress") {
        return Err(format!(
            "expected ReadMail header + subject, got {:?}",
            String::from_utf8_lossy(&post_r)
        ));
    }
    if !contains(&post_r, b"Hello sysop, this is your first message.") {
        return Err(format!(
            "expected ReadMail to render body, got {:?}",
            String::from_utf8_lossy(&post_r)
        ));
    }

    // N rescans for new mail. The previous R 1 advanced last_read
    // (via ReadMail) and last_scanned to 1, so a re-scan from
    // last_scanned + 1 finds no further unread messages.
    write_line(&mut stream, b"N")?;
    let post_n = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after N: {e}"))?;
    if !contains(&post_n, b"No new mail.") {
        return Err(format!(
            "expected `No new mail.` after second scan, got {:?}",
            String::from_utf8_lossy(&post_n)
        ));
    }

    // J 1 re-joins the same conference and fires Slice 41's auto
    // scan-on-join. Because the previous scans advanced
    // last_scanned past the only message, the re-join sees zero
    // unread, must NOT render SCREEN_MAILSCAN, and must still emit
    // the "No new mail." summary. Pins the `unread_count > 0`
    // boundary that gates the SCREEN_MAILSCAN render.
    write_line(&mut stream, b"J 1")?;
    let post_j = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after J 1: {e}"))?;
    if contains(&post_j, b"New mail in this conference") {
        return Err(format!(
            "auto scan-on-join must not render SCREEN_MAILSCAN when unread_count is zero, got {:?}",
            String::from_utf8_lossy(&post_j)
        ));
    }
    if !contains(&post_j, b"No new mail.") {
        return Err(format!(
            "expected `No new mail.` summary after zero-unread re-join, got {:?}",
            String::from_utf8_lossy(&post_j)
        ));
    }

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
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
