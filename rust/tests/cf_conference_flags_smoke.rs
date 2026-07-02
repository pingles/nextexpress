//! Tier C (Slice C5) in-process smoke: the `CF` conference-flags editor.
//!
//! Boots a [`TelnetListener`] in-process (the `tierb_mail_scan_smoke.rs`
//! shape) with two conferences the seeded sysop can access, then drives
//! the `CF` editor over telnet:
//!
//!   * the M/A/F/Z listing shows both conferences with mail- and
//!     file-scan set (the C5 default for a granted membership);
//!   * `M` then `1` toggles conference 1's mail-scan off — its `M` cell
//!     clears on the redraw while file-scan stays set;
//!   * `M` then `*` toggles **every** conference's mail-scan (design D1:
//!     the legacy advertises `*` but no-ops it — `NextExpress` honours the
//!     advertised toggle-all), flipping conference 1 back on and
//!     conference 2 off;
//!   * a non-M/A/F/Z key (`Q`) leaves the editor for the menu.
//!
//! This proves the headline C5 capability is reachable through the same
//! composition root and telnet adapter the binary uses.

mod support;

use nextexpress::domain::conference::{Conference, MessageBase};

use support::{contains, drain_until, end_session, sign_in_seeded_sysop, write_line, TestRuntime};

const MASK_PROMPT: &[u8] = b"[Z]oom >: ";
const EXPR_PROMPT: &[u8] = b"'+' All on >: ";
const HEADER: &[u8] = b"\x1b[32m        M A F Z Conference";

#[tokio::test]
async fn cf_lists_toggles_one_and_star_toggles_all_over_telnet() {
    let addr = spawn_two_conference_listener().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // --- initial listing: both conferences mail+file scan on (default) ---
    write_line(&mut stream, b"CF").await;
    let listing = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&listing, HEADER),
        "missing the CF column header, got {:?}",
        String::from_utf8_lossy(&listing)
    );
    assert!(
        contains(&listing, b"    1\x1b[34m] \x1b[36m*   *   \x1b[0mOne"),
        "conference 1 should show mail+file scan set, got {:?}",
        String::from_utf8_lossy(&listing)
    );
    assert!(
        contains(&listing, b"    2\x1b[34m] \x1b[36m*   *   \x1b[0mTwo"),
        "conference 2 should show mail+file scan set, got {:?}",
        String::from_utf8_lossy(&listing)
    );

    // --- M then `1`: toggle conference 1's mail-scan off ---
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, EXPR_PROMPT).await;
    write_line(&mut stream, b"1").await;
    let after_one = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&after_one, b"    1\x1b[34m] \x1b[36m    *   \x1b[0mOne"),
        "conference 1's mail cell should clear (file stays set), got {:?}",
        String::from_utf8_lossy(&after_one)
    );
    assert!(
        contains(&after_one, b"    2\x1b[34m] \x1b[36m*   *   \x1b[0mTwo"),
        "conference 2 must be unchanged by a `1` edit, got {:?}",
        String::from_utf8_lossy(&after_one)
    );

    // --- M then `*`: toggle-all mail-scan (D1) ---
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, EXPR_PROMPT).await;
    write_line(&mut stream, b"*").await;
    let after_star = drain_until(&mut stream, MASK_PROMPT).await;
    assert!(
        contains(&after_star, b"    1\x1b[34m] \x1b[36m*   *   \x1b[0mOne"),
        "`*` must toggle conference 1's mail cell back on, got {:?}",
        String::from_utf8_lossy(&after_star)
    );
    assert!(
        contains(&after_star, b"    2\x1b[34m] \x1b[36m    *   \x1b[0mTwo"),
        "`*` must toggle conference 2's mail cell off (legacy no-ops this), got {:?}",
        String::from_utf8_lossy(&after_star)
    );

    // --- a non-M/A/F/Z key leaves the editor for the menu ---
    write_line(&mut stream, b"Q").await;
    let menu = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&menu, b"[\x1b[36m1\x1b[34m:\x1b[36mOne\x1b[0m]"),
        "CF must return to the menu in conference 1, got {:?}",
        String::from_utf8_lossy(&menu)
    );

    end_session(&mut stream).await;
}

/// Builds a `Runtime` with two conferences (both accessible to the
/// seeded sysop) and empty in-memory mail stores, then binds a
/// [`TelnetListener`] and spawns its accept loop.
async fn spawn_two_conference_listener() -> std::net::SocketAddr {
    let conferences = vec![
        Conference::new(
            1,
            "One".to_string(),
            vec![MessageBase::new(1, 1, "general".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Two".to_string(),
            vec![MessageBase::new(2, 1, "general".to_string())],
        )
        .expect("valid conference"),
    ];
    support::spawn_seeded_sysop(TestRuntime::new(
        std::path::PathBuf::from("."),
        conferences,
        support::empty_mail_stores(),
        support::empty_file_repo(),
    ))
    .await
}
