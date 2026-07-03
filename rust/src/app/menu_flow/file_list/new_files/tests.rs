//! Capture-replay tests for the `N` (new-files scan) handler —
//! slice D9, pinned to `comparison/transcripts/ae_tierd_newfiles.txt`
//! (pass 2 unless noted; section labels N1–N9 cited per test). The
//! fake terminal does not echo, so `terminal.output` is the pure
//! server-generated wire — the parity surface. The clock is always a
//! [`ManualClock`] frozen at the capture day (2026-07-03 12:00 UTC),
//! so every date-derived byte is deterministic.

use std::sync::Arc;
use std::time::SystemTime;

use time::macros::datetime;

use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::adapters::system_clock::ManualClock;
use crate::app::menu_command::{FileSpan, NewFilesArg, NewFilesSpec, ScanRequest};
use crate::app::menu_flow::test_support::{
    joined, key, menu_session, menu_session_with_user, more_clear, services_with,
    services_with_demo_catalogue, test_user, CaptureTerminal, EXIT_TAIL,
};
use crate::app::services::AppServices;
use crate::app::terminal::{KeyEvent, KeyRead, TerminalRead};
use crate::domain::bytes::Bytes;
use crate::domain::files::area::{FileArea, FileAreaRef};
use crate::domain::files::file::{File, FileStatus};
use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};
use crate::domain::session::typed::MenuSession;

use super::super::scan::ScanKind;
use super::super::wire;
use super::{parse_date_answer, resolve_request};

/// The capture day: both reference passes ran on 2026-07-03.
fn capture_now() -> SystemTime {
    SystemTime::from(datetime!(2026-07-03 12:00 UTC))
}

/// The demo catalogue under a clock frozen at the capture day.
fn demo_services_at_capture_noon() -> AppServices {
    let mut services = services_with_demo_catalogue();
    services.clock = Arc::new(ManualClock::set_to(capture_now()));
    services
}

/// Drives the `N` handler against a scripted terminal with a
/// caller-owned session (so tests can inspect the flag set after).
async fn run_new_files(
    services: &AppServices,
    terminal: &mut CaptureTerminal,
    session: &mut MenuSession,
    arg: NewFilesArg,
) {
    let mut flow = crate::app::menu_flow::MenuFlow { terminal, services };
    flow.handle_new_files(session, arg)
        .await
        .expect("new files scan");
}

/// `\x1b[0m\r\n` + the N banner + blank — the entry preamble of every
/// N form (N1a; raw on the prompt path, counted on the inline path —
/// identical bytes either way).
fn new_files_preamble() -> Vec<u8> {
    let mut bytes = b"\x1b[0m\r\n".to_vec();
    bytes.extend_from_slice(wire::NEW_FILES_BANNER);
    bytes.extend_from_slice(b"\r\n\r\n");
    bytes
}

/// The assembled listing lines of `files` in conference 1 under the
/// default (empty) flag set.
fn assembled(files: &[File], quick: bool) -> Vec<Vec<u8>> {
    wire::assemble_dir_lines(files, 1, &FlaggedFiles::default(), quick)
        .into_iter()
        .map(|line| line.bytes)
        .collect()
}

/// The `(conference 1, area)` rows uploaded on/after `cutoff`,
/// assembled — the expected body of a `NewSince` scan.
fn new_lines_since(services: &AppServices, area: u32, cutoff: SystemTime) -> Vec<Vec<u8>> {
    let files = services
        .file_repo
        .list_new_since(FileAreaRef::new(1, area), cutoff)
        .expect("files");
    assembled(&files, false)
}

/// The full `(conference 1, area)` listing, newest-first — the
/// expected body of the `R` (reverse) mode.
fn reversed_lines(services: &AppServices, area: u32) -> Vec<Vec<u8>> {
    let mut files = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, area))
        .expect("files");
    files.reverse();
    assembled(&files, false)
}

// --- Prompt path (bare N) ----------------------------------------------

#[tokio::test]
async fn bare_n_enter_at_the_directories_prompt_aborts_with_a_single_reset() {
    // N1a: banner, the Date prompt (Enter = default), the Directories
    // prompt (byte-identical to bare F's), Enter=None — blank, ONE
    // reset, menu. A first-time caller (no prior call) defaults the
    // date to today (TO-CONFIRM #12).
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line(String::new()),
        TerminalRead::Line(String::new()),
    ]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn bare_n_defaults_the_date_to_the_day_of_the_previous_call() {
    // Capture-proven (pass 1 advertised `06-25-26` while today was
    // 07-03): the Enter default is the day of the PREVIOUS call, not
    // today — both the prompt label and the scan cutoff. Kills a
    // `last_call.unwrap_or(now)` → `now` mutant on both sites.
    let services = demo_services_at_capture_noon();
    let mut user = test_user();
    user.record_last_call(SystemTime::from(datetime!(2026-06-25 15:00 UTC)));
    let mut session = menu_session_with_user(user);
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line(String::new()),
        TerminalRead::Line("2".to_string()),
    ]);
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    // Dir 2's newest row is 06-10 — nothing on/after 06-25.
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("06-25-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Scanning dir 2 for 06-25-26... Nothing found!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn n3_replay_days_back_answer_scans_the_dir_renumbered_from_one() {
    // N3 (pass 2): `-30` at the Date prompt → 06-03-26 (month
    // underflow from 07-03), dir 1 → PROTRACK is `[ File #1 ]` (the
    // filtered set renumbers from 1), README1ST falls through as a
    // plain row, End of File List, post-End More?, `Y` → overprint
    // clear, two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![
            TerminalRead::Line("-30".to_string()),
            TerminalRead::Line("1".to_string()),
        ],
        vec![key(b'Y')],
    );
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let cutoff = SystemTime::from(datetime!(2026-06-03 0:00 UTC));
    let body = new_lines_since(&services, 1, cutoff);
    let flat = body.concat();
    let text = String::from_utf8_lossy(&flat);
    assert!(
        text.contains("File #1 ") && text.contains("PROTRACK.LHA"),
        "PROTRACK heads the renumbered filtered set",
    );
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Scanning dir 1 for 06-03-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&body));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn junk_date_answer_takes_the_single_shot_error_and_exits() {
    // N5: `FOO` at the Date prompt — blank, `Error in date!`, blank,
    // ONE reset, straight back to the menu (single-shot; the
    // internal's looping prompt is the shadowed stock path). The
    // Directories prompt is never shown.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("FOO".to_string())]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(b"\r\nError in date!\r\n\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn calendar_invalid_date_answer_takes_the_same_error() {
    // TO-CONFIRM #7 (provisional): `13-40-26` is date-shaped but not a
    // calendar date — same single-shot `Error in date!` envelope.
    let services = demo_services_at_capture_noon();
    let mut terminal =
        CaptureTerminal::with_lines(vec![TerminalRead::Line("13-40-26".to_string())]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(b"\r\nError in date!\r\n\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn out_of_range_dir_answer_reports_the_highest_dir() {
    // N8b: `9` at the Directories prompt — the post-answer blank, F's
    // highest-dir error envelope byte-identical, TWO-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line(String::new()),
        TerminalRead::Line("9".to_string()),
    ]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"The highest directory number is 2!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn junk_directories_answer_errors_in_input() {
    // TO-CONFIRM #3 (provisional): junk at N's Directories prompt
    // takes F's `Error in input!` envelope — same door machinery,
    // byte-identical prompt.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line(String::new()),
        TerminalRead::Line("XYZ".to_string()),
    ]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\nError in input!\r\n\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn r_answer_runs_the_full_reverse_scan() {
    // N4: `R` at the Date prompt runs exactly the FR mode — the full
    // unfiltered dir, newest-first, `Reverse-scanning` header, post-End
    // More?, `Y` → clear + two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![
            TerminalRead::Line("R".to_string()),
            TerminalRead::Line("2".to_string()),
        ],
        vec![key(b'Y')],
    );
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Reverse-scanning dir 2... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&reversed_lines(&services, 2)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn r_with_a_trailing_date_discards_the_date() {
    // N4b: `R 12-30-26` at the Date prompt ran the identical plain
    // full-reverse scan — the date token is tolerated and discarded.
    let services = demo_services_at_capture_noon();
    let run = |answer: &str| {
        let lines = vec![
            TerminalRead::Line(answer.to_string()),
            TerminalRead::Line("2".to_string()),
        ];
        let services = &services;
        async move {
            let mut terminal = CaptureTerminal::with_lines_and_keys(lines, vec![key(b'Y')]);
            let mut session = menu_session();
            run_new_files(services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
            terminal.output
        }
    };
    let plain = run("R").await;
    let dated = run("R 12-30-26").await;
    assert_eq!(
        String::from_utf8_lossy(&dated),
        String::from_utf8_lossy(&plain),
    );
}

#[tokio::test]
async fn n2_page_one_fires_more_after_29_lines_from_the_post_answer_blank() {
    // N2's page-1 pin: the door resets its counter at each interactive
    // prompt, so the prompt path's first More? fires after EXACTLY 29
    // counted lines starting at the post-answer blank (blank + header
    // + blank + 26 body lines; the capture's line 29 is MODRIPPR's
    // name row). `Q` → `Quit` echo + two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![
            TerminalRead::Line("01-01-26".to_string()),
            TerminalRead::Line("A".to_string()),
        ],
        vec![key(b'Q')],
    );
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let cutoff = SystemTime::from(datetime!(2026-01-01 0:00 UTC));
    let body = new_lines_since(&services, 1, cutoff);
    assert!(
        String::from_utf8_lossy(&body[25]).contains("MODRIPPR.LZH"),
        "the 26th body line is MODRIPPR's name row (capture cross-check)",
    );
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Scanning dir 1 for 01-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&body[..26]));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn hold_answer_under_a_date_scan_lists_date_filtered_held_rows() {
    // TO-CONFIRM #1 (interim, PLAUSIBLE): `H` under a date scan lists
    // the held rows the same way the mode filters a dir, under the
    // dir→HOLD header substitution. Held rows before the cutoff and
    // Available rows never appear.
    let file = |name: &str, status, at| {
        File::new(
            name.to_string(),
            Bytes::new(1_000),
            status,
            Some(b'P'),
            format!("{name} description"),
            at,
        )
    };
    let old_held = file(
        "OLDHELD.LHA",
        FileStatus::HeldForReview,
        SystemTime::from(datetime!(2026-05-01 12:00 UTC)),
    );
    let new_held = file(
        "NEWHELD.LHA",
        FileStatus::HeldForReview,
        SystemTime::from(datetime!(2026-06-20 12:00 UTC)),
    );
    let available = file(
        "AVAIL.LHA",
        FileStatus::Available,
        SystemTime::from(datetime!(2026-06-20 12:00 UTC)),
    );
    let mut services = services_with(InMemoryFileRepository::new(
        vec![FileArea::new(1, 1, "Main".to_string())],
        vec![
            (1, 1, old_held),
            (1, 1, new_held.clone()),
            (1, 1, available),
        ],
    ));
    services.clock = Arc::new(ManualClock::set_to(capture_now()));
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![
            TerminalRead::Line("06-01-26".to_string()),
            TerminalRead::Line("H".to_string()),
        ],
        vec![key(b'Y')],
    );
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(1));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Scanning HOLD dir for 06-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&assembled(&[new_held], false)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn hold_answer_with_nothing_held_reports_nothing_found() {
    // TO-CONFIRM #1 (interim): the demo catalogue holds nothing, so
    // `H` under the default date reports the HOLD Nothing-found header
    // and exits with the two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line(String::new()),
        TerminalRead::Line("H".to_string()),
    ]);
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(&wire::date_prompt("07-03-26"));
    expected.extend_from_slice(&wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"Scanning HOLD dir for 07-03-26... Nothing found!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn flag_by_number_inside_a_date_scan_matches_the_filtered_numbering() {
    // Slice D9 + D2f: inside a `NewSince` scan the flag registry
    // carries the RENUMBERED set — `R` + `1` flags PROTRACK (the
    // filtered `[ File #1 ]`), not ANSIPACK (the unfiltered #1), and
    // repaints its aligned marker slot in place.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![
            TerminalRead::Line("-30".to_string()),
            TerminalRead::Line("1".to_string()),
        ],
        vec![
            key(b'R'),
            key(b'1'),
            KeyRead::Key(KeyEvent::Enter),
            key(b'Y'),
        ],
    );
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Prompt).await;
    assert!(
        session
            .flagged_files()
            .contains(&FlaggedKey::new(1, "PROTRACK.LHA")),
        "the filtered File #1 (PROTRACK) is flagged",
    );
    assert!(
        !session
            .flagged_files()
            .contains(&FlaggedKey::new(1, "ANSIPACK.LHA")),
        "the unfiltered dir-1 File #1 (ANSIPACK) is NOT flagged",
    );
    assert!(
        terminal
            .output
            .windows(b"\x1b[14G[X]".len())
            .any(|w| w == b"\x1b[14G[X]"),
        "the aligned marker slot is repainted in place",
    );
}

// --- Inline path (N <args>) --------------------------------------------

#[tokio::test]
async fn inline_date_scans_the_upload_dir_by_default() {
    // N7a: `N 01-01-26` with no dir token scans the Upload (highest)
    // dir; the preamble is COUNTED (inline path), post-End More?, `Y`
    // → clear + two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(vec![key(b'Y')]);
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(26),
        },
        span: None,
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let cutoff = SystemTime::from(datetime!(2026-01-01 0:00 UTC));
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for 01-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&new_lines_since(&services, 2, cutoff)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn inline_page_one_fires_more_after_29_lines_counting_the_preamble() {
    // N7c's page-1 pin: the inline path counts its preamble (reset +
    // banner + blank + header + blank + 24 body lines = 29) — F's
    // span-path model exactly; the boundary falls mid-block. `Q` at
    // the More? → `Quit` + two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(vec![key(b'Q')]);
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(26),
        },
        span: Some(FileSpan::Dir(1)),
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let cutoff = SystemTime::from(datetime!(2026-01-01 0:00 UTC));
    let body = new_lines_since(&services, 1, cutoff);
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 1 for 01-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&body[..24]));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn inline_quick_scan_drops_description_continuations() {
    // N7q: `N 01-01-26 2 Q` shows only the first line of every
    // description — MYDEMO's continuation is absent and `[ File #3 ]`
    // follows its name row directly.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(vec![key(b'Y')]);
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(26),
        },
        span: Some(FileSpan::Dir(2)),
        quick: true,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let files = services
        .file_repo
        .list_new_since(
            FileAreaRef::new(1, 2),
            SystemTime::from(datetime!(2026-01-01 0:00 UTC)),
        )
        .expect("files");
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for 01-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&assembled(&files, true)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
    assert!(
        !String::from_utf8_lossy(&terminal.output).contains("Greets to everyone"),
        "quick mode drops MYDEMO's continuation line",
    );
}

#[tokio::test]
async fn inline_non_stop_streams_without_any_more_prompt() {
    // N7ns: `N 01-01-26 2 NS` — no More? anywhere (post-End included),
    // straight from End of File List to the two-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(26),
        },
        span: Some(FileSpan::Dir(2)),
        quick: false,
        non_stop: true,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let cutoff = SystemTime::from(datetime!(2026-01-01 0:00 UTC));
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for 01-01-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&new_lines_since(&services, 2, cutoff)));
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
    assert!(
        !terminal
            .output
            .windows(wire::MORE_PROMPT.len())
            .any(|w| w == wire::MORE_PROMPT),
        "non-stop mode never pauses",
    );
}

#[tokio::test]
async fn inline_newest_two_lists_the_ascending_tail() {
    // N7n: `N !2` — the 2 newest files of the Upload dir, ASCENDING
    // (MYDEMO then TOOLPACK; FRESHUPL absent), under the last-x
    // header.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(vec![key(b'Y')]);
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::NewestLast(2),
        span: None,
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let files = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, 2))
        .expect("files");
    let tail = &files[files.len() - 2..];
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for the last 2 files... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&assembled(tail, false)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
    let text = String::from_utf8_lossy(&terminal.output).to_string();
    let mydemo = text.find("MYDEMO.DMS").expect("MYDEMO listed");
    let toolpack = text.find("TOOLPACK.LHA").expect("TOOLPACK listed");
    assert!(mydemo < toolpack, "the tail lists ascending");
    assert!(!text.contains("FRESHUPL.LHA"), "only the newest 2 list");
}

#[tokio::test]
async fn inline_newest_overshoot_saturates_to_the_whole_dir() {
    // TO-CONFIRM #8 edge (a NextExpress choice): `N !999 2 NS` over the
    // 3-row dir lists the whole dir rather than panicking — the tail
    // take saturates.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::NewestLast(999),
        span: Some(FileSpan::Dir(2)),
        quick: false,
        non_stop: true,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let files = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, 2))
        .expect("files");
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for the last 999 files... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&assembled(&files, false)));
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn inline_today_scans_from_the_clock_day() {
    // N7t: `N T` on the (frozen) capture day — 07-03-26, nothing in
    // the seeded corpus is that fresh, so the Nothing-found header
    // runs straight into the two-reset tail (no blank between).
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Today,
        span: None,
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for 07-03-26... Nothing found!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn inline_yesterday_scans_from_the_previous_day() {
    // N7y: `N Y` → 07-02-26.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Yesterday,
        span: None,
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    assert!(
        String::from_utf8_lossy(&terminal.output)
            .contains("Scanning dir 2 for 07-02-26... Nothing found!"),
        "yesterday's label is the clock day minus one",
    );
}

#[tokio::test]
async fn inline_since_last_call_and_bare_dir_form_scan_identically() {
    // N7s / N7d: `N S` scans since the day of the previous call —
    // 06-10 here, so the same-day pair MYDEMO/TOOLPACK is INCLUDED
    // (the inclusive boundary through the whole stack) and FRESHUPL
    // (06-09) is not. The bare-dir form `N 2` provisionally shares
    // the SinceLastCall source (TO-CONFIRM #5) — byte-identical run.
    let services = demo_services_at_capture_noon();
    let run = |arg: NewFilesArg| {
        let services = &services;
        async move {
            let mut user = test_user();
            user.record_last_call(SystemTime::from(datetime!(2026-06-10 15:00 UTC)));
            let mut session = menu_session_with_user(user);
            let mut terminal = CaptureTerminal::with_keys(vec![key(b'Y')]);
            run_new_files(services, &mut terminal, &mut session, arg).await;
            terminal.output
        }
    };
    let since = run(NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::SinceLastCall,
        span: None,
        quick: false,
        non_stop: false,
    }))
    .await;
    let cutoff = SystemTime::from(datetime!(2026-06-10 0:00 UTC));
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Scanning dir 2 for 06-10-26... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&new_lines_since(&services, 2, cutoff)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&since),
        String::from_utf8_lossy(&expected),
    );
    let text = String::from_utf8_lossy(&since).to_string();
    assert!(
        text.contains("MYDEMO.DMS") && text.contains("TOOLPACK.LHA"),
        "the same-day pair is included (inclusive boundary)",
    );
    assert!(!text.contains("FRESHUPL.LHA"), "06-09 is before the cutoff");

    let bare_dir = run(NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::SinceLastCall,
        span: Some(FileSpan::Dir(2)),
        quick: false,
        non_stop: false,
    }))
    .await;
    assert_eq!(
        String::from_utf8_lossy(&bare_dir),
        String::from_utf8_lossy(&since),
    );
}

#[tokio::test]
async fn inline_reverse_scans_the_dir_newest_first() {
    // N7r: `N R 2` — exactly the FR mode over dir 2.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(vec![key(b'Y')]);
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Reverse,
        span: Some(FileSpan::Dir(2)),
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    let mut expected = new_files_preamble();
    expected.extend_from_slice(b"Reverse-scanning dir 2... Ok!\r\n\r\n");
    expected.extend_from_slice(&joined(&reversed_lines(&services, 2)));
    expected.extend_from_slice(wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn invalid_takes_the_argument_error_envelope() {
    // N7e (`N R -1`): reset line, the Copyright help banner, blanks,
    // `Argument error! Type 'n ?' for help.`, single-reset tail.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Invalid).await;
    let mut expected = b"\x1b[0m\r\n".to_vec();
    expected.extend_from_slice(wire::HELP_BANNER.as_bytes());
    expected.extend_from_slice(b"\r\n\r\n");
    expected.extend_from_slice(b"Argument error! Type 'n ?' for help.");
    expected.extend_from_slice(b"\r\n\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn inline_calendar_invalid_date_takes_the_argument_error() {
    // TO-CONFIRM #7 (provisional): a date-shaped but calendar-invalid
    // inline form (`N 13-40-26`) resolves to nothing and takes the
    // same argument-error envelope every captured bad inline form did.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    let arg = NewFilesArg::Scan(NewFilesSpec {
        request: ScanRequest::Date {
            month: 13,
            day: 40,
            year: Some(26),
        },
        span: None,
        quick: false,
        non_stop: false,
    });
    run_new_files(&services, &mut terminal, &mut session, arg).await;
    assert!(
        String::from_utf8_lossy(&terminal.output).contains("Argument error! Type 'n ?' for help."),
        "unresolvable inline dates take the argument-error envelope",
    );
}

#[tokio::test]
async fn help_writes_the_rebranded_screen_and_nothing_else() {
    // N6: `N ?` — the byte-exact rebranded help screen, alone.
    let services = demo_services_at_capture_noon();
    let mut terminal = CaptureTerminal::with_keys(Vec::new());
    let mut session = menu_session();
    run_new_files(&services, &mut terminal, &mut session, NewFilesArg::Help).await;
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        wire::NEW_FILES_HELP_SCREEN,
    );
}

// --- Pure date functions -----------------------------------------------

#[test]
fn parse_date_answer_accepts_the_advertised_forms() {
    // N1a advertises `mm-dd-yy`, `-x` and `R`; `R <date>` discards the
    // date (N4b).
    assert_eq!(parse_date_answer("R"), Some(ScanRequest::Reverse));
    assert_eq!(parse_date_answer("r"), Some(ScanRequest::Reverse));
    assert_eq!(
        parse_date_answer("R 12-30-26"),
        Some(ScanRequest::Reverse),
        "a date after R is tolerated and discarded (N4b)",
    );
    assert_eq!(parse_date_answer("-30"), Some(ScanRequest::DaysBack(30)));
    assert_eq!(
        parse_date_answer("01-01-26"),
        Some(ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(26),
        })
    );
    assert_eq!(
        parse_date_answer("6-1"),
        Some(ScanRequest::Date {
            month: 6,
            day: 1,
            year: None,
        }),
        "the year-omitted help form parses at the prompt too",
    );
}

#[test]
fn parse_date_answer_rejects_everything_else() {
    // Junk → `Error in date!` (N5). The inline-only verbs are
    // provisionally rejected at the prompt (TO-CONFIRM #2), as is
    // trailing junk after a valid date (TO-CONFIRM #11).
    for answer in ["FOO", "T", "Y", "S", "!2", "01-01-26 X", "-", "1-2-3-4"] {
        assert_eq!(
            parse_date_answer(answer),
            None,
            "answer {answer:?} must be rejected",
        );
    }
}

#[test]
fn resolve_request_derives_cutoffs_in_utc_from_the_clock() {
    let now = capture_now();
    let new_since = |date: SystemTime| ScanKind::NewSince {
        cutoff: date,
        label: super::super::dir_row::format_dir_date(date),
    };
    assert_eq!(
        resolve_request(ScanRequest::Today, now, None),
        Some(new_since(SystemTime::from(datetime!(2026-07-03 0:00 UTC)))),
    );
    assert_eq!(
        resolve_request(ScanRequest::Yesterday, now, None),
        Some(new_since(SystemTime::from(datetime!(2026-07-02 0:00 UTC)))),
    );
    // `-30` from 07-03 underflows the month → 06-03 (capture N3).
    assert_eq!(
        resolve_request(ScanRequest::DaysBack(30), now, None),
        Some(new_since(SystemTime::from(datetime!(2026-06-03 0:00 UTC)))),
    );
    // Crossing a year boundary backwards.
    assert_eq!(
        resolve_request(
            ScanRequest::DaysBack(30),
            SystemTime::from(datetime!(2026-01-15 12:00 UTC)),
            None,
        ),
        Some(new_since(SystemTime::from(datetime!(2025-12-16 0:00 UTC)))),
    );
    // SinceLastCall: the day of the previous call; a first-time caller
    // gets today (TO-CONFIRM #12).
    assert_eq!(
        resolve_request(
            ScanRequest::SinceLastCall,
            now,
            Some(SystemTime::from(datetime!(2026-06-25 15:00 UTC))),
        ),
        Some(new_since(SystemTime::from(datetime!(2026-06-25 0:00 UTC)))),
    );
    assert_eq!(
        resolve_request(ScanRequest::SinceLastCall, now, None),
        Some(new_since(SystemTime::from(datetime!(2026-07-03 0:00 UTC)))),
    );
}

#[test]
fn resolve_request_pivots_two_digit_years_at_77() {
    // `axconsts.e:41` TWODIGITYEARSWITCHOVER: yy > 77 → 19yy, else
    // 20yy. The labels are identical two-digit shapes, so the cutoff
    // instant is the discriminator.
    let now = capture_now();
    let cutoff_of = |year| match resolve_request(
        ScanRequest::Date {
            month: 1,
            day: 1,
            year: Some(year),
        },
        now,
        None,
    ) {
        Some(ScanKind::NewSince { cutoff, .. }) => cutoff,
        other => panic!("expected NewSince, got {other:?}"),
    };
    assert_eq!(
        cutoff_of(78),
        SystemTime::from(datetime!(1978-01-01 0:00 UTC))
    );
    assert_eq!(
        cutoff_of(77),
        SystemTime::from(datetime!(2077-01-01 0:00 UTC))
    );
    assert_eq!(
        cutoff_of(26),
        SystemTime::from(datetime!(2026-01-01 0:00 UTC))
    );
}

#[test]
fn resolve_request_defaults_an_omitted_year_to_the_clock_year() {
    // TO-CONFIRM #6 (provisional): `mm-dd` takes the current year.
    assert_eq!(
        resolve_request(
            ScanRequest::Date {
                month: 12,
                day: 30,
                year: None,
            },
            capture_now(),
            None,
        ),
        Some(ScanKind::NewSince {
            cutoff: SystemTime::from(datetime!(2026-12-30 0:00 UTC)),
            label: "12-30-26".to_string(),
        }),
    );
}

#[test]
fn resolve_request_rejects_calendar_invalid_dates() {
    // TO-CONFIRM #7 (provisional): date-shaped but not a real date.
    let now = capture_now();
    for (month, day) in [(13, 1), (0, 5), (2, 30), (6, 0), (99, 99)] {
        assert_eq!(
            resolve_request(
                ScanRequest::Date {
                    month,
                    day,
                    year: Some(26),
                },
                now,
                None,
            ),
            None,
            "{month:02}-{day:02}-26 must not resolve",
        );
    }
}

#[test]
fn resolve_request_maps_the_dateless_modes_straight_through() {
    let now = capture_now();
    assert_eq!(
        resolve_request(ScanRequest::Reverse, now, None),
        Some(ScanKind::Full { reverse: true }),
    );
    assert_eq!(
        resolve_request(ScanRequest::NewestLast(5), now, None),
        Some(ScanKind::NewestLast { count: 5 }),
    );
}
