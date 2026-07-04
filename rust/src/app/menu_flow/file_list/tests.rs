use crate::app::menu_command::{FileListArg, FileSpan, ZippyArg};
use crate::app::menu_flow::test_support::{
    conference, joined, key, menu_session, more_clear, services_with_demo_catalogue,
    CaptureTerminal, EXIT_TAIL,
};
use crate::app::seed;
use crate::app::services::AppServices;
use crate::app::terminal::TerminalRead;
use crate::domain::files::area::FileAreaRef;
use crate::domain::files::flagged::FlaggedFiles;

use super::test_support::{
    area_lines, listing_preamble, run_file_list, services_with_two_small_areas,
};

/// Drives the `Z` (zippy) handler against a key-/line-scripted terminal.
/// The fake terminal does not echo, so `terminal.output` is the pure
/// server-generated wire — the parity surface (slice D4,
/// `comparison/transcripts/ae_tierd_zippy*.txt`).
async fn run_zippy(services: &AppServices, terminal: &mut CaptureTerminal, arg: ZippyArg) {
    let mut session = menu_session();
    let mut flow = super::super::MenuFlow { terminal, services };
    flow.handle_zippy_search(&mut session, arg)
        .await
        .expect("zippy search");
}

/// One seeded demo file by conference, area and name — for building the
/// expected `dir_row` block of a zippy match.
fn demo_file(conf: u32, area: u32, name: &str) -> crate::domain::files::file::File {
    let confs = vec![conference(1)];
    let (_, placements) = seed::demo_file_catalogue(&confs);
    placements
        .into_iter()
        .find(|(c, a, f)| *c == conf && *a == area && f.name() == name)
        .map(|(_, _, f)| f)
        .expect("seeded demo file present")
}

/// The raw DIR rows a zippy match dumps for `file` (no frames — the
/// internal command emits `super::dir_row` verbatim), each `\r\n`-terminated.
fn zippy_block(file: &crate::domain::files::file::File) -> Vec<u8> {
    let mut out = Vec::new();
    for line in super::dir_row::dir_row_lines(file) {
        out.extend_from_slice(&line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[tokio::test]
async fn f_99_emits_the_highest_dir_error() {
    // ae_tierd_aquascan.txt:330-342 (A7), max flexed to the
    // conference's area count.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(99),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(b"The highest directory number is 2!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn f_0_takes_the_highest_dir_error_unverified() {
    // UNVERIFIED: `F 0` was not captured live; the dispatch range
    // check `1..=max` routes it with the out-of-range arguments.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(0),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(b"The highest directory number is 2!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn invalid_arguments_emit_argument_error_under_the_help_banner() {
    // ae_tierd_aquascan4.txt U4: the help-banner variant (no
    // form-feed) + the argument error + a single-reset tail.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(&services, &mut terminal, FileListArg::Invalid).await;
    let mut expected = b"\x1b[0m\r\n".to_vec();
    expected.extend_from_slice(super::wire::HELP_BANNER.as_bytes());
    expected.extend_from_slice(b"\r\n\r\n");
    expected.extend_from_slice(b"Argument error! Type 'f ?' for help.\r\n");
    expected.extend_from_slice(b"\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn bare_f_prompts_and_enter_aborts_with_a_single_reset() {
    // ae_tierd_aquascan3.txt S3 (:165-177): the door's own
    // Directories prompt; Enter alone aborts — blank, ONE reset,
    // menu (the per-path tail asymmetry).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line(String::new())]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn bare_f_junk_answer_errors_in_input() {
    // ae_tierd_aquascan.txt:109-120 (A2): a junk line at the
    // Directories prompt — blank, `Error in input!`, blank, ONE
    // reset, menu.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("XYZ".to_string())]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\nError in input!\r\n\r\n\x1b[0m\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn bare_f_numeric_answer_scans_that_dir() {
    // ae_tierd_aquascan3.txt S2 (:131-163): `2` at the prompt —
    // a Visible LINE read, unchanged by D2b — then the dir-2
    // scan with NO banner re-emit, through the post-End More?
    // where `Q` is a bare keypress (:321).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![TerminalRead::Line("2".to_string())],
        vec![key(b'Q')],
    );
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn bare_fr_prompt_uses_the_reverse_banner_then_reverse_scans_the_answer() {
    // Bare `FR` (`Prompt { reverse: true }`) opens the same Directories
    // prompt as bare `F` but under the reverse banner; answering `2`
    // reverse-scans dir 2 (newest-first) with the reverse header.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![TerminalRead::Line("2".to_string())],
        vec![key(b'Q')],
    );
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: true,
            fr_banner: true,
        },
    )
    .await;
    let mut dir2 = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, 2))
        .expect("files");
    dir2.reverse();
    let reversed: Vec<Vec<u8>> =
        super::wire::assemble_dir_lines(&dir2, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
    let mut expected = b"\x1b[0m\r\n".to_vec();
    expected.extend_from_slice(super::wire::listing_banner(true));
    expected.extend_from_slice(b"\r\n\r\n");
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Reverse-scanning dir 2... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&reversed));
    expected.extend_from_slice(super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_spaced_r_prompt_keeps_the_f_banner_and_reverse_scans() {
    // `F R` (ae_tierd_fr_probe.txt FR1): the banner right label
    // follows the typed head — `'f ?' for options`, NOT the `FR`
    // variant — while the chosen span runs the reverse scan.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![TerminalRead::Line("2".to_string())],
        vec![key(b'Q')],
    );
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: true,
            fr_banner: false,
        },
    )
    .await;
    let mut dir2 = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, 2))
        .expect("files");
    dir2.reverse();
    let reversed: Vec<Vec<u8>> =
        super::wire::assemble_dir_lines(&dir2, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
    let mut expected = b"\x1b[0m\r\n".to_vec();
    expected.extend_from_slice(super::wire::listing_banner(false));
    expected.extend_from_slice(b"\r\n\r\n");
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Reverse-scanning dir 2... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&reversed));
    expected.extend_from_slice(super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_dir_q_quick_scan_drops_description_continuations() {
    // `F 1 Q` (ae_tierd_fr_probe.txt FR3): quick mode keeps each
    // file's first description line and drops the continuations —
    // the same engine flag `N`'s `Q` token exercises (N7q).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines_and_keys(Vec::new(), vec![key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: true,
            fr_banner: false,
        },
    )
    .await;
    let out = String::from_utf8_lossy(&terminal.output);
    assert!(
        out.contains("Collection of 40 ANSI screens from the"),
        "the first description line stays",
    );
    assert!(
        !out.contains("Mirage art crew"),
        "quick drops description continuations (FR3)",
    );
}

#[tokio::test]
async fn bare_f_u_answer_scans_the_upload_dir() {
    // ae_tierd_aquascan4.txt U6: `U` at the prompt (a Visible
    // LINE read, unchanged by D2b) resolves to the
    // highest-numbered area; `Y` at the post-End More? is a bare
    // keypress.
    let services = services_with_two_small_areas();
    let mut terminal = CaptureTerminal::with_lines_and_keys(
        vec![TerminalRead::Line("U".to_string())],
        vec![key(b'Y')],
    );
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Prompt {
            reverse: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(&super::wire::directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_help_shows_the_nextscan_help_screen() {
    // ae_tierd_aquascan3.txt S1 (:100-129): form feed, the
    // Copyright help banner, the verbatim syntax text (with the
    // `- Configure NextScan` swap), and the captured epilogue.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(&services, &mut terminal, FileListArg::Help).await;
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(super::wire::HELP_SCREEN.as_bytes()),
    );
    assert!(
        terminal
            .output
            .windows(super::wire::HELP_BANNER.len())
            .any(|w| w == super::wire::HELP_BANNER.as_bytes()),
        "the help screen embeds the help banner",
    );
}

// --- Slice D4: the internal `Z` (zippy text search) handler ------------
//
// Parity surface: the genuine internal command (`Z` is not AquaScan-
// shadowed), pinned to `comparison/transcripts/ae_tierd_zippy.txt`
// (Z1-Z7) and `ae_tierd_zippy2.txt` (ZU/ZH/ZOOR). The fake terminal does
// not echo, so `terminal.output` is the pure server-generated wire.

#[tokio::test]
async fn zippy_inline_query_dumps_the_matching_block_in_the_chosen_dir() {
    // ae_tierd_zippy.txt Z1/Z2: `Z <token>` (inline) skips the search
    // prompt, opens the internal getDirSpan('') Directories prompt, and
    // after a dir-number answer dumps each matching file's raw DIR rows
    // (whole block, continuations included) under a plain "Scanning
    // directory N" header — no NextScan frames.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("1".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("STARVIEW".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 1\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "STARVIEW.LHA")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_prompt_path_reads_the_query_then_the_directory() {
    // ae_tierd_zippy.txt Z1: bare `Z` emits the 26137 blank then the
    // search prompt, reads the string, emits the 26154 blank, then the
    // getDirSpan prompt.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![
        TerminalRead::Line("STARVIEW".to_string()),
        TerminalRead::Line("1".to_string()),
    ]);
    run_zippy(&services, &mut terminal, ZippyArg::Prompt).await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(super::wire::ZIPPY_SEARCH_PROMPT);
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 1\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "STARVIEW.LHA")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_empty_query_returns_to_the_menu() {
    // ae_tierd_zippy.txt Z3: an empty search string (StrLen=0,
    // express.e:26155-26156) emits the 26154 blank and returns with no
    // directory prompt.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line(String::new())]);
    run_zippy(&services, &mut terminal, ZippyArg::Prompt).await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(super::wire::ZIPPY_SEARCH_PROMPT);
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_no_match_emits_only_the_scan_header() {
    // ae_tierd_zippy.txt Z4: a token matching nothing emits the scan
    // header and the trailing blank — no file rows.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("1".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("ZqzNoMatch".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 1\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_blank_directory_answer_aborts() {
    // ae_tierd_zippy.txt Z5: a blank getDirSpan answer is (Enter)=none —
    // emit the 26872 blank and return (express.e:26871-26873).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line(String::new())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("STARVIEW".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_all_directories_answer_walks_every_area() {
    // ae_tierd_zippy.txt Z6: `A` scans area 1 then area 2, each under
    // its own "Scanning directory N" header (no inter-dir blank).
    // PROTRACKER matches three files in dir 1, none in dir 2.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("A".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("PROTRACKER".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 1\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "MODRIPPR.LZH")));
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "PTREPLAY.LHA")));
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "PROTRACK.LHA")));
    expected.extend_from_slice(b"Scanning directory 2\r\n");
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_upload_answer_scans_the_highest_directory() {
    // ae_tierd_zippy2.txt ZU: `U` scans the highest dir (here 2),
    // rendered with its number. DEMO matches only MYDEMO.DMS there.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("U".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("DEMO".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 2\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 2, "MYDEMO.DMS")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_hold_answer_scans_the_empty_hold_directory() {
    // ae_tierd_zippy2.txt ZH: `H` scans the hold dir ("Scanning
    // directory HOLD"); the seeded hold is empty, so no rows.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("H".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("DEMO".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(super::wire::ZIPPY_SCANNING_HOLD);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_out_of_range_directory_reports_no_such_directory() {
    // ae_tierd_zippy2.txt ZOOR: a dir past the max takes the internal
    // getDirSpan error — "\r\nNo such directory.\r\n\r\n"
    // (express.e:26905), distinct from AquaScan's highest-dir error, and
    // with no 26172 blank (getDirSpan failed).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("5".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("DEMO".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(super::wire::ZIPPY_NO_SUCH_DIRECTORY);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_match_is_case_insensitive() {
    // ae_tierd_zippy.txt Z7: a lowercase token matches (UpperStr on both
    // the query and each row, express.e:26160/27597).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("1".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("starview".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 1\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "STARVIEW.LHA")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_matches_against_the_filename_row_not_just_the_description() {
    // The legacy matches the whole rendered DIR line, filename included
    // (express.e:27595-27598, InStr over each row). `XPRZMODM` appears
    // only in the filename (the description has "XPR Zmodem" with a
    // space), so a match here proves the row — not just the description
    // text — is searched.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("1".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("XPRZMODM".to_string()),
    )
    .await;

    let block = zippy_block(&demo_file(1, 1, "XPRZMODM.LHA"));
    assert!(
        terminal.output.windows(block.len()).any(|w| w == block),
        "filename-only match must dump the row: {:?}",
        String::from_utf8_lossy(&terminal.output),
    );
}

#[tokio::test]
async fn zippy_highest_dir_number_is_in_range_and_scans() {
    // Boundary: a dir answer equal to the highest dir (max) is VALID —
    // the range check is `1..=max`, not `1..max` (express.e:26904's
    // `dirScan>maxDirs`). Answering "2" scans dir 2 (where DEMO matches
    // MYDEMO.DMS), it must NOT take the out-of-range error.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("2".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("DEMO".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\nScanning directory 2\r\n");
    expected.extend_from_slice(&zippy_block(&demo_file(1, 2, "MYDEMO.DMS")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_zero_directory_is_out_of_range() {
    // Lower boundary: dir 0 (`Val('0')=0 < 1`) is out of range and takes
    // "No such directory." (express.e:26904 `dirScan<1`), not a scan of
    // some "dir 0".
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(vec![TerminalRead::Line("0".to_string())]);
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::Query("DEMO".to_string()),
    )
    .await;

    let mut expected = b"\r\n".to_vec();
    expected.extend_from_slice(&super::wire::zippy_directories_prompt(2));
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(super::wire::ZIPPY_NO_SUCH_DIRECTORY);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

// --- Slice D7: the inline `Z <term> <span>` area-spec ------------------
//
// `getDirSpan(item(1))` (express.e:26162-26163) resolves the span WITHOUT
// the Directories prompt, so these drive `handle_zippy_search` with no
// scripted line reads — pinned to `comparison/transcripts/ae_tierd_zippy3.txt`.
// `ART` matches ANSIPACK ("Mirage art crew") and ASMTUT05 ("...part 5...")
// in dir 1, nothing in dir 2.

#[tokio::test]
async fn zippy_inline_directory_span_scans_immediately_without_prompting() {
    // ae_tierd_zippy3.txt "Z ART 1": two blanks (26137 + 26172) then the
    // header and matches — no Directories prompt, no scripted reads.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "1".to_string(),
        },
    )
    .await;

    let mut expected = b"\r\n\r\nScanning directory 1\r\n".to_vec();
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "ANSIPACK.LHA")));
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "ASMTUT05.TXT")));
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_inline_all_dirs_span_walks_every_area_immediately() {
    // ae_tierd_zippy3.txt "Z ART A": inline `A` scans dir 1 (matches)
    // then dir 2 (no match), no prompt.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "A".to_string(),
        },
    )
    .await;

    let mut expected = b"\r\n\r\nScanning directory 1\r\n".to_vec();
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "ANSIPACK.LHA")));
    expected.extend_from_slice(&zippy_block(&demo_file(1, 1, "ASMTUT05.TXT")));
    expected.extend_from_slice(b"Scanning directory 2\r\n");
    expected.extend_from_slice(b"\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_inline_upload_span_scans_the_highest_dir_immediately() {
    // ae_tierd_zippy3.txt "Z ART U": inline `U` scans dir 2 (no ART
    // match there), no prompt.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "U".to_string(),
        },
    )
    .await;

    let expected = b"\r\n\r\nScanning directory 2\r\n\r\n".to_vec();
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_inline_hold_span_scans_the_hold_dir_immediately() {
    // ae_tierd_zippy3.txt "Z ART H": inline `H` scans the (empty) hold
    // dir, no prompt.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "H".to_string(),
        },
    )
    .await;

    let mut expected = b"\r\n\r\n".to_vec();
    expected.extend_from_slice(super::wire::ZIPPY_SCANNING_HOLD);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_inline_out_of_range_span_reports_no_such_directory() {
    // ae_tierd_zippy3.txt "Z ART 9": an inline out-of-range dir takes the
    // getDirSpan error immediately (no prompt, no 26172 blank).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "9".to_string(),
        },
    )
    .await;

    let mut expected = b"\r\n\r\n".to_vec();
    expected.extend_from_slice(super::wire::ZIPPY_NO_SUCH_DIRECTORY);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn zippy_inline_junk_span_reports_no_such_directory() {
    // ae_tierd_zippy3.txt "Z ART xyz": a non-U/A/H non-numeric inline
    // span is `Val=0` -> out of range -> "No such directory."
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_zippy(
        &services,
        &mut terminal,
        ZippyArg::QueryInDir {
            query: "ART".to_string(),
            span: "xyz".to_string(),
        },
    )
    .await;

    let mut expected = b"\r\n\r\n".to_vec();
    expected.extend_from_slice(super::wire::ZIPPY_NO_SUCH_DIRECTORY);
    expected.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}
