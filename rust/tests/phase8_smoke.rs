//! Phase 8 binary smoke tests (Slices 49a + 49b).
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path
//! pre-populated with a `Conf01/` and a single seed message, then
//! drives the new Phase 8 menu commands over real telnet:
//!
//! ## Slice 49a — `RP` (reply) and `FW` (forward)
//!   1. Sign in as the seeded `sysop` / `sysop`.
//!   2. Auto-rejoin attaches the session to Conf01.
//!   3. Post a fresh original via `E sysop` so the smoke test owns
//!      a known recipient (the existing seed mail #1 is also
//!      authored by the sysop).
//!   4. `RP 2` replies to the freshly-posted mail. The body editor
//!      collects the reply text; the rule defaults the subject to
//!      `"Re: <original.subject>"` and the addressee to the source
//!      author. Posting reports `Message #3 saved.`.
//!   5. `R 3` rereads the reply to confirm the spec's defaults
//!      landed on the wire: subject prefixed `Re: `, recipient =
//!      original author (`sysop`).
//!   6. `FW 2 / sysop / a note / .` forwards the mail back to the
//!      sysop with a `--`-separated note. Posting reports
//!      `Message #4 saved.`. Reading mail #4 shows the
//!      `Fwd: <subject>` prefix, the spec's `forward_header_for`
//!      block (`From:` / `Date:` / `Subject:` lines) prepended to
//!      the original body, and the note after the separator.
//!   7. `G` ends the session cleanly.
//!
//! ## Slice 49b — sysop `K`, `MV`, `EH`
//!   1. Sign in as the seeded `sysop` / `sysop`.
//!   2. `K 1` with confirm `y` soft-deletes the seed mail.
//!   3. `R 1` confirms the row no longer renders to ordinary
//!      readers (the sysop sees it via its own re-read path).
//!   4. `EH 1` rewrites the subject; `R 1` confirms.
//!   5. `MV 1` moves mail #1 from msgbase #1 to msgbase #2
//!      (registered ahead of time); the smoke confirms the new
//!      number lands at `target.highest_message + 1`.
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
fn binary_walks_phase8_reply_forward_flow_over_telnet() {
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
        walk_phase8_reply_forward_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "cohesive end-to-end script"
)]
fn walk_phase8_reply_forward_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    drain_until(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;

    // Step 1: post an original we control via `E sysop`. The seed
    // mail (#1) is also from the sysop but its subject doesn't carry
    // the marker we want to scan for after a reply.
    write_line(&mut stream, b"E sysop")?;
    drain_until(&mut stream, b"Subject: ").map_err(|e| format!("E Subject prompt: {e}"))?;
    write_line(&mut stream, b"Phase 8 source")?;
    drain_until(&mut stream, b"Private (y/N)? ").map_err(|e| format!("Private prompt: {e}"))?;
    write_line(&mut stream, b"N")?;
    drain_until(&mut stream, b"End with a single '.'")
        .map_err(|e| format!("Body instructions: {e}"))?;
    write_line(&mut stream, b"Phase 8 original body.")?;
    write_line(&mut stream, b".")?;
    let post_e = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after E: {e}"))?;
    if !contains(&post_e, b"Message #2 saved.") {
        return Err(format!(
            "expected `Message #2 saved.` after E, got {:?}",
            String::from_utf8_lossy(&post_e)
        ));
    }

    // Step 2: reply to it. RP 2 → body editor → `.`. Subject and
    // addressee come from the source.
    write_line(&mut stream, b"RP 2")?;
    drain_until(&mut stream, b"End with a single '.'")
        .map_err(|e| format!("RP body instructions: {e}"))?;
    write_line(&mut stream, b"Replying inline.")?;
    write_line(&mut stream, b".")?;
    let post_rp = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after RP: {e}"))?;
    if !contains(&post_rp, b"Message #3 saved.") {
        return Err(format!(
            "expected `Message #3 saved.` after RP, got {:?}",
            String::from_utf8_lossy(&post_rp)
        ));
    }

    // Step 3: read back the reply. Subject must carry the `Re: `
    // prefix and the body must be the user's input.
    write_line(&mut stream, b"R 3")?;
    let post_r3 = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("read sub-prompt after R 3: {e}"))?;
    if !contains(&post_r3, b"Re: Phase 8 source") {
        return Err(format!(
            "expected the reply to carry `Re: Phase 8 source`, got {:?}",
            String::from_utf8_lossy(&post_r3)
        ));
    }
    if !contains(&post_r3, b"Replying inline.") {
        return Err(format!(
            "expected the reply body on R 3, got {:?}",
            String::from_utf8_lossy(&post_r3)
        ));
    }
    // Tier B B4: leave the read sub-prompt with `Q`.
    write_line(&mut stream, b"Q")?;
    drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu prompt after R 3 sub-prompt Q: {e}"))?;

    // Step 4: forward the original to sysop with a note. FW 2 →
    // To: sysop → note line → `.`.
    write_line(&mut stream, b"FW 2")?;
    drain_until(&mut stream, b"Forward to: ").map_err(|e| format!("FW To prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"blank line skips")
        .map_err(|e| format!("FW note instructions: {e}"))?;
    write_line(&mut stream, b"Please look at this.")?;
    write_line(&mut stream, b".")?;
    let post_fw = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after FW: {e}"))?;
    if !contains(&post_fw, b"Message #4 saved.") {
        return Err(format!(
            "expected `Message #4 saved.` after FW, got {:?}",
            String::from_utf8_lossy(&post_fw)
        ));
    }

    // Step 5: read back the forward. Subject must be prefixed
    // `Fwd: `, the spec's `forward_header_for` block must
    // prepend to the body, the original body must appear, and
    // the note must follow the `--` separator.
    write_line(&mut stream, b"R 4")?;
    let post_r4 = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("read sub-prompt after R 4: {e}"))?;
    if !contains(&post_r4, b"Fwd: Phase 8 source") {
        return Err(format!(
            "expected the forward to carry `Fwd: Phase 8 source`, got {:?}",
            String::from_utf8_lossy(&post_r4)
        ));
    }
    if !contains(&post_r4, b"From: sysop") {
        return Err(format!(
            "expected forward_header_for `From: sysop` line, got {:?}",
            String::from_utf8_lossy(&post_r4)
        ));
    }
    if !contains(&post_r4, b"Subject: Phase 8 source") {
        return Err(format!(
            "expected forward_header_for `Subject:` line, got {:?}",
            String::from_utf8_lossy(&post_r4)
        ));
    }
    if !contains(&post_r4, b"Phase 8 original body.") {
        return Err(format!(
            "expected forward to include the original body, got {:?}",
            String::from_utf8_lossy(&post_r4)
        ));
    }
    if !contains(&post_r4, b"--") || !contains(&post_r4, b"Please look at this.") {
        return Err(format!(
            "expected forward to carry the `--` separator + note, got {:?}",
            String::from_utf8_lossy(&post_r4)
        ));
    }
    // Tier B B4: leave the read sub-prompt with `Q` before logging off.
    write_line(&mut stream, b"Q")?;
    drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu prompt after R 4 sub-prompt Q: {e}"))?;

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}

// ─── Test plumbing (shared shape with phase7_smoke) ───────────

fn seed_conf01_with_one_message(bbs_path: &Path) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    let conference_toml = "number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n";
    std::fs::write(conf01.join("conference.toml"), conference_toml.as_bytes())
        .expect("write Conf01/conference.toml");
    std::fs::write(conf01.join("menu.txt"), b"CONF1-MENU\r\n").expect("write Conf01/menu.txt");

    let msgbase = conf01.join("MsgBase");
    std::fs::create_dir_all(&msgbase).expect("create MsgBase");
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn binary_walks_phase8_sysop_admin_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_conf01_two_msgbases(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_phase8_sysop_admin_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("sysop smoke test failed: {message}");
    }
}

fn seed_conf01_two_msgbases(bbs_path: &Path) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    let conference_toml = concat!(
        "number = 1\n",
        "name = \"Main\"\n",
        "[[msgbase]]\n",
        "number = 1\n",
        "name = \"main\"\n",
        "[[msgbase]]\n",
        "number = 2\n",
        "name = \"archive\"\n",
    );
    std::fs::write(conf01.join("conference.toml"), conference_toml.as_bytes())
        .expect("write Conf01/conference.toml");
    std::fs::write(conf01.join("menu.txt"), b"CONF1-MENU\r\n").expect("write Conf01/menu.txt");

    let mb1 = conf01.join("MsgBase");
    std::fs::create_dir_all(&mb1).expect("create MsgBase");
    let seed1 = r#"{
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
        "body": "Seed body to be admin-touched.\n"
    }"#;
    std::fs::write(mb1.join("0000001.json"), seed1).expect("write seed mail #1");

    // Second msgbase exists but is empty — it's the move target.
    let mb2 = conf01.join("MsgBase2");
    std::fs::create_dir_all(&mb2).expect("create MsgBase2");
}

fn walk_phase8_sysop_admin_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;

    // Step 1: EH 1 rewrites the seed mail's subject; R 1 confirms
    // the new subject. We do EH before K so the post-edit row is
    // still alive to read.
    write_line(&mut stream, b"EH 1")?;
    drain_until(&mut stream, b"New subject (blank = unchanged): ")
        .map_err(|e| format!("EH subject prompt: {e}"))?;
    write_line(&mut stream, b"Sysop-edited subject")?;
    drain_until(&mut stream, b"New To (blank = unchanged): ")
        .map_err(|e| format!("EH To prompt: {e}"))?;
    write_line(&mut stream, b"")?; // keep current addressee
    let post_eh = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after EH: {e}"))?;
    if !contains(&post_eh, b"Header updated.") {
        return Err(format!(
            "expected `Header updated.` after EH, got {:?}",
            String::from_utf8_lossy(&post_eh)
        ));
    }

    write_line(&mut stream, b"R 1")?;
    let post_r_after_eh = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("read sub-prompt after R 1 (post-EH): {e}"))?;
    if !contains(&post_r_after_eh, b"Sysop-edited subject") {
        return Err(format!(
            "expected R 1 to show the edited subject, got {:?}",
            String::from_utf8_lossy(&post_r_after_eh)
        ));
    }
    // Tier B B4: leave the read sub-prompt with `Q` before the move.
    write_line(&mut stream, b"Q")?;
    drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu prompt after R 1 sub-prompt Q: {e}"))?;

    // Step 2: MV 1 moves the mail from (1,1) to (1,2). The target
    // is empty so the new number is 1.
    write_line(&mut stream, b"MV 1")?;
    drain_until(&mut stream, b"Target conference number: ")
        .map_err(|e| format!("MV conference prompt: {e}"))?;
    write_line(&mut stream, b"1")?;
    drain_until(&mut stream, b"Target msgbase number: ")
        .map_err(|e| format!("MV msgbase prompt: {e}"))?;
    write_line(&mut stream, b"2")?;
    let post_mv = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after MV: {e}"))?;
    if !contains(&post_mv, b"Message moved. New number 1.") {
        return Err(format!(
            "expected `Message moved. New number 1.` after MV, got {:?}",
            String::from_utf8_lossy(&post_mv)
        ));
    }

    // Step 3: now the source's #1 is soft-deleted by the move. Post
    // a fresh mail with `E sysop` so we have a live row to delete.
    write_line(&mut stream, b"E sysop")?;
    drain_until(&mut stream, b"Subject: ").map_err(|e| format!("E Subject prompt: {e}"))?;
    write_line(&mut stream, b"To be killed")?;
    drain_until(&mut stream, b"Private (y/N)? ").map_err(|e| format!("Private prompt: {e}"))?;
    write_line(&mut stream, b"N")?;
    drain_until(&mut stream, b"End with a single '.'")
        .map_err(|e| format!("Body instructions: {e}"))?;
    write_line(&mut stream, b"Doomed.")?;
    write_line(&mut stream, b".")?;
    let post_e = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after E: {e}"))?;
    if !contains(&post_e, b"Message #2 saved.") {
        return Err(format!(
            "expected `Message #2 saved.` after E, got {:?}",
            String::from_utf8_lossy(&post_e)
        ));
    }

    // Step 4: K 2 with confirm y deletes it.
    write_line(&mut stream, b"K 2")?;
    drain_until(&mut stream, b"Delete message (y/N)? ")
        .map_err(|e| format!("Confirm delete prompt: {e}"))?;
    write_line(&mut stream, b"y")?;
    let post_k = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after K: {e}"))?;
    if !contains(&post_k, b"Message deleted.") {
        return Err(format!(
            "expected `Message deleted.` after K, got {:?}",
            String::from_utf8_lossy(&post_k)
        ));
    }

    // Step 5: R 2 now reports the source-deleted line via the read
    // path (mail visibility = Deleted → read denied).
    write_line(&mut stream, b"R 2")?;
    let post_r = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after R 2 (post-K): {e}"))?;
    if !contains(&post_r, b"deleted") {
        return Err(format!(
            "expected R 2 after delete to surface a deletion notice, got {:?}",
            String::from_utf8_lossy(&post_r)
        ));
    }

    write_line(&mut stream, b"G")?;
    drain_until(&mut stream, b"Goodbye").map_err(|e| format!("Goodbye line: {e}"))?;
    Ok(())
}
