//! Phase 4 binary smoke test (Slice 34a).
//!
//! Spawns the compiled `nextexpress` binary against a temp BBS path
//! pre-populated with two `Conf<NN>/conference.toml` files, then
//! drives the full Phase 4 flow over real telnet:
//!
//!   1. Sign in as the seeded `sysop` / `sysop`.
//!   2. Auto-rejoin attaches the session to Conf01 and the listener
//!      writes the JOINED screen + the per-conference menu loaded
//!      from `Conf01/menu.txt`.
//!   3. `J 2` switches to Conf02; the JOIN / JOINED screens render
//!      and the per-conference menu changes.
//!   4. `G` ends the session cleanly.
//!
//! Library-level slice tests assert each piece in isolation; this
//! test proves a fresh `cargo run` actually delivers the headline
//! "Conferences (read)" capability.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const NEEDLE_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn binary_walks_phase4_conference_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("nextexpress.toml");
    let toml = format!(
        "port = 0\nmax_nodes = 1\nbbs_path = {}\nmax_password_failures = 3\n",
        toml_string(dir.path()),
    );
    std::fs::write(&config_path, toml).expect("write config");
    seed_two_conferences(dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_nextexpress"))
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");

    let outcome = (|| -> Result<(), String> {
        let addr = read_listen_addr(&mut child)?;
        walk_phase4_conference_flow(&addr)
    })();

    let _ = child.kill();
    let _ = child.wait();

    if let Err(message) = outcome {
        panic!("smoke test failed: {message}");
    }
}

/// Writes Conf01 + Conf02 + Conf03 with distinguishable
/// per-conference menus so the smoke can prove which conference the
/// session is attached to without having to parse the JOINED screen
/// specifically. Conf03 declares `accepted_name_type = "real_name"`
/// so the smoke can verify the listener renders `SCREEN_REALNAMES`
/// on promotion (Slice 34).
fn seed_two_conferences(bbs_path: &Path) {
    let conf01 = bbs_path.join("Conf01");
    std::fs::create_dir_all(&conf01).expect("create Conf01");
    std::fs::write(
        conf01.join("conference.toml"),
        b"number = 1\nname = \"Main\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write Conf01/conference.toml");
    std::fs::write(conf01.join("menu.txt"), b"CONF1-MENU\r\n").expect("write Conf01/menu.txt");

    let conf02 = bbs_path.join("Conf02");
    std::fs::create_dir_all(&conf02).expect("create Conf02");
    std::fs::write(
        conf02.join("conference.toml"),
        b"number = 2\nname = \"Programming\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write Conf02/conference.toml");
    std::fs::write(conf02.join("menu.txt"), b"CONF2-MENU\r\n").expect("write Conf02/menu.txt");

    let conf03 = bbs_path.join("Conf03");
    std::fs::create_dir_all(&conf03).expect("create Conf03");
    std::fs::write(
        conf03.join("conference.toml"),
        b"number = 3\nname = \"Authors\"\naccepted_name_type = \"real_name\"\n[[msgbase]]\nnumber = 1\nname = \"main\"\n",
    )
    .expect("write Conf03/conference.toml");
    std::fs::write(conf03.join("menu.txt"), b"CONF3-MENU\r\n").expect("write Conf03/menu.txt");
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

/// Walks the Phase 4 path: sign-in → auto-rejoin → Conf01 menu →
/// `J 2` → Conf02 menu → `G` → goodbye.
fn walk_phase4_conference_flow(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(PER_READ_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    drain_until(&mut stream, b"Enter your Name: ").map_err(|e| format!("Name prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;
    drain_until(&mut stream, b"PassWord: ").map_err(|e| format!("Password prompt: {e}"))?;
    write_line(&mut stream, b"sysop")?;

    // After auth: the binary writes Authenticated, then runs the
    // auto-rejoin which renders the JOINED screen, then the
    // Conf01 menu.
    let post_auth = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after auto-rejoin: {e}"))?;
    if !contains(&post_auth, b"CONF1-MENU") {
        return Err(format!(
            "expected Conf01 per-conference menu after auto-rejoin, got {:?}",
            String::from_utf8_lossy(&post_auth)
        ));
    }
    // Legacy `joinConf` (`amiexpress/express.e:5073`) emits
    // `Conference <n>: <name> Auto-ReJoined` on auto-rejoin.
    if !contains(&post_auth, b"Conference 1: Main Auto-ReJoined") {
        return Err(format!(
            "expected legacy auto-rejoin announcement after auto-rejoin, got {:?}",
            String::from_utf8_lossy(&post_auth)
        ));
    }

    // Switch to Conf02 with the explicit-join command.
    write_line(&mut stream, b"J 2")?;
    let post_j = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after J 2: {e}"))?;
    if !contains(&post_j, b"CONF2-MENU") {
        return Err(format!(
            "expected Conf02 per-conference menu after explicit join, got {:?}",
            String::from_utf8_lossy(&post_j)
        ));
    }
    // Legacy `joinConf` (`amiexpress/express.e:5083`) emits
    // `\x1b[32mJoining Conference\x1b[33m:\x1b[0m <name>` on
    // explicit join. The ANSI escapes carry colour; the readable
    // text is `Joining Conference: Programming`.
    if !contains(&post_j, b"Joining Conference") || !contains(&post_j, b"Programming") {
        return Err(format!(
            "expected legacy `Joining Conference: Programming` line after `J 2`, got {:?}",
            String::from_utf8_lossy(&post_j)
        ));
    }

    // J 3 promotes display_name_type to RealName (Slice 34); the
    // listener must surface the SCREEN_REALNAMES fallback so the
    // user knows their identity is rendered differently in this
    // conference.
    write_line(&mut stream, b"J 3")?;
    let post_j3 = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after J 3: {e}"))?;
    if !contains(&post_j3, b"CONF3-MENU") {
        return Err(format!(
            "expected Conf03 per-conference menu after `J 3`, got {:?}",
            String::from_utf8_lossy(&post_j3)
        ));
    }
    if !contains(&post_j3, b"real names") {
        return Err(format!(
            "expected real-names notice after promotion to Conf03, got {:?}",
            String::from_utf8_lossy(&post_j3)
        ));
    }

    // J 99 doesn't exist; the resolver falls through to
    // first_accessible (Conf01) and the listener surfaces the
    // legacy "do not have access" notice before the JOINED screen.
    write_line(&mut stream, b"J 99")?;
    let post_j99 = drain_until_capturing(&mut stream, b"Command: ")
        .map_err(|e| format!("Command prompt after J 99: {e}"))?;
    if !contains(&post_j99, b"do not have access") {
        return Err(format!(
            "expected no-access notice after `J 99`, got {:?}",
            String::from_utf8_lossy(&post_j99)
        ));
    }
    if !contains(&post_j99, b"CONF1-MENU") {
        return Err(format!(
            "expected fallback to Conf01 menu after `J 99`, got {:?}",
            String::from_utf8_lossy(&post_j99)
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
