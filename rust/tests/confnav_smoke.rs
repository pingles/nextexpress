//! Tier C conference-navigation in-process integration test (slice
//! C-wire).
//!
//! Boots a [`TelnetListener`] in-process (the `quickwins_smoke.rs`
//! harness shape — AGENTS.md rule 6), signs in over a real telnet
//! client, and drives the whole Tier C surface through one session:
//! the interactive `Conference Number (1-N): ` prompt (slice C2),
//! `<` / `>` neighbour hops (C3), `JM` and the dotted / two-token `J`
//! forms (C4a), and `<<` / `>>` plus the `Message Base Number (1-N): `
//! prompt (C4b). Every assertion pins wire bytes observed live
//! against the genuine `AmiExpress` 5.6.0 reference
//! (`comparison/evidence-tierC/live-observations.md`,
//! `comparison/transcripts/ae_tierc{,2,3,4}.txt`) or, for the
//! multi-base flows the reference install cannot exercise, the raw
//! legacy source (`amiexpress/express.e`, cited per step).
//!
//! Fixture: conference 1 `Main` (single base), 2 `Hidden` (granted to
//! nobody — the `<` / `>` walks must skip it), 3 `Files` (two bases,
//! `general` + `uploads`), 4 `Last` (single base).

mod support;

use nextexpress::domain::conference::{Conference, ConferenceMembership, MessageBase};

use support::{
    contains, drain_until, empty_file_repo, empty_mail_stores, end_session, sign_in_seeded_sysop,
    spawn_seeded_sysop, write_line, TestRuntime,
};

/// The single-base `JM` / `<<` / `>>` failure notice, byte-for-byte
/// (`amiexpress/express.e:25213`; observed live for every non-dotted
/// form, `comparison/transcripts/ae_tierc.txt`).
const SINGLE_BASE_NOTICE: &[u8] =
    b"\r\nThis conference does not contain multiple message bases\r\n\r\n";

// One session drives the whole Tier C battery so the scenario order
// matches the live reference captures; splitting it would lose the
// cross-command state (current conference/base) each step depends on.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn tier_c_conference_navigation_over_telnet() {
    let addr = spawn_listener().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Expert mode on, so no menu screen redraws between commands —
    // the AmiExpress reference user the live captures were taken with
    // also had expert mode set, which keeps the abort/prompt framing
    // comparable byte-for-byte.
    write_line(&mut stream, b"X").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(&capture, b"Expert mode enabled");

    // --- C2: bare `J` opens the prompt with nothing between the echo
    // and the prompt (no JoinConf asset installed; live:
    // `b'J\r\nConference Number (1-2): '`). N is the highest
    // conference number, here 4.
    write_line(&mut stream, b"J").await;
    let capture = drain_until(&mut stream, b"Conference Number (1-4): ").await;
    assert_ends_with(&capture, b"J\r\nConference Number (1-4): ");

    // Blank input aborts with exactly one CRLF after the echoed
    // Enter, straight into the menu prompt (live framing
    // `b'\r\n\r\n\x1b[0m...'`); the conference is unchanged.
    write_line(&mut stream, b"").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_starts_with(&capture, b"\r\n\r\n\x1b[0m");
    assert_contains(&capture, b"\x1b[36m1\x1b[34m:\x1b[36mMain");

    // --- C2: prompt input is clamped (live: `99` joined the top
    // conference, `0` / `abc` joined conference 1).
    write_line(&mut stream, b"J").await;
    drain_until(&mut stream, b"Conference Number (1-4): ").await;
    write_line(&mut stream, b"99").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Last\r\n",
    );

    write_line(&mut stream, b"J abc").await;
    drain_until(&mut stream, b"Conference Number (1-4): ").await;
    write_line(&mut stream, b"0").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Main\r\n",
    );

    // --- C2: an in-range but ungranted conference gets the legacy
    // no-access notice and stays put (`amiexpress/express.e:25156-25158`);
    // direct arguments are never clamped. `J 2abc` pins the `Val`
    // digit-prefix parse (live: `J 2abc` targeted conference 2).
    for line in [b"J 2".as_slice(), b"J 2abc".as_slice()] {
        write_line(&mut stream, line).await;
        let capture = drain_until(&mut stream, b"mins. left): ").await;
        assert_contains(
            &capture,
            b"\r\nYou do not have access to the requested conference\r\n\r\n",
        );
        assert_contains(&capture, b"\x1b[36m1\x1b[34m:\x1b[36mMain");
    }

    // --- C3: `>` from 1 skips the ungranted 2 and joins 3, whose two
    // bases put the bracketed base name in the announcement
    // (`amiexpress/express.e:5077-5079`).
    write_line(&mut stream, b">").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [general]\r\n",
    );

    // --- C4b: `>>` steps to base 2 (full join, bracketed name).
    write_line(&mut stream, b">>").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [uploads]\r\n",
    );

    // `>>` past the top falls into the JM no-arg flow: the
    // `Message Base Number (1-N): ` prompt (`amiexpress/express.e:
    // 24587-24588`); blank aborts and stays.
    write_line(&mut stream, b">>").await;
    let capture = drain_until(&mut stream, b"Message Base Number (1-2): ").await;
    assert_ends_with(&capture, b">>\r\nMessage Base Number (1-2): ");
    write_line(&mut stream, b"").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_starts_with(&capture, b"\r\n\r\n\x1b[0m");

    // `<<` steps back down to base 1.
    write_line(&mut stream, b"<<").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [general]\r\n",
    );

    // `<<` past the bottom: the same prompt (`amiexpress/express.e:
    // 24573-24574`); this time answer `9`, which JM's flow CLAMPS to
    // the top base (`:25233-25234`).
    write_line(&mut stream, b"<<").await;
    drain_until(&mut stream, b"Message Base Number (1-2): ").await;
    write_line(&mut stream, b"9").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [uploads]\r\n",
    );

    // --- C4a: `JM 1` joins an explicit in-range base of the current
    // conference; `JM 1.1` is the dotted delegation to `J` (live:
    // `JM 1.1` joined conference 1).
    write_line(&mut stream, b"JM 1").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [general]\r\n",
    );
    write_line(&mut stream, b"JM 1.1").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Main\r\n",
    );

    // --- C4a/C4b: on the single-base `Main`, every non-dotted `JM`
    // form and both siblings print the exact legacy notice (observed
    // live), and nothing joins or prompts.
    for line in [
        b"JM".as_slice(),
        b"JM 1".as_slice(),
        b"JM 9".as_slice(),
        b"<<".as_slice(),
        b">>".as_slice(),
    ] {
        write_line(&mut stream, line).await;
        let capture = drain_until(&mut stream, b"mins. left): ").await;
        assert_contains(&capture, SINGLE_BASE_NOTICE);
        assert_contains(&capture, b"\x1b[36m1\x1b[34m:\x1b[36mMain");
    }

    // --- C4b: `J 1 2` opens the `(1-1)` prompt even on a single-base
    // conference (live: `b'J 1 2\r\nMessage Base Number (1-1): '`).
    // The answer is passed UNCLAMPED to the join, which resets a base
    // the conference does not hold to the primary
    // (`amiexpress/express.e:25179` + `:4995`): `5` joins base 1 with
    // the plain (no-bracket) announcement.
    write_line(&mut stream, b"J 1 2").await;
    let capture = drain_until(&mut stream, b"Message Base Number (1-1): ").await;
    assert_ends_with(&capture, b"J 1 2\r\nMessage Base Number (1-1): ");
    write_line(&mut stream, b"5").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Main\r\n",
    );

    // --- C3: `>` at the top of the catalogue falls into the C2
    // prompt and a blank abort stays put (live: `>` at the last
    // conference re-used the J prompt).
    write_line(&mut stream, b"J 4").await;
    drain_until(&mut stream, b"mins. left): ").await;
    write_line(&mut stream, b">").await;
    let capture = drain_until(&mut stream, b"Conference Number (1-4): ").await;
    assert_ends_with(&capture, b">\r\nConference Number (1-4): ");
    write_line(&mut stream, b"").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(&capture, b"\x1b[36m4\x1b[34m:\x1b[36mLast");

    // `<` from 4 skips the ungranted 2 only when relevant — the
    // nearest granted below 4 is 3.
    write_line(&mut stream, b"<").await;
    let capture = drain_until(&mut stream, b"mins. left): ").await;
    assert_contains(
        &capture,
        b"\x1b[32mJoining Conference\x1b[33m:\x1b[0m Files [general]\r\n",
    );

    end_session(&mut stream).await;
}

/// Boots the Tier C fixture: four conferences (see module docs), the
/// seeded sysop granted everything except conference 2, an empty mail
/// store, a temp-dir BBS root (no screen assets — the prompts must
/// arrive bare, as on the reference).
async fn spawn_listener() -> std::net::SocketAddr {
    let conferences = vec![
        Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            2,
            "Hidden".to_string(),
            vec![MessageBase::new(2, 1, "main".to_string())],
        )
        .expect("valid conference"),
        Conference::new(
            3,
            "Files".to_string(),
            vec![
                MessageBase::new(3, 1, "general".to_string()),
                MessageBase::new(3, 2, "uploads".to_string()),
            ],
        )
        .expect("valid conference"),
        Conference::new(
            4,
            "Last".to_string(),
            vec![MessageBase::new(4, 1, "main".to_string())],
        )
        .expect("valid conference"),
    ];

    // `spawn_seeded_sysop` grants the sysop every conference; the
    // Tier C walks need conference 2 ungranted, so revoke that row
    // (a revoked membership never grants access — `has_membership`
    // requires `granted`).
    let fixture = TestRuntime::new(
        std::env::temp_dir(),
        conferences,
        empty_mail_stores(),
        empty_file_repo(),
    )
    .with_sysop(|sysop| {
        sysop.upsert_membership(ConferenceMembership::new(2, false));
    });
    spawn_seeded_sysop(fixture).await
}

#[track_caller]
fn assert_contains(haystack: &[u8], needle: &[u8]) {
    assert!(
        contains(haystack, needle),
        "expected {:?} in {:?}",
        String::from_utf8_lossy(needle),
        String::from_utf8_lossy(haystack),
    );
}

#[track_caller]
fn assert_starts_with(haystack: &[u8], prefix: &[u8]) {
    assert!(
        haystack.starts_with(prefix),
        "expected {:?} to start with {:?}",
        String::from_utf8_lossy(haystack),
        String::from_utf8_lossy(prefix),
    );
}

#[track_caller]
fn assert_ends_with(haystack: &[u8], suffix: &[u8]) {
    assert!(
        haystack.ends_with(suffix),
        "expected {:?} to end with {:?}",
        String::from_utf8_lossy(haystack),
        String::from_utf8_lossy(suffix),
    );
}
