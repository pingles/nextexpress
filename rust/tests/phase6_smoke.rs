//! Phase 6 in-process smoke test (Slice 41a).
//!
//! Boots a `TelnetListener` in-process against a temp BBS path
//! pre-populated with a `Conf01/MsgBase/0000001.json`, then drives the
//! Phase 6 read flow over real telnet.

mod support;

use std::path::Path;

use nextexpress::domain::conference::{Conference, MessageBase};

use support::{
    contains, drain_until, empty_file_repo, file_mail_stores, spawn_seeded_sysop, write_line,
    TestRuntime,
};

#[tokio::test]
async fn listener_walks_phase6_mail_read_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    seed_conf01_with_one_unread_message(dir.path());
    let conferences = phase6_conferences();
    let mail_stores = file_mail_stores(dir.path(), &conferences);
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        mail_stores,
        empty_file_repo(),
    ))
    .await;

    let (mut stream, post_auth) = support::sign_in_seeded_sysop_declining_logon_scan(&addr).await;
    assert!(
        contains(&post_auth, b"Scanning conferences for mail"),
        "expected the logon conference scan header, got {:?}",
        String::from_utf8_lossy(&post_auth)
    );
    assert!(
        contains(&post_auth, b"Welcome to"),
        "expected the seeded message in the logon scan listing, got {:?}",
        String::from_utf8_lossy(&post_auth)
    );

    write_line(&mut stream, b"R 1").await;
    let post_r = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&post_r, b"Subject") && contains(&post_r, b"Welcome to NextExpress"),
        "expected ReadMail header + subject, got {:?}",
        String::from_utf8_lossy(&post_r)
    );
    assert!(
        contains(&post_r, b"Hello sysop, this is your first message."),
        "expected ReadMail to render body, got {:?}",
        String::from_utf8_lossy(&post_r)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    write_line(&mut stream, b"J 1").await;
    let post_j = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&post_j, b"New mail in this conference"),
        "auto scan-on-join must not render SCREEN_MAILSCAN when unread_count is zero, got {:?}",
        String::from_utf8_lossy(&post_j)
    );
    assert!(
        contains(&post_j, b"No new mail."),
        "expected `No new mail.` summary after zero-unread re-join, got {:?}",
        String::from_utf8_lossy(&post_j)
    );

    support::end_session(&mut stream).await;
}

fn phase6_conferences() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid Conf01")]
}

/// Seeds a single-msgbase Conf01 with one message addressed to the
/// seeded sysop (slot 1).
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
