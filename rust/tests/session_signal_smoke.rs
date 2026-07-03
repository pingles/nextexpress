//! In-process smoke for the per-session signal lane (July 2026
//! review, item 26): another task delivers bytes into a session parked
//! at the menu prompt, and the session's read loop resumes intact.
//! This is the seam Tier E's `OLM`/page delivery and Tier G's kick
//! ride on; the sender side here stands in for those future handlers.

mod support;

use nextexpress::app::terminal::SessionSignal;
use support::{
    contains, drain_until, empty_file_repo, empty_mail_stores, end_session, read_idle,
    sign_in_seeded_sysop, spawn_seeded_sysop_with_pool, write_line, TestRuntime,
};

use nextexpress::domain::conference::{Conference, MessageBase};

fn one_conference() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid conference")]
}

#[tokio::test]
async fn delivery_reaches_a_session_parked_at_the_menu_prompt() {
    let (addr, pool) = spawn_seeded_sysop_with_pool(TestRuntime::new(
        std::env::temp_dir(),
        one_conference(),
        empty_mail_stores(),
        empty_file_repo(),
    ))
    .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // The session sits blocked in its menu-prompt read; the pool hands
    // out its signal sender — the handle a future OLM handler will use.
    let sender = pool
        .signal_sender(1)
        .await
        .expect("node 1 carries a live signal lane");
    sender
        .send(SessionSignal::Deliver(
            b"\r\n*OLM from node 2*\r\n".to_vec(),
        ))
        .expect("session receiver is alive");

    let delivered = read_idle(&mut stream, std::time::Duration::from_millis(500)).await;
    assert!(
        contains(&delivered, b"*OLM from node 2*"),
        "the delivery reaches the parked session; got {:?}",
        String::from_utf8_lossy(&delivered)
    );

    // The interrupted read resumed: the next command still works.
    write_line(&mut stream, b"T").await;
    let response = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&response, b"It is "),
        "T executes normally after the delivery; got {:?}",
        String::from_utf8_lossy(&response)
    );
    end_session(&mut stream).await;
}

#[tokio::test]
async fn release_clears_the_signal_lane() {
    let (addr, pool) = spawn_seeded_sysop_with_pool(TestRuntime::new(
        std::env::temp_dir(),
        one_conference(),
        empty_mail_stores(),
        empty_file_repo(),
    ))
    .await;
    let mut stream = sign_in_seeded_sysop(&addr).await;
    assert!(
        pool.signal_sender(1).await.is_some(),
        "a live session carries a sender"
    );
    end_session(&mut stream).await;
    // The logoff releases the node; poll briefly for the teardown.
    for _ in 0..50 {
        if pool.signal_sender(1).await.is_none() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("release must clear the node's signal sender");
}
