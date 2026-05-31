//! Phase 8 binary smoke tests — reply / forward / kill / move / edit
//! header via the `R` read sub-prompt.
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path
//! pre-populated with a `Conf01/` and a single seed message, then drives
//! the mail-management operations over real telnet. Tier B B8 retired
//! the standalone `RP` / `FW` / `K` / `MV` / `EH` menu commands — these
//! operations now live inside the `readMSG` sub-prompt, so each flow
//! reads a message (`R <n>`) and then types the sub-prompt option.
//!
//! ## Reply / forward
//!   1. Post a fresh original via `E sysop` (`Message #2 saved.`).
//!   2. `R 2` -> `R` replies; the reply posts `Message #3 saved.` and
//!      the loop advances onto it, showing the `Re: ` subject + body.
//!   3. `R 2` -> `F` forwards back to the sysop with a `--`-separated
//!      note (`Message #4 saved.`); `R 4` shows the `Fwd: ` prefix, the
//!      `forward_header_for` block, the original body and the note.
//!
//! ## Sysop edit / move / delete
//!   1. `R 1` -> `EH` rewrites the subject and re-displays it in place.
//!   2. `M` moves the edited mail from msgbase #1 to #2; the new number
//!      lands at `target.highest_message + 1`.
//!   3. `E sysop` posts a fresh row; `R 2` -> `D` soft-deletes it, and a
//!      re-read surfaces the deletion notice.

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

    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ")
        .map_err(|e| format!("Graphics prompt: {e}"))?;
    write_line(&mut stream, b"Y")?;
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

    // Step 2: reply via the sub-prompt (Tier B B8 retired the top-level
    // `RP`). `R 2` -> `R` -> body -> `.`. The reply posts msg 3 and the
    // sub-prompt advances to it, so the reply is displayed inline with
    // the `Re: ` subject and the typed body.
    write_line(&mut stream, b"R 2")?;
    drain_until(&mut stream, b">: ").map_err(|e| format!("sub-prompt after R 2: {e}"))?;
    write_line(&mut stream, b"R")?;
    drain_until(&mut stream, b"End with a single '.'")
        .map_err(|e| format!("reply body instructions: {e}"))?;
    write_line(&mut stream, b"Replying inline.")?;
    write_line(&mut stream, b".")?;
    let post_reply = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("sub-prompt after reply: {e}"))?;
    for (needle, what) in [
        (&b"Message #3 saved."[..], "reply-saved notice"),
        (&b"Re: Phase 8 source"[..], "`Re: ` subject"),
        (&b"Replying inline."[..], "reply body"),
    ] {
        if !contains(&post_reply, needle) {
            return Err(format!(
                "expected {what} after the sub-prompt reply, got {:?}",
                String::from_utf8_lossy(&post_reply)
            ));
        }
    }
    write_line(&mut stream, b"Q")?;
    drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu after reply Q: {e}"))?;

    // Step 3: forward the original (msg 2) via the sub-prompt. `R 2` ->
    // `F` -> To: sysop -> note -> `.`. Forward stays on msg 2.
    write_line(&mut stream, b"R 2")?;
    drain_until(&mut stream, b">: ").map_err(|e| format!("sub-prompt after R 2 (fwd): {e}"))?;
    write_line(&mut stream, b"F")?;
    drain_until(&mut stream, b"Forward to: ").map_err(|e| format!("forward To prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"blank line skips")
        .map_err(|e| format!("forward note instructions: {e}"))?;
    write_line(&mut stream, b"Please look at this.")?;
    write_line(&mut stream, b".")?;
    let post_fwd = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("sub-prompt after forward: {e}"))?;
    if !contains(&post_fwd, b"Message #4 saved.") {
        return Err(format!(
            "expected `Message #4 saved.` after the sub-prompt forward, got {:?}",
            String::from_utf8_lossy(&post_fwd)
        ));
    }
    write_line(&mut stream, b"Q")?;
    drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu after forward Q: {e}"))?;

    // Step 4: read back the forward (msg 4): the `Fwd: ` subject, the
    // spec's `forward_header_for` block, the original body, and the
    // `--`-separated note.
    write_line(&mut stream, b"R 4")?;
    let post_r4 = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("read sub-prompt after R 4: {e}"))?;
    for (needle, what) in [
        (&b"Fwd: Phase 8 source"[..], "`Fwd: ` subject"),
        (&b"From: sysop"[..], "forward_header_for `From:`"),
        (
            &b"Subject: Phase 8 source"[..],
            "forward_header_for `Subject:`",
        ),
        (&b"Phase 8 original body."[..], "original body"),
        (&b"--"[..], "note separator"),
        (&b"Please look at this."[..], "note"),
    ] {
        if !contains(&post_r4, needle) {
            return Err(format!(
                "expected {what} on the forwarded message, got {:?}",
                String::from_utf8_lossy(&post_r4)
            ));
        }
    }
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

    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ")
        .map_err(|e| format!("Graphics prompt: {e}"))?;
    write_line(&mut stream, b"Y")?;
    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"mins. left): ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;

    // Step 1: `EH` via the sub-prompt rewrites the seed mail's subject,
    // and the option re-displays the edited message in place (Tier B B8
    // retired the top-level `EH`). `R 1` -> `EH`.
    write_line(&mut stream, b"R 1")?;
    drain_until(&mut stream, b">: ").map_err(|e| format!("sub-prompt after R 1: {e}"))?;
    write_line(&mut stream, b"EH")?;
    drain_until(&mut stream, b"New subject (blank = unchanged): ")
        .map_err(|e| format!("EH subject prompt: {e}"))?;
    write_line(&mut stream, b"Sysop-edited subject")?;
    drain_until(&mut stream, b"New To (blank = unchanged): ")
        .map_err(|e| format!("EH To prompt: {e}"))?;
    write_line(&mut stream, b"")?; // keep current addressee
    let post_eh = drain_until_capturing(&mut stream, b">: ")
        .map_err(|e| format!("sub-prompt after EH: {e}"))?;
    if !contains(&post_eh, b"Header updated.") || !contains(&post_eh, b"Sysop-edited subject") {
        return Err(format!(
            "expected `Header updated.` and the edited subject re-displayed, got {:?}",
            String::from_utf8_lossy(&post_eh)
        ));
    }

    // Step 2: `M`ove the edited mail (still the current message) from
    // (1,1) to (1,2). The base holds only this one message, so the
    // post-move advance hits the out-of-range clamp and returns to the
    // menu.
    write_line(&mut stream, b"M")?;
    drain_until(&mut stream, b"Target conference number: ")
        .map_err(|e| format!("move conference prompt: {e}"))?;
    write_line(&mut stream, b"1")?;
    drain_until(&mut stream, b"Target msgbase number: ")
        .map_err(|e| format!("move msgbase prompt: {e}"))?;
    write_line(&mut stream, b"2")?;
    let post_mv = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu after move: {e}"))?;
    if !contains(&post_mv, b"Message moved. New number 1.") {
        return Err(format!(
            "expected `Message moved. New number 1.` after the sub-prompt move, got {:?}",
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

    // Step 4: `D`elete the fresh mail via the sub-prompt. `R 2` -> `D`
    // -> confirm `y`. With only this message in the base, the post-delete
    // advance clamps back to the menu.
    write_line(&mut stream, b"R 2")?;
    drain_until(&mut stream, b">: ").map_err(|e| format!("sub-prompt after R 2: {e}"))?;
    write_line(&mut stream, b"D")?;
    drain_until(&mut stream, b"Delete message (y/N)? ")
        .map_err(|e| format!("confirm delete prompt: {e}"))?;
    write_line(&mut stream, b"y")?;
    let post_k = drain_until_capturing(&mut stream, b"mins. left): ")
        .map_err(|e| format!("menu after delete: {e}"))?;
    if !contains(&post_k, b"Message deleted.") {
        return Err(format!(
            "expected `Message deleted.` after the sub-prompt delete, got {:?}",
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
