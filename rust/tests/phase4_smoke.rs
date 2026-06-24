//! Phase 4 in-process smoke test (Slice 34a).
//!
//! Boots a `TelnetListener` in-process against a temp BBS path
//! pre-populated with three `Conf<NN>/conference.toml` files, then
//! drives the full Phase 4 flow over real telnet.

mod support;

use std::path::Path;

use nextexpress::domain::conference::{Conference, MessageBase, NameType};

use support::{
    contains, drain_until, empty_file_repo, empty_mail_stores, end_session, spawn_seeded_sysop,
    write_line, TestRuntime,
};

#[tokio::test]
async fn listener_walks_phase4_conference_flow_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    seed_two_conferences(dir.path());

    let conferences = phase4_conferences();
    let addr = spawn_seeded_sysop(TestRuntime::new(
        dir.path().to_path_buf(),
        conferences,
        empty_mail_stores(),
        empty_file_repo(),
    ))
    .await;

    let (mut stream, post_auth) = support::sign_in_seeded_sysop_capturing_menu(&addr).await;
    assert!(
        contains(&post_auth, b"CONF1-MENU"),
        "expected Conf01 per-conference menu after auto-rejoin, got {:?}",
        String::from_utf8_lossy(&post_auth)
    );
    assert!(
        contains(&post_auth, b"Conference 1: Main Auto-ReJoined"),
        "expected legacy auto-rejoin announcement, got {:?}",
        String::from_utf8_lossy(&post_auth)
    );

    write_line(&mut stream, b"J 2").await;
    let post_j = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&post_j, b"CONF2-MENU"),
        "expected Conf02 per-conference menu after explicit join, got {:?}",
        String::from_utf8_lossy(&post_j)
    );
    assert!(
        contains(&post_j, b"Joining Conference") && contains(&post_j, b"Programming"),
        "expected legacy `Joining Conference: Programming` line, got {:?}",
        String::from_utf8_lossy(&post_j)
    );

    write_line(&mut stream, b"J 3").await;
    let post_j3 = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&post_j3, b"CONF3-MENU"),
        "expected Conf03 per-conference menu after `J 3`, got {:?}",
        String::from_utf8_lossy(&post_j3)
    );
    assert!(
        contains(&post_j3, b"real names"),
        "expected real-names notice after promotion to Conf03, got {:?}",
        String::from_utf8_lossy(&post_j3)
    );

    write_line(&mut stream, b"J 99").await;
    drain_until(&mut stream, b"Conference Number (1-3): ").await;
    write_line(&mut stream, b"").await;
    let post_blank = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&post_blank, b"Joining Conference")
            && !contains(&post_blank, b"do not have access"),
        "blank input at the join prompt must abort silently, got {:?}",
        String::from_utf8_lossy(&post_blank)
    );
    assert!(
        contains(&post_blank, b"CONF3-MENU"),
        "expected to stay in Conf03 after the blank join abort, got {:?}",
        String::from_utf8_lossy(&post_blank)
    );

    end_session(&mut stream).await;
}

fn phase4_conferences() -> Vec<Conference> {
    vec![
        Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid Conf01"),
        Conference::new(
            2,
            "Programming".to_string(),
            vec![MessageBase::new(2, 1, "main".to_string())],
        )
        .expect("valid Conf02"),
        Conference::with_name_type(
            3,
            "Authors".to_string(),
            vec![MessageBase::new(3, 1, "main".to_string())],
            NameType::RealName,
        )
        .expect("valid Conf03"),
    ]
}

/// Writes Conf01 + Conf02 + Conf03 with distinguishable
/// per-conference menus so the smoke can prove which conference the
/// session is attached to without parsing only the JOINED screen.
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
