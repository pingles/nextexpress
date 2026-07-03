//! Tier D `N` (`NextScan` new-files scan, slice D9) in-process
//! integration tests.
//!
//! Each scenario boots a [`TelnetListener`] in-process with a
//! [`ManualClock`] frozen at the capture day (2026-07-03), signs in as
//! the seeded sysop, drives the `N` command over a real telnet client,
//! and asserts the captured wire bytes (parity target: the `AquaScan`
//! v1.0 door with `NextScan` branding —
//! `comparison/transcripts/ae_tierd_newfiles.txt`, two passes,
//! sections N1–N9). The expected literals are restated here
//! independently of the production constants on purpose: the smoke
//! guards them against drift.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use nextexpress::adapters::in_memory_file_repository::InMemoryFileRepository;
use nextexpress::adapters::system_clock::ManualClock;
use nextexpress::app::seed;
use nextexpress::app::services::SharedFileRepo;
use nextexpress::domain::conference::{Conference, MessageBase};

use time::macros::datetime;

use support::{
    contains, drain_until, end_session_forced, read_idle, sign_in_seeded_sysop, write_key,
    write_line, TestRuntime,
};

/// The `N` listing banner: the landed `NextScan` banner with the
/// `'n ?'` label — same 18 visible label columns as `'f ?'`, so the
/// 40-dash run and 77-col frame hold (`ae_tierd_newfiles.txt` N1a,
/// branding per `designs/NEXTSCAN.md` §7).
const NEW_FILES_BANNER: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------------[ \x1b[36m'n ?' for options \x1b[34m]--\x1b[0m\r\n";

/// The door's `Date:` prompt up to the spliced default (N1a).
const DATE_PROMPT_HEAD: &[u8] =
    b"\x1b[36mDate: \x1b[32m(\x1b[33mMM-DD-YY\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33m-X\x1b[32m) \x1b[36mDays, \x1b[32m(\x1b[33mR\x1b[32m)\x1b[36meverse, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=";

/// The `Date:` prompt's tail after the default (trailing space, N1a).
const DATE_PROMPT_TAIL: &[u8] = b" ?\x1b[0m ";

/// The door's Directories prompt for the two-area demo conference —
/// byte-identical to bare `F`'s (`ae_tierd_newfiles.txt` N1a).
const DIRECTORIES_PROMPT: &[u8] =
    b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m ";

/// The `More?` pager prompt (shared with `F` — zero `(S)kip Conf`
/// anywhere in the N captures).
const MORE_PROMPT: &[u8] =
    b"\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m ";

/// The two-reset tail of every listing-shaped exit.
const EXIT_TAIL: &[u8] = b"\x1b[0m\r\n\x1b[0m\r\n";

/// The capture day both reference passes ran on.
fn capture_noon() -> SystemTime {
    SystemTime::from(datetime!(2026-07-03 12:00 UTC))
}

/// Boots an in-process listener over the seeded demo corpus with
/// `clock` installed (the two-conference wiring the `F` smoke uses).
async fn spawn_listener_with_demo_files_at(clock: Arc<ManualClock>) -> SocketAddr {
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
    let (areas, files) = seed::demo_file_catalogue(&conferences);
    let file_repo: SharedFileRepo = Arc::new(InMemoryFileRepository::new(areas, files));
    support::spawn_seeded_sysop(
        TestRuntime::new(
            std::env::temp_dir(),
            conferences,
            support::empty_mail_stores(),
            file_repo,
        )
        .with_clock(clock),
    )
    .await
}

#[tokio::test]
async fn bare_n_defaults_the_date_to_the_previous_call_day() {
    // The capture's two-round proof (pass 1 advertised `06-25-26`
    // while today was 07-03): the Enter default is the day of the
    // PREVIOUS call. Session 1 logs off on 06-25 to stamp
    // `last_call`; the clock advances to the capture day; session 2's
    // date prompt advertises 06-25-26 and Enter scans from it.
    let clock = Arc::new(ManualClock::set_to(SystemTime::from(datetime!(
        2026-06-25 15:00 UTC
    ))));
    let addr = spawn_listener_with_demo_files_at(clock.clone()).await;

    // --- Session 1: sign in on 06-25, log off (stamps last_call) ---
    let mut s1 = sign_in_seeded_sysop(&addr).await;
    end_session_forced(&mut s1).await;
    drop(s1);

    // --- Session 2: the capture day ---
    clock.set(capture_noon());
    let mut s2 = sign_in_seeded_sysop(&addr).await;
    write_line(&mut s2, b"N").await;
    let opened = drain_until(&mut s2, DATE_PROMPT_TAIL).await;
    assert!(
        contains(&opened, NEW_FILES_BANNER),
        "N opens under the NextScan 'n ?' banner, got {:?}",
        String::from_utf8_lossy(&opened),
    );
    let mut dated_prompt = DATE_PROMPT_HEAD.to_vec();
    dated_prompt.extend_from_slice(b"06-25-26");
    dated_prompt.extend_from_slice(DATE_PROMPT_TAIL);
    assert!(
        contains(&opened, &dated_prompt),
        "the Enter default is the previous call's day (06-25-26), got {:?}",
        String::from_utf8_lossy(&opened),
    );
    assert!(
        !contains(&opened, b"=07-03-26"),
        "the default must not be today",
    );

    // Enter keeps the default; dir 2's newest row (06-10) is older.
    write_line(&mut s2, b"").await;
    drain_until(&mut s2, DIRECTORIES_PROMPT).await;
    write_line(&mut s2, b"2").await;
    let scanned = drain_until(&mut s2, b"mins. left): ").await;
    let mut expected = b"\r\nScanning dir 2 for 06-25-26... Nothing found!\r\n".to_vec();
    expected.extend_from_slice(EXIT_TAIL);
    assert!(
        contains(&scanned, &expected),
        "Enter scans from the previous-call day, got {:?}",
        String::from_utf8_lossy(&scanned),
    );
    end_session_forced(&mut s2).await;
}

#[tokio::test]
async fn bare_n_date_and_dir_answers_run_the_dated_scan() {
    // N1a/N2 shape over telnet: bare N -> the Date prompt (first call
    // ever, so the default is today) -> 01-01-26 -> dir 2: the dated
    // header, the renumbered filtered trio, End of File List, the
    // post-End More?, and Q's echoed Quit into the two-reset tail.
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N").await;
    let opened = drain_until(&mut stream, DATE_PROMPT_TAIL).await;
    let mut expected_head = b"N\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(NEW_FILES_BANNER);
    expected_head.extend_from_slice(b"\r\n");
    assert!(
        contains(&opened, &expected_head),
        "bare N opens with the NextScan preamble, got {:?}",
        String::from_utf8_lossy(&opened),
    );
    let mut todays_prompt = DATE_PROMPT_HEAD.to_vec();
    todays_prompt.extend_from_slice(b"07-03-26");
    todays_prompt.extend_from_slice(DATE_PROMPT_TAIL);
    assert!(
        contains(&opened, &todays_prompt),
        "a first-time caller defaults to today (TO-CONFIRM #12), got {:?}",
        String::from_utf8_lossy(&opened),
    );

    write_line(&mut stream, b"01-01-26").await;
    drain_until(&mut stream, DIRECTORIES_PROMPT).await;
    write_line(&mut stream, b"2").await;
    let page = drain_until(&mut stream, MORE_PROMPT).await;
    assert!(
        contains(&page, b"\r\nScanning dir 2 for 01-01-26... Ok!\r\n\r\n"),
        "the dated scan header follows the post-answer blank, got {:?}",
        String::from_utf8_lossy(&page),
    );
    // The filtered set renumbers from #1 (FRESHUPL is dir 2's oldest).
    assert!(
        contains(&page, b"\x1b[34m[\x1b[0m File #1 \x1b[34m]"),
        "the filtered set renumbers from File #1, got {:?}",
        String::from_utf8_lossy(&page),
    );
    assert!(
        contains(&page, b"FRESHUPL.LHA") && contains(&page, b"TOOLPACK.LHA"),
        "the dir-2 trio lists, got {:?}",
        String::from_utf8_lossy(&page),
    );
    assert!(
        contains(
            &page,
            b"\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m\r\n"
        ),
        "the End of File List trailer precedes the post-End More?, got {:?}",
        String::from_utf8_lossy(&page),
    );

    // D2b: Q is a bare keypress at the post-End More?.
    write_key(&mut stream, b"Q").await;
    let quit = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        quit.starts_with(b"Quit\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "Q echoes Quit into the two-reset tail, got {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn inline_n_scan_pages_at_29_lines_and_hotkey_q_quits() {
    // N7c over telnet: `N 01-01-26 1` counts its preamble (the inline
    // page-1 model), pauses at the first More?, and a lone `Q`
    // keypress — no Enter — echoes Quit and exits (write_key +
    // read_idle prove the hotkey acts per keystroke).
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N 01-01-26 1").await;
    let page = drain_until(&mut stream, MORE_PROMPT).await;
    let mut expected_head = b"N 01-01-26 1\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(NEW_FILES_BANNER);
    expected_head.extend_from_slice(b"\r\nScanning dir 1 for 01-01-26... Ok!\r\n\r\n");
    assert!(
        page.starts_with(&expected_head),
        "the inline form opens with the counted preamble, got {:?}",
        String::from_utf8_lossy(&page[..expected_head.len().min(page.len())]),
    );
    // The N7c boundary: page 1 pauses INSIDE the 02-03-26 block —
    // after its closing blank, before the File #4 frame (one line
    // earlier than F 1's boundary: the header+blank pair here is one
    // line shorter than F's banner+blank+header+blank run).
    let mut expected_tail = b" 02-03-26\r\n\x1b[0m\r\n".to_vec();
    expected_tail.extend_from_slice(MORE_PROMPT);
    assert!(
        page.ends_with(&expected_tail),
        "page 1 pauses after 29 counted lines (mid-block), got tail {:?}",
        String::from_utf8_lossy(&page[page.len().saturating_sub(160)..]),
    );

    write_key(&mut stream, b"Q").await;
    let quit = read_idle(&mut stream, support::DRAIN_DEADLINE).await;
    assert!(
        quit.starts_with(b"Quit\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "a lone Q keypress quits without a terminator, got {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );
    assert!(
        contains(&quit, b"mins. left): "),
        "the menu prompt follows the tail",
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn n_junk_date_takes_the_single_shot_error() {
    // N5: a junk Date answer — blank, `Error in date!`, blank, ONE
    // reset, straight back to the menu. The Directories prompt never
    // shows (single-shot; the internal's looping prompt is the
    // shadowed stock path).
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N").await;
    drain_until(&mut stream, DATE_PROMPT_TAIL).await;
    write_line(&mut stream, b"FOO").await;
    let error = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&error, b"\r\nError in date!\r\n\r\n\x1b[0m\r\n"),
        "the single-shot junk-date envelope, got {:?}",
        String::from_utf8_lossy(&error),
    );
    assert!(
        !contains(&error, b"Directories: "),
        "the error exits to the menu without the Directories prompt",
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn n_help_shows_the_rebranded_screen() {
    // N6 with the branding swaps: the Copyright NextScan help banner,
    // the verbatim syntax rows (`N W` advertised but unported —
    // Argument error, the F W precedent), `Configure NextScan`, and no
    // AquaScan byte anywhere.
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N ?").await;
    let help = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(
            &help,
            "\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m\r\n".as_bytes(),
        ),
        "the Copyright help banner is the landed NextScan one, got {:?}",
        String::from_utf8_lossy(&help),
    );
    assert!(
        contains(
            &help,
            b"\x1b[0m  N [S] [dir] [Q] [NS]        \x1b[36m- Scan since day of last call\r\n",
        ),
        "the syntax rows are verbatim from N6, got {:?}",
        String::from_utf8_lossy(&help),
    );
    assert!(
        contains(&help, b"- Configure NextScan\r\n"),
        "the Configure row is rebranded",
    );
    assert!(
        contains(&help, b"`-- H -- Hold dir\r\n"),
        "the ASCII diagram survives verbatim",
    );
    assert!(
        !contains(&help, b"AquaScan"),
        "no AquaScan branding leaks: {:?}",
        String::from_utf8_lossy(&help),
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn n_invalid_argument_reports_the_n_help_pointer() {
    // N7e: `N R -1` — the Copyright help banner + the `'n ?'`
    // argument-error envelope with a single-reset tail.
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"N R -1").await;
    let error = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(
            &error,
            b"\r\n\r\nArgument error! Type 'n ?' for help.\r\n\r\n\x1b[0m\r\n",
        ),
        "the argument-error envelope carries the 'n ?' pointer, got {:?}",
        String::from_utf8_lossy(&error),
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn utf8_gate_every_n_session_byte_decodes() {
    // Encoding policy (AGENTS.md): the wire is valid UTF-8. Drive
    // every N surface with high-bit bytes — the help screen (©), the
    // listing body (wave-art separators), both error envelopes and the
    // prompt path — and assert the entire received stream decodes.
    let addr =
        spawn_listener_with_demo_files_at(Arc::new(ManualClock::set_to(capture_noon()))).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;
    let mut all = Vec::new();
    write_line(&mut stream, b"N ?").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"N 01-01-26 A NS").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"N R -1").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"N").await;
    all.extend(drain_until(&mut stream, DATE_PROMPT_TAIL).await);
    write_line(&mut stream, b"FOO").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    assert!(
        std::str::from_utf8(&all).is_ok(),
        "session stream contains non-UTF-8 bytes: {:?}",
        String::from_utf8_lossy(&all),
    );
    end_session_forced(&mut stream).await;
}
