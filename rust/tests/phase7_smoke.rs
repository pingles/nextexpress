//! Phase 7 binary smoke tests (Slices 42 and 44a).
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path
//! pre-populated with a `Conf01/` and a single seed message, then
//! drives the Phase 7 write flows over real telnet:
//!
//! ## Slice 42 — single-addressee `E`
//!   1. Sign in as the seeded `sysop` / `sysop`.
//!   2. Auto-rejoin attaches the session to Conf01.
//!   3. `E sysop` opens the line-mode composer, walks subject /
//!      private / body prompts, finishes with `.` on its own line.
//!   4. The listener confirms `Message #2 saved.` (the seed mail was
//!      #1).
//!   5. `R 2` rereads the freshly posted message to prove the store
//!      persisted it.
//!   6. `G` ends the session cleanly.
//!
//! ## Slice 44a — broadcast + comment-to-sysop, configured msgbase
//!   1. Seed a Conf01 whose `[[msgbase]]` carries
//!      `allowed_addressing = "individual_or_all"` (i.e. EALL is
//!      forbidden by sysop policy).
//!   2. Sign in as `sysop`.
//!   3. `E ALL` succeeds and the freshly-saved mail renders
//!      `Recv'd: N/A` on read-back (broadcast contract).
//!   4. `E EALL` is rejected with the addressing-not-allowed notice;
//!      the message base's `highest_message` does not advance.
//!   5. `C` walks the comment-to-sysop sub-flow (subject + body,
//!      no `To:` / `Private` prompt) and persists a private mail
//!      addressed to `Sysop`. Re-reading it shows
//!      `Status: Private Message`.
//!   6. `G` ends the session cleanly.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn binary_walks_phase7_mail_post_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_conf01_with_one_message(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_phase7_post_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

fn seed_conf01_with_one_message(bbs_path: &Path) {
    seed_conf01_with(
        bbs_path,
        "number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
        true,
    );
}

/// Seeds a `Conf01/` whose msgbase forbids EALL (Slice 44a's policy
/// fixture). The base accepts ALL but not EALL; the smoke test pins
/// the resulting wire surface.
fn seed_conf01_with_individual_or_all(bbs_path: &Path) {
    seed_conf01_with(
        bbs_path,
        concat!(
            "number = 1\n",
            "name = \"Main\"\n",
            "[[msgbase]]\n",
            "number = 1\n",
            "name = \"main\"\n",
            "allowed_addressing = \"individual_or_all\"\n",
        ),
        false,
    );
}

fn seed_conf01_with(bbs_path: &Path, conference_toml: &str, write_seed_mail: bool) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    std::fs::write(conf01.join("conference.toml"), conference_toml.as_bytes())
        .expect("write Conf01/conference.toml");
    std::fs::write(conf01.join("menu.txt"), b"CONF1-MENU\r\n").expect("write Conf01/menu.txt");

    let msgbase = conf01.join("MsgBase");
    std::fs::create_dir_all(&msgbase).expect("create MsgBase");
    if write_seed_mail {
        let mail = r#"{
            "conference_number": 1,
            "msgbase_number": 1,
            "number": 1,
            "visibility": "public",
            "from_name": "Sysop",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "Seed",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 1,
            "addressee_slot": 1,
            "body": "Seed body.\n"
        }"#;
        std::fs::write(msgbase.join("0000001.json"), mail).expect("write 0000001.json");
    }
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

fn walk_phase7_post_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    // Wait for the menu prompt after sign-in and auto-rejoin.
    drain_until(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;

    // E sysop opens the composer. We send sysop as the recipient
    // inline so the To: prompt is skipped.
    write_line(&mut stream, b"E sysop")?;
    drain_until(&mut stream, b"Subject: ").map_err(|e| format!("Subject prompt: {e}"))?;
    write_line(&mut stream, b"Hello from the smoke test")?;
    drain_until(&mut stream, b"Private (y/N)? ").map_err(|e| format!("Private prompt: {e}"))?;
    write_line(&mut stream, b"N")?;
    // Body prompt instructs and asks for the first line.
    drain_until(&mut stream, b"End with a single '.'")
        .map_err(|e| format!("Body instructions: {e}"))?;
    write_line(&mut stream, b"Body line one.")?;
    write_line(&mut stream, b"Body line two.")?;
    write_line(&mut stream, b".")?;

    let post_e = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after E: {e}"))?;
    if !contains(&post_e, b"Message #2 saved.") {
        return Err(format!(
            "expected `Message #2 saved.` after E, got {:?}",
            String::from_utf8_lossy(&post_e)
        ));
    }

    // R 2 reads the message we just posted, proving it persisted.
    write_line(&mut stream, b"R 2")?;
    let post_r = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after R 2: {e}"))?;
    if !contains(&post_r, b"Hello from the smoke test") {
        return Err(format!(
            "expected R 2 to render the newly posted subject, got {:?}",
            String::from_utf8_lossy(&post_r)
        ));
    }
    if !contains(&post_r, b"Body line one.") || !contains(&post_r, b"Body line two.") {
        return Err(format!(
            "expected R 2 to render the body lines, got {:?}",
            String::from_utf8_lossy(&post_r)
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

#[test]
fn binary_walks_phase7_broadcast_and_comment_to_sysop_over_telnet() {
    // Slice 44a: end-to-end smoke test for the Phase 7 broadcast and
    // comment-to-sysop flows. The seeded conference forbids EALL via
    // `allowed_addressing = "individual_or_all"`, so the smoke checks
    // both the allowed (`E ALL`) and refused (`E EALL`) branches plus
    // the dedicated `C` command.
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_conf01_with_individual_or_all(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_phase7_broadcast_and_comment_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

fn walk_phase7_broadcast_and_comment_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    sign_in_as_sysop(&mut stream)?;
    post_all_message(&mut stream)?;
    read_back_all_message(&mut stream)?;
    reject_eall_post(&mut stream)?;
    post_comment_to_sysop(&mut stream)?;
    read_back_comment_to_sysop(&mut stream)?;

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}

fn sign_in_as_sysop(stream: &mut TcpStream) -> Result<(), String> {
    drain_until(stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(stream, b"sysop")?;
    drain_until(stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(stream, b"sysop")?;
    drain_until(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;
    Ok(())
}

fn post_all_message(stream: &mut TcpStream) -> Result<(), String> {
    write_line(stream, b"E ALL")?;
    drain_until(stream, b"Subject: ").map_err(|e| format!("ALL subject prompt: {e}"))?;
    write_line(stream, b"Notice to everyone")?;
    drain_until(stream, b"Private (y/N)? ").map_err(|e| format!("ALL private prompt: {e}"))?;
    write_line(stream, b"N")?;
    drain_until(stream, b"End with a single '.'")
        .map_err(|e| format!("ALL body instructions: {e}"))?;
    write_line(stream, b"Hi everyone.")?;
    write_line(stream, b".")?;
    let after = drain_until_capturing(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after E ALL: {e}"))?;
    if !contains(&after, b"Message #1 saved.") {
        return Err(format!(
            "expected `Message #1 saved.` after E ALL, got {:?}",
            String::from_utf8_lossy(&after)
        ));
    }
    Ok(())
}

fn read_back_all_message(stream: &mut TcpStream) -> Result<(), String> {
    write_line(stream, b"R 1")?;
    let after = drain_until_capturing(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after R 1: {e}"))?;
    for (needle, description) in [
        (&b"To     \x1b[33m:\x1b[0m ALL"[..], "`To: ALL`"),
        (&b"Recv'd\x1b[33m:\x1b[0m N/A"[..], "`Recv'd: N/A`"),
        (&b"Hi everyone."[..], "broadcast body"),
    ] {
        if !contains(&after, needle) {
            return Err(format!(
                "expected {description} on broadcast read-back, got {:?}",
                String::from_utf8_lossy(&after)
            ));
        }
    }
    Ok(())
}

fn reject_eall_post(stream: &mut TcpStream) -> Result<(), String> {
    write_line(stream, b"E EALL")?;
    drain_until(stream, b"Subject: ").map_err(|e| format!("EALL subject prompt: {e}"))?;
    write_line(stream, b"Echo")?;
    drain_until(stream, b"Private (y/N)? ").map_err(|e| format!("EALL private prompt: {e}"))?;
    write_line(stream, b"N")?;
    drain_until(stream, b"End with a single '.'")
        .map_err(|e| format!("EALL body instructions: {e}"))?;
    write_line(stream, b"Cross-conference notice.")?;
    write_line(stream, b".")?;
    let after = drain_until_capturing(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after E EALL: {e}"))?;
    if !contains(&after, b"This message base does not accept that addressee.") {
        return Err(format!(
            "expected addressing-not-allowed notice after E EALL, got {:?}",
            String::from_utf8_lossy(&after)
        ));
    }
    if contains(&after, b"Message #2 saved.") {
        return Err(format!(
            "EALL must not have been persisted, got {:?}",
            String::from_utf8_lossy(&after)
        ));
    }
    Ok(())
}

fn post_comment_to_sysop(stream: &mut TcpStream) -> Result<(), String> {
    write_line(stream, b"C")?;
    drain_until(stream, b"Subject: ").map_err(|e| format!("C subject prompt: {e}"))?;
    write_line(stream, b"Welcome screen typo")?;
    drain_until(stream, b"End with a single '.'")
        .map_err(|e| format!("C body instructions: {e}"))?;
    write_line(stream, b"There's a typo on the welcome screen.")?;
    write_line(stream, b".")?;
    let after = drain_until_capturing(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after C: {e}"))?;
    // The rejected EALL did not advance highest_message, so the
    // comment-to-sysop is the second persisted mail.
    if !contains(&after, b"Message #2 saved.") {
        return Err(format!(
            "expected `Message #2 saved.` after C, got {:?}",
            String::from_utf8_lossy(&after)
        ));
    }
    Ok(())
}

fn read_back_comment_to_sysop(stream: &mut TcpStream) -> Result<(), String> {
    write_line(stream, b"R 2")?;
    let after = drain_until_capturing(stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after R 2: {e}"))?;
    for (needle, description) in [
        (&b"To     \x1b[33m:\x1b[0m Sysop"[..], "`To: Sysop`"),
        (
            &b"Status\x1b[33m:\x1b[0m Private Message"[..],
            "`Status: Private Message`",
        ),
        (
            &b"There's a typo on the welcome screen."[..],
            "comment-to-sysop body",
        ),
    ] {
        if !contains(&after, needle) {
            return Err(format!(
                "expected {description} on comment-to-sysop read-back, got {:?}",
                String::from_utf8_lossy(&after)
            ));
        }
    }
    Ok(())
}
