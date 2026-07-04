//! Slice D8 `FS` (file status) in-process smoke — FAITHFUL DENY.
//!
//! `internalCommandFS` (`amiexpress/express.e:24871-24874`) gates on
//! `ACS_CONFERENCE_ACCOUNTING`; on the shipped reference board no
//! account holds the right — sysop sec 255 included — so **every** `FS`
//! form denies with `higherAccess()` (`express.e:3038`, printed by the
//! dispatcher tail at `:28400`). Captured live in
//! `comparison/transcripts/ae_tierd_fs.txt`; `NextExpress` mirrors the
//! outcome with an unconditional deny (no gate, no granted branch —
//! `designs/2026-07-04-fs-design.md` §7). The granted `fileStatus(0)`
//! accounting table is slice A11's surface.
//!
//! One scenario per grammar row G1–G5. Each asserts the echoed input
//! followed by the restated deny literal, and that no accounting table
//! leaked (`Uploads` is the table's section head — capture line 63).

mod support;

use nextexpress::domain::conference::{Conference, MessageBase};
use tokio::net::TcpStream;

use support::{contains, drain_until, end_session_forced, write_line, TestRuntime};

/// The `higherAccess()` deny as one restated single-line literal —
/// independent of the production `HIGHER_ACCESS_LINE` const
/// (`ae_tierd_fs.txt:101-103`, legacy `\b\n` → wire `\r\n`).
const HIGHER_ACCESS_DENY: &[u8] = b"\r\nCommand requires higher access.\r\n";

/// The accounting table's section head (`fileStatus`,
/// `amiexpress/express.e:24156`; captured at login,
/// `ae_tierd_fs.txt:63`). Its absence proves the deny never fell
/// through to a table render.
const TABLE_HEAD: &[u8] = b"Uploads";

#[tokio::test]
async fn fs_bare_denies_with_higher_access() {
    // G1 — bare `FS` (`ae_tierd_fs.txt` "D8: FS (bare, from conf 2)"):
    // echo, then the deny, then straight back to the menu prompt.
    let addr = spawn_listener_with_two_conferences().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"FS").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert_denied(&out, b"FS\r\n\r\nCommand requires higher access.\r\n");

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn fs_case_and_argument_forms_deny_identically() {
    // G2 (`fs`, edge R13), G3 (`FS 1`, edge R7), G4 (`FS xyz`, edge
    // R14): case folds and arguments are discarded —
    // `internalCommandFS()` takes no params, and the gate denies before
    // any output difference (`ae_tierd_fs.txt`).
    let addr = spawn_listener_with_two_conferences().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"fs").await;
    let lower = drain_until(&mut stream, b"mins. left): ").await;
    assert_denied(&lower, b"fs\r\n\r\nCommand requires higher access.\r\n");

    write_line(&mut stream, b"FS 1").await;
    let numeric = drain_until(&mut stream, b"mins. left): ").await;
    assert_denied(&numeric, b"FS 1\r\n\r\nCommand requires higher access.\r\n");

    write_line(&mut stream, b"FS xyz").await;
    let junk = drain_until(&mut stream, b"mins. left): ").await;
    assert_denied(&junk, b"FS xyz\r\n\r\nCommand requires higher access.\r\n");

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn fs_after_conference_join_denies_identically() {
    // G5 — `FS` from a different current conference (`ae_tierd_fs.txt`
    // "D8 xref: FS (from conf 1)"): the deny bytes are identical; only
    // the (volatile) menu-prompt conference fields differ, asserted via
    // the prompt suffix rather than the captured literal (§10.6).
    let addr = spawn_listener_with_two_conferences().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"J 2").await;
    let joined = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&joined, b"Joining Conference"),
        "expected the join notice before FS, got {:?}",
        String::from_utf8_lossy(&joined)
    );

    write_line(&mut stream, b"FS").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert_denied(&out, b"FS\r\n\r\nCommand requires higher access.\r\n");

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn utf8_gate_every_fs_session_byte_decodes() {
    // Encoding policy (AGENTS.md): the wire is valid UTF-8. The D8
    // surface is pure ASCII (design §6), so this gate is expected
    // trivially green — it pins that expectation.
    let addr = spawn_listener_with_two_conferences().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    let mut all = Vec::new();
    write_line(&mut stream, b"FS").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"FS xyz").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    assert!(
        std::str::from_utf8(&all).is_ok(),
        "session stream contains non-UTF-8 bytes: {:?}",
        String::from_utf8_lossy(&all)
    );

    end_session_forced(&mut stream).await;
}

/// Asserts one denied `FS` round: the echoed input runs straight into
/// the deny line (`expected_echo_and_deny`, a restated capture literal),
/// and no accounting table leaked.
fn assert_denied(out: &[u8], expected_echo_and_deny: &[u8]) {
    assert!(
        contains(out, expected_echo_and_deny),
        "expected `{}`, got {:?}",
        String::from_utf8_lossy(expected_echo_and_deny),
        String::from_utf8_lossy(out)
    );
    assert!(
        contains(out, HIGHER_ACCESS_DENY),
        "expected the higherAccess() deny, got {:?}",
        String::from_utf8_lossy(out)
    );
    assert!(
        !contains(out, TABLE_HEAD),
        "FS must never render the accounting table (A11's surface), got {:?}",
        String::from_utf8_lossy(out)
    );
}

/// Boots an in-process listener with the seeded sysop and two
/// conferences, so the G5 scenario can `J 2` before `FS`. No file
/// catalogue is needed — the deny never touches the file repo.
async fn spawn_listener_with_two_conferences() -> std::net::SocketAddr {
    let conferences = vec![
        Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Other".to_string(),
            vec![MessageBase::new(2, 1, "main".to_string())],
        )
        .expect("valid conference"),
    ];
    support::spawn_seeded_sysop(TestRuntime::new(
        std::env::temp_dir(),
        conferences,
        support::empty_mail_stores(),
        support::empty_file_repo(),
    ))
    .await
}

/// Connects to `addr`, walks the auth handshake as the seeded
/// `sysop` / `sysop`, and returns the stream at the menu prompt.
async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    support::sign_in_seeded_sysop(addr).await
}
