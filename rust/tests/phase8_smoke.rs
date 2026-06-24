//! Phase 8 in-process smoke tests for reply / forward / kill / move /
//! edit header via the `R` read sub-prompt.
//!
//! Boots a `TelnetListener` in-process against temp BBS paths and drives
//! the mail-management operations over real telnet. Tier B B8 retired
//! the standalone `RP` / `FW` / `K` / `MV` / `EH` menu commands; these
//! operations now live inside the `readMSG` sub-prompt, so each flow
//! reads a message (`R <n>`) and then types the sub-prompt option.

mod support;

use std::path::Path;

use nextexpress::domain::conference::{Conference, MessageBase};
use tokio::net::TcpStream;

use support::{
    contains, drain_until, empty_file_repo, file_mail_stores, spawn_seeded_sysop, write_line,
    TestRuntime,
};

#[tokio::test]
async fn listener_walks_phase8_reply_forward_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    seed_conf01_with_one_message(dir.path());
    let conferences = phase8_one_msgbase_conferences();
    let mail_stores = file_mail_stores(dir.path(), &conferences);
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        mail_stores,
        empty_file_repo(),
    ))
    .await;

    let (mut stream, _) = support::sign_in_seeded_sysop_declining_logon_scan(&addr).await;
    walk_phase8_reply_forward_flow(&mut stream).await;
    support::end_session(&mut stream).await;
}

#[allow(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "cohesive end-to-end script"
)]
async fn walk_phase8_reply_forward_flow(stream: &mut TcpStream) {
    // Step 1: post an original we control via `E sysop`. The seed
    // mail (#1) is also from the sysop but its subject does not carry
    // the marker we want to scan for after a reply.
    write_line(stream, b"E sysop").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"Phase 8 source").await;
    drain_until(stream, b"Private (y/N)? ").await;
    write_line(stream, b"N").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"Phase 8 original body.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;
    let post_e = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_e, b"Message #2 saved."),
        "expected `Message #2 saved.` after E, got {:?}",
        String::from_utf8_lossy(&post_e)
    );

    // Step 2: reply via the sub-prompt (Tier B B8 retired the top-level
    // `RP`). `R 2` -> `R` -> body -> `.`. The reply posts msg 3 and the
    // sub-prompt advances to it, so the reply is displayed inline with
    // the `Re: ` subject and the typed body.
    write_line(stream, b"R 2").await;
    drain_until(stream, b">: ").await;
    write_line(stream, b"R").await;
    drain_until(stream, b"End with a single '.'").await;
    write_line(stream, b"Replying inline.").await;
    write_line(stream, b".").await;
    let post_reply = drain_until(stream, b">: ").await;
    for (needle, what) in [
        (&b"Message #3 saved."[..], "reply-saved notice"),
        (&b"Re: Phase 8 source"[..], "`Re: ` subject"),
        (&b"Replying inline."[..], "reply body"),
    ] {
        assert!(
            contains(&post_reply, needle),
            "expected {what} after the sub-prompt reply, got {:?}",
            String::from_utf8_lossy(&post_reply)
        );
    }
    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;

    // Step 3: forward the original (msg 2) via the sub-prompt. `R 2` ->
    // `F` -> To: sysop -> note -> `.`. Forward stays on msg 2.
    write_line(stream, b"R 2").await;
    drain_until(stream, b">: ").await;
    write_line(stream, b"F").await;
    drain_until(stream, b"Forward to: ").await;
    write_line(stream, b"sysop").await;
    drain_until(stream, b"blank line skips").await;
    write_line(stream, b"Please look at this.").await;
    write_line(stream, b".").await;
    let post_fwd = drain_until(stream, b">: ").await;
    assert!(
        contains(&post_fwd, b"Message #4 saved."),
        "expected `Message #4 saved.` after the sub-prompt forward, got {:?}",
        String::from_utf8_lossy(&post_fwd)
    );
    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;

    // Step 4: read back the forward (msg 4): the `Fwd: ` subject, the
    // spec's `forward_header_for` block, the original body, and the
    // `--`-separated note.
    write_line(stream, b"R 4").await;
    let post_r4 = drain_until(stream, b">: ").await;
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
        assert!(
            contains(&post_r4, needle),
            "expected {what} on the forwarded message, got {:?}",
            String::from_utf8_lossy(&post_r4)
        );
    }
    write_line(stream, b"Q").await;
    drain_until(stream, b"mins. left): ").await;
}

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

#[tokio::test]
async fn listener_walks_phase8_sysop_admin_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    seed_conf01_two_msgbases(dir.path());
    let conferences = phase8_two_msgbase_conferences();
    let mail_stores = file_mail_stores(dir.path(), &conferences);
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        mail_stores,
        empty_file_repo(),
    ))
    .await;

    let (mut stream, _) = support::sign_in_seeded_sysop_declining_logon_scan(&addr).await;
    walk_phase8_sysop_admin_flow(&mut stream).await;
    support::end_session(&mut stream).await;
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

    // Second msgbase exists but is empty. It is the move target.
    let mb2 = conf01.join("MsgBase2");
    std::fs::create_dir_all(&mb2).expect("create MsgBase2");
}

#[allow(
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "cohesive end-to-end script"
)]
async fn walk_phase8_sysop_admin_flow(stream: &mut TcpStream) {
    // Step 1: `EH` via the sub-prompt rewrites the seed mail's subject,
    // and the option re-displays the edited message in place (Tier B B8
    // retired the top-level `EH`). `R 1` -> `EH`.
    write_line(stream, b"R 1").await;
    drain_until(stream, b">: ").await;
    write_line(stream, b"EH").await;
    drain_until(stream, b"New subject (blank = unchanged): ").await;
    write_line(stream, b"Sysop-edited subject").await;
    drain_until(stream, b"New To (blank = unchanged): ").await;
    write_line(stream, b"").await;
    let post_eh = drain_until(stream, b">: ").await;
    assert!(
        contains(&post_eh, b"Header updated.") && contains(&post_eh, b"Sysop-edited subject"),
        "expected `Header updated.` and the edited subject re-displayed, got {:?}",
        String::from_utf8_lossy(&post_eh)
    );

    // Step 2: `M`ove the edited mail (still the current message) from
    // (1,1) to (1,2). The base holds only this one message, so the
    // post-move advance hits the out-of-range clamp and returns to the
    // menu.
    write_line(stream, b"M").await;
    drain_until(stream, b"Target conference number: ").await;
    write_line(stream, b"1").await;
    drain_until(stream, b"Target msgbase number: ").await;
    write_line(stream, b"2").await;
    let post_mv = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_mv, b"Message moved. New number 1."),
        "expected `Message moved. New number 1.` after the sub-prompt move, got {:?}",
        String::from_utf8_lossy(&post_mv)
    );

    // Step 3: now the source's #1 is soft-deleted by the move. Post
    // a fresh mail with `E sysop` so we have a live row to delete.
    write_line(stream, b"E sysop").await;
    drain_until(stream, b"Subject: ").await;
    write_line(stream, b"To be killed").await;
    drain_until(stream, b"Private (y/N)? ").await;
    write_line(stream, b"N").await;
    drain_until(stream, b"Enter your text. (Enter) alone to end.").await;
    write_line(stream, b"Doomed.").await;
    write_line(stream, b"").await;
    drain_until(stream, b"Msg. Options:").await;
    write_line(stream, b"S").await;
    let post_e = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_e, b"Message #2 saved."),
        "expected `Message #2 saved.` after E, got {:?}",
        String::from_utf8_lossy(&post_e)
    );

    // Step 4: `D`elete the fresh mail via the sub-prompt. `R 2` -> `D`
    // -> confirm `y`. With only this message in the base, the
    // post-delete advance clamps back to the menu.
    write_line(stream, b"R 2").await;
    drain_until(stream, b">: ").await;
    write_line(stream, b"D").await;
    drain_until(stream, b"Delete message (y/N)? ").await;
    write_line(stream, b"y").await;
    let post_k = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_k, b"Message deleted."),
        "expected `Message deleted.` after the sub-prompt delete, got {:?}",
        String::from_utf8_lossy(&post_k)
    );

    // Step 5: R 2 now reports the source-deleted line via the read
    // path (mail visibility = Deleted -> read denied).
    write_line(stream, b"R 2").await;
    let post_r = drain_until(stream, b"mins. left): ").await;
    assert!(
        contains(&post_r, b"deleted"),
        "expected R 2 after delete to surface a deletion notice, got {:?}",
        String::from_utf8_lossy(&post_r)
    );
}

fn phase8_one_msgbase_conferences() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid Conf01")]
}

fn phase8_two_msgbase_conferences() -> Vec<Conference> {
    vec![Conference::new(
        1,
        "Main".to_string(),
        vec![
            MessageBase::new(1, 1, "main".to_string()),
            MessageBase::new(1, 2, "archive".to_string()),
        ],
    )
    .expect("valid Conf01")]
}
