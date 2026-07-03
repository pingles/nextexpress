//! Two concurrent sessions on one account must both persist their
//! logons (SYSTEM.md item 1: command-style user writes).
//!
//! Before the command cutover, `finalise_logoff` wrote the session's
//! whole stale user aggregate back to storage, so the interleaving
//! below — A signs in, B signs in, B logs off, then A's stale logoff
//! lands last — silently reverted B's `times_called` bump. Command
//! writes are additive deltas, so both logons survive.

mod support;

use nextexpress::domain::conference::{Conference, MessageBase};

use support::{contains, drain_until, end_session, sign_in_seeded_sysop, write_line, TestRuntime};

#[tokio::test]
async fn concurrent_same_account_logons_both_persist() {
    let conferences = vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid conference")];
    let fixture = TestRuntime::new(
        std::env::current_dir().expect("cwd"),
        conferences,
        support::empty_mail_stores(),
        support::empty_file_repo(),
    )
    .with_config(|c| c.max_nodes = 2);
    let addr = support::spawn_seeded_sysop(fixture).await;

    let mut a = sign_in_seeded_sysop(&addr).await; // logon #1
    let mut b = sign_in_seeded_sysop(&addr).await; // logon #2
                                                   // B logs off first so A's stale session writes last.
    end_session(&mut b).await;
    end_session(&mut a).await;

    // Logon #3: the S screen counts all three calls. Under the old
    // whole-aggregate save, A's logoff reverted B's bump and this
    // read `2`.
    let mut c = sign_in_seeded_sysop(&addr).await;
    write_line(&mut c, b"S").await;
    let post_s = drain_until(&mut c, b"mins. left): ").await;
    let needle: &[u8] = b"\x1b[32m# Times On \x1b[33m:\x1b[0m 3\r\n";
    assert!(
        contains(&post_s, needle),
        "expected the third logon to show `# Times On : 3`, got {:?}",
        String::from_utf8_lossy(&post_s)
    );
    end_session(&mut c).await;
}
