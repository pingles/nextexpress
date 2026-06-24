//! Phase 7 in-process smoke tests (Slices 42 and 44a).
//!
//! Boots a `TelnetListener` in-process against temp BBS paths and drives
//! the Phase 7 write flows over real telnet.

mod support;

use std::path::Path;

use nextexpress::domain::conference::{AllScanScope, AllowedAddressing, Conference, MessageBase};
use tokio::net::TcpStream;

use support::{
    contains, drain_until, empty_file_repo, file_mail_stores, spawn_seeded_sysop, write_line,
    TestRuntime,
};

#[tokio::test]
async fn listener_walks_phase7_mail_post_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    seed_conf01_with_one_message(dir.path());
    let conferences = phase7_conferences();
    let mail_stores = file_mail_stores(dir.path(), &conferences);
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        mail_stores,
        empty_file_repo(),
    ))
    .await;

    let (mut stream, _) = support::sign_in_seeded_sysop_declining_logon_scan(&addr).await;
    walk_phase7_post_flow(&mut stream).await;
    support::end_session(&mut stream).await;
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

async fn walk_phase7_post_flow(stream: &mut TcpStream) {
    // E sysop opens the composer. We send sysop as the recipient
    // inline so the To: prompt is skipped.
    write_line(stream, b"E sysop").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"Hello from the smoke test").await;
    drain_until(stream, b"Private (y/N)? ").await;
    write_line(stream, b"N").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"Body line one.").await;
    write_line(stream, b"Body line two.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;

    let post_e = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_e, b"Message #2 saved."),
        "expected `Message #2 saved.` after E, got {:?}",
        String::from_utf8_lossy(&post_e)
    );

    // R 2 reads the message we just posted, proving it persisted.
    write_line(stream, b"R 2").await;
    let post_r = drain_until(stream, b">: ").await;
    assert!(
        contains(&post_r, b"Hello from the smoke test"),
        "expected R 2 to render the newly posted subject, got {:?}",
        String::from_utf8_lossy(&post_r)
    );
    assert!(
        contains(&post_r, b"Body line one.") && contains(&post_r, b"Body line two."),
        "expected R 2 to render the body lines, got {:?}",
        String::from_utf8_lossy(&post_r)
    );

    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;
}

#[tokio::test]
async fn listener_walks_phase7_broadcast_and_comment_to_sysop_over_telnet() {
    // Slice 44a: end-to-end smoke test for the Phase 7 broadcast and
    // comment-to-sysop flows. The seeded conference forbids EALL via
    // `allowed_addressing = "individual_or_all"`, so the smoke checks
    // both the allowed (`E ALL`) and refused (`E EALL`) branches plus
    // the dedicated `C` command.
    let dir = tempfile::tempdir().expect("tempdir");
    seed_conf01_with_individual_or_all(dir.path());
    let conferences = phase7_individual_or_all_conferences();
    let mail_stores = file_mail_stores(dir.path(), &conferences);
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        mail_stores,
        empty_file_repo(),
    ))
    .await;

    let mut stream = support::sign_in_seeded_sysop(&addr).await;
    post_all_message(&mut stream).await;
    read_back_all_message(&mut stream).await;
    reject_eall_post(&mut stream).await;
    post_comment_to_sysop(&mut stream).await;
    read_back_comment_to_sysop(&mut stream).await;
    support::end_session(&mut stream).await;
}

async fn post_all_message(stream: &mut TcpStream) {
    write_line(stream, b"E ALL").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"Notice to everyone").await;
    drain_until(stream, b"Private (y/N)? ").await;
    write_line(stream, b"N").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"Hi everyone.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;
    let after = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"Message #1 saved."),
        "expected `Message #1 saved.` after E ALL, got {:?}",
        String::from_utf8_lossy(&after)
    );
}

async fn read_back_all_message(stream: &mut TcpStream) {
    write_line(stream, b"R 1").await;
    let after = drain_until(stream, b">: ").await;
    for (needle, description) in [
        (&b"To     \x1b[33m:\x1b[0m ALL"[..], "`To: ALL`"),
        (&b"Recv'd\x1b[33m:\x1b[0m N/A"[..], "`Recv'd: N/A`"),
        (&b"Hi everyone."[..], "broadcast body"),
    ] {
        assert!(
            contains(&after, needle),
            "expected {description} on broadcast read-back, got {:?}",
            String::from_utf8_lossy(&after)
        );
    }
    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;
}

async fn reject_eall_post(stream: &mut TcpStream) {
    write_line(stream, b"E EALL").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"Echo").await;
    drain_until(stream, b"Private (y/N)? ").await;
    write_line(stream, b"N").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"Cross-conference notice.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;
    let after = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"This message base does not accept that addressee."),
        "expected addressing-not-allowed notice after E EALL, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        !contains(&after, b"Message #2 saved."),
        "EALL must not have been persisted, got {:?}",
        String::from_utf8_lossy(&after)
    );
}

async fn post_comment_to_sysop(stream: &mut TcpStream) {
    write_line(stream, b"C").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"Welcome screen typo").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"There's a typo on the welcome screen.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;
    let after = drain_until(stream, b"mins. left): ").await;
    // The rejected EALL did not advance highest_message, so the
    // comment-to-sysop is the second persisted mail.
    assert!(
        contains(&after, b"Message #2 saved."),
        "expected `Message #2 saved.` after C, got {:?}",
        String::from_utf8_lossy(&after)
    );
}

async fn read_back_comment_to_sysop(stream: &mut TcpStream) {
    write_line(stream, b"R 2").await;
    let after = drain_until(stream, b">: ").await;
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
        assert!(
            contains(&after, needle),
            "expected {description} on comment-to-sysop read-back, got {:?}",
            String::from_utf8_lossy(&after)
        );
    }
    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;
}

fn phase7_conferences() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid Conf01")]
}

fn phase7_individual_or_all_conferences() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::with_options(
            1,
            1,
            "main".to_string(),
            AllowedAddressing::IndividualOrAll,
            AllScanScope::default(),
        )],
    )
    .expect("valid Conf01")]
}
