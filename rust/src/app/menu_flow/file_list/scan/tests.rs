//! Capture-replay tests for the `NextScan` scan engine (moved here
//! from `file_list/tests.rs` with slice D9's item-17 extraction —
//! assertions verbatim, only paths changed). They keep driving
//! [`handle_file_list`](crate::app::menu_flow::MenuFlow::handle_file_list):
//! the byte pins (page boundaries, repaint geometry, `?`-redraw
//! windows) are entry-point-level parity surfaces.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::app::menu_command::{FileListArg, FileSpan};
use crate::app::menu_flow::test_support::{
    conference, flag_clear, joined, key, keyed_terminal, keyed_terminal_no_ansi, menu_session,
    more_clear, services_with, services_with_demo_catalogue, test_services, CaptureTerminal,
    EXIT_TAIL,
};
use crate::app::seed;
use crate::app::services::AppServices;
use crate::app::terminal::{
    KeyEvent, KeyRead, Terminal, TerminalEcho, TerminalFuture, TerminalRead,
};
use crate::domain::files::area::FileAreaRef;
use crate::domain::files::flagged::FlaggedFiles;

use super::super::test_support::{
    area_lines, f_1_emitted_lines, listing_preamble, run_file_list, services_with_two_small_areas,
};

#[tokio::test]
async fn f_2_ns_streams_the_trio_without_pausing() {
    // The S7-shape non-stop run over the captured Dir2 trio: scan
    // header, blank, assembled body, two-reset tail — and no
    // More? prompt anywhere.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(2),
            non_stop: true,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let conferences = vec![conference(1)];
    let (_, placements) = seed::demo_file_catalogue(&conferences);
    let trio: Vec<crate::domain::files::file::File> = placements
        .into_iter()
        .filter(|(_, area, _)| *area == 2)
        .map(|(_, _, f)| f)
        .collect();
    let mut expected = listing_preamble();
    expected.extend_from_slice(b"Scanning dir 2 from top... Ok!\r\n\r\n");
    for line in super::super::wire::assemble_dir_lines(&trio, 1, &FlaggedFiles::default(), false) {
        expected.extend_from_slice(&line.bytes);
        expected.extend_from_slice(b"\r\n");
    }
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn fr_2_ns_streams_the_trio_newest_first() {
    // `FR 2 NS`: the reverse banner (`'fr ?'`) + `Reverse-scanning dir
    // 2... Ok!` header, the captured Dir2 trio emitted newest-first
    // (the forward order reversed), two-reset tail, and no More?.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(2),
            non_stop: true,
            reverse: true,
            quick: false,
            fr_banner: true,
        },
    )
    .await;
    let conferences = vec![conference(1)];
    let (_, placements) = seed::demo_file_catalogue(&conferences);
    let mut trio: Vec<crate::domain::files::file::File> = placements
        .into_iter()
        .filter(|(_, area, _)| *area == 2)
        .map(|(_, _, f)| f)
        .collect();
    trio.reverse();
    let mut expected = b"\x1b[0m\r\n".to_vec();
    expected.extend_from_slice(super::super::wire::listing_banner(true));
    expected.extend_from_slice(b"\r\n\r\n");
    expected.extend_from_slice(b"Reverse-scanning dir 2... Ok!\r\n\r\n");
    for line in super::super::wire::assemble_dir_lines(&trio, 1, &FlaggedFiles::default(), false) {
        expected.extend_from_slice(&line.bytes);
        expected.extend_from_slice(b"\r\n");
    }
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn fr_a_ns_descends_dirs_highest_first() {
    // `FR A` walks the span highest→lowest (`express.e:27654` reverse
    // walk: `fLLoop:=dirScan; fLLoop--`), so dir 2's header precedes
    // dir 1's. The forward `F A` is the opposite order.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::All,
            non_stop: true,
            reverse: true,
            quick: false,
            fr_banner: true,
        },
    )
    .await;
    let out = String::from_utf8_lossy(&terminal.output);
    let dir2 = out
        .find("Reverse-scanning dir 2")
        .expect("dir 2 reverse header present");
    let dir1 = out
        .find("Reverse-scanning dir 1")
        .expect("dir 1 reverse header present");
    assert!(
        dir2 < dir1,
        "FR A must scan the highest dir first (dir 2 before dir 1)",
    );
}

#[tokio::test]
async fn q_at_more_quits_on_a_single_keypress() {
    // The captured page-1 boundary (ae_tierd_aquascan3.txt:212,
    // S4): More? fires after exactly 29 emitted lines — right
    // after the 02-03-26 separator block. `Q` is a bare key, no
    // Enter (ae_tierd_aquascan3.txt:321, harness sent the single
    // byte): it echoes `Quit` with no clear, then the listing
    // exit tail.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn y_at_more_clears_the_prompt_and_streams_a_fresh_page() {
    // `Y` (ae_tierd_aquascan3.txt S4, bare keypress): overprint
    // clear, the counter resets, and the next 29 lines stream to
    // the next More?.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'Y'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(&joined(&lines[29..58]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn c_at_more_form_feeds_and_resumes_without_reprompt() {
    // `C` (ae_tierd_aquascan3.txt:292-321, S6, bare keypress):
    // `\r` + form feed, no overprint clear, no prompt redraw —
    // the listing resumes immediately with a reset counter.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'C'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"\r\x0c");
    expected.extend_from_slice(&joined(&lines[29..58]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn n_then_s_opens_the_nonstop_confirm_and_y_goes_nonstop() {
    // ae_tierd_aquascan3.txt S7 (:361 + repr :490) +
    // ae_tierd_aquascan4.txt U3 (:154-156): `n` echoes on its own
    // keypress, `s` wipes the prompt line (echoed n included)
    // with the 69-space overprint and asks the Are-you-sure
    // confirm; `Y` (unechoed) clears again and the rest of the
    // listing streams with no further More? — footer straight to
    // the exit tail. The aggregate bytes for n-then-s are
    // identical to the old same-packet `ns` line.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'n'), key(b's'), key(b'Y')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"n");
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::NS_CONFIRM_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(&joined(&lines[29..]));
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn declining_the_ns_confirm_redraws_more_and_stays_paged() {
    // ae_tierd_aquascan4.txt U3: `n` at the confirm (a single
    // unechoed keypress) clears and More? redraws; paged mode
    // continues.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'n'), key(b's'), key(b'n'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"n");
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::NS_CONFIRM_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn held_n_then_other_key_erases_and_runs_the_verb() {
    // ae_tierd_aquascan4.txt U1 (:133): `n` echoes on its own
    // keypress and holds (ambiguous N=Quit / ns prefix); the next
    // key first erases it with BS-SP-BS, then runs as its own
    // verb — `n` then `Q` gives `n` … `\x08 \x08Quit`.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'n'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"n");
    expected.extend_from_slice(b"\x08 \x08");
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn lone_n_echoes_holds_then_enter_quits() {
    // Probe P1 (ae_tierd_probes.txt:100-138): Enter after a held
    // `n` quits with the CR echoed as `\r\n` and the exit tail
    // following directly — NO `Quit` word, NO BS-SP-BS; the held
    // `n` stays on the prompt line.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'n'), KeyRead::Key(KeyEvent::Enter)]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"n");
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

/// Runs `F 2` with `keys` scripted at its post-End `More?` and
/// returns `(actual output, expected bytes up to and including
/// the More? prompt)` — shared by the single-key resume pins
/// below, which append their verb's bytes to the expectation.
async fn f_2_more_output(keys: Vec<KeyRead>) -> (Vec<u8>, Vec<u8>) {
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(2),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = joined(&[
        b"\x1b[0m".to_vec(),
        super::super::wire::LISTING_BANNER.to_vec(),
        Vec::new(),
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]);
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    (terminal.output, expected)
}

#[tokio::test]
async fn enter_at_more_without_a_held_n_resumes_via_the_overprint() {
    // Design §4 (2026-06-12, probe-corrected): Enter at More?
    // with no held `n` is a continue — the captured 69-space
    // overprint resume (ae_tierd_aquascan3.txt S4 shape), NOT
    // the held-n quit of probe P1. Dir 2 is the last dir, so
    // the resume runs straight into the exit tail.
    let (output, mut expected) = f_2_more_output(vec![KeyRead::Key(KeyEvent::Enter)]).await;
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn unknown_key_at_more_resumes_via_the_overprint() {
    // The door's default verb: unknown keys clear with the
    // 69-space overprint and resume (ae_tierd_aquascan3.txt S4
    // overprint shape; specific key uncaptured — inference
    // recorded in COMMAND_PARITY.md).
    let (output, mut expected) = f_2_more_output(vec![key(b'X')]).await;
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn k_at_more_skips_the_rest_of_the_current_dir() {
    // `K` (ae_tierd_help_audit.txt PK, bare keypress): the verb's
    // overprint clear, then the walk's dir-transition CRLF, then the
    // next dir's header — the rest of dir 1, its footer and its
    // post-End More? are all abandoned.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'K'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::All,
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    // `F A`'s page 1 is byte-identical to `F 1`'s (same preamble,
    // same dir-1 header and body).
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn l_at_more_reloads_the_current_dir_from_the_top() {
    // `L` (ae_tierd_help_audit.txt PL, bare keypress): a form feed +
    // CRLF, then the current dir restarts from its header with a
    // fresh page counter — the entry preamble is NOT re-emitted.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'L'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(2),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let dir2 = |expected: &mut Vec<u8>| {
        expected.extend_from_slice(&joined(&[
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]));
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    };
    let mut expected = joined(&[
        b"\x1b[0m".to_vec(),
        super::super::wire::LISTING_BANNER.to_vec(),
        Vec::new(),
    ]);
    dir2(&mut expected);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"\x0c\r\n");
    dir2(&mut expected);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn l_reload_replaces_numeric_identity_with_the_refetched_catalogue() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};
    use crate::domain::files::repository::{FileRepository, FileRepositoryError};

    struct ReloadingFileRepository {
        reads: AtomicUsize,
        old: File,
        new: File,
    }

    impl FileRepository for ReloadingFileRepository {
        fn areas_in_conference(
            &self,
            conference: u32,
        ) -> Result<Vec<FileArea>, FileRepositoryError> {
            Ok((conference == 1)
                .then(|| FileArea::new(1, 1, "Main".to_string()))
                .into_iter()
                .collect())
        }

        fn find_in_area(&self, _area: FileAreaRef) -> Result<Vec<File>, FileRepositoryError> {
            let file = if self.reads.fetch_add(1, Ordering::SeqCst) == 0 {
                &self.old
            } else {
                &self.new
            };
            Ok(vec![file.clone()])
        }

        fn list_held(&self, _conference: u32) -> Result<Vec<File>, FileRepositoryError> {
            Ok(Vec::new())
        }

        fn list_new_since(
            &self,
            area: FileAreaRef,
            _since: SystemTime,
        ) -> Result<Vec<File>, FileRepositoryError> {
            self.find_in_area(area)
        }
    }

    let file = |name: &str| {
        File::new(
            name.to_string(),
            Bytes::new(1_000),
            FileStatus::Available,
            Some(b'P'),
            format!("{name} description"),
            SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
        )
    };
    let mut services = test_services();
    services.file_repo = Arc::new(ReloadingFileRepository {
        reads: AtomicUsize::new(0),
        old: file("OLD.LHA"),
        new: file("NEW.LHA"),
    });
    let mut session = menu_session();
    let mut terminal = keyed_terminal(vec![
        key(b'L'),
        key(b'R'),
        key(b'1'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("reloaded listing");
    }

    let output = String::from_utf8_lossy(&terminal.output);
    assert!(output.contains("OLD.LHA"));
    assert!(output.contains("NEW.LHA"));
    let flags = session.flagged_files_mut();
    assert!(
        flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1, "NEW.LHA",
        ))
    );
    assert!(
        !flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1, "OLD.LHA",
        ))
    );
}

#[tokio::test]
async fn ctrl_c_at_more_quits_with_the_break_banner() {
    // Ctrl-C (ae_tierd_help_audit.txt PCC, bare keypress): `\r\n`,
    // the reset `**Break` line, then the standard two-reset exit
    // tail — the in-pager help's "Quit (Can be used at any time)",
    // pinned at the More? prompt. Driven as the adapter delivers it:
    // `KeyEvent::CtrlC`, not a `Char` (the PCC replay caught the raw
    // 0x03 mapping to `Other` and resuming).
    let (output, mut expected) = f_2_more_output(vec![KeyRead::Key(KeyEvent::CtrlC)]).await;
    expected.extend_from_slice(b"\r\n\x1b[0m**Break\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn l_at_post_end_reloads_with_the_form_feed_counted() {
    // The reload path from the post-End More? (as opposed to a
    // mid-list page boundary): the FF pre-count must survive
    // `post_end_pause`'s counter reset, so the reloaded page's More?
    // still fires after FF + 28 lines (PL's captured boundary).
    let services = services_with_demo_catalogue();
    let lines = f_1_emitted_lines(&services);
    let full_pages = lines.len() / 29;
    assert_ne!(
        lines.len() % 29,
        0,
        "the fixture must leave a partial last page so the L lands on \
         the post-End More?, not a page boundary",
    );
    let mut keys = vec![key(b'Y'); full_pages];
    keys.push(key(b'L'));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = Vec::new();
    for page in 0..full_pages {
        expected.extend_from_slice(&joined(&lines[page * 29..(page + 1) * 29]));
        expected.extend_from_slice(super::super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
    }
    expected.extend_from_slice(&joined(&lines[full_pages * 29..]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"\x0c\r\n");
    expected.extend_from_slice(&joined(&lines[3..31]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn l_reload_counts_the_form_feed_toward_the_fresh_page() {
    // PL's captured boundary: the reloaded page's More? fires after
    // the form-feed line plus 28 listing lines — the door counts what
    // it prints, the `\x0c\r\n` line included.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'L'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"\x0c\r\n");
    // The reload re-emits from the dir header (preamble is lines
    // 0..3) and pages after 28 more lines: FF + 28 = the 29-line page.
    expected.extend_from_slice(&joined(&lines[3..31]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_at_more_flag_prompt_emits_no_confirmation_bytes() {
    // ae_tierd_aquascan3.txt S4 (:212-217) + probe P3
    // (ae_tierd_probes.txt, per-keystroke echo at the flag read):
    // clear, the flag prompt, each typed char echoed as it
    // arrives (aggregate identical to the old verbatim replay),
    // Enter finishing with NO trailing CRLF, the wider clear,
    // More? redrawn — and no confirmation text anywhere. Flagging
    // is silent at the prompt (only the session set changes); the
    // typed TERMV48X.LHA is not in the dir-1 display, so legacy name
    // handling stores it but no in-place row repaint fires, leaving
    // the wire bytes unaffected.
    let services = services_with_demo_catalogue();
    let mut keys = vec![key(b'F')];
    keys.extend(b"TERMV48X.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::FLAG_BY_NAME_PROMPT);
    expected.extend_from_slice(b"TERMV48X.LHA");
    expected.extend_from_slice(&flag_clear());
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn flag_entry_backspace_erases_with_bs_sp_bs_and_skips_an_empty_entry() {
    // Design §4 (2026-06-12; uncaptured — the probe battery only
    // exercised printables, P3): Backspace at the flag entry
    // erases the last char with BS-SP-BS, and a Backspace on an
    // empty entry emits nothing. Keys: F, BS (empty — silent),
    // T, X, BS, Enter — the echo stream is `TX` + BS-SP-BS, then
    // the captured wider clear and More? redraw.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![
        key(b'F'),
        KeyRead::Key(KeyEvent::Backspace),
        key(b'T'),
        key(b'X'),
        KeyRead::Key(KeyEvent::Backspace),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::FLAG_BY_NAME_PROMPT);
    expected.extend_from_slice(b"TX\x08 \x08");
    expected.extend_from_slice(&flag_clear());
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn flag_entry_stops_echoing_at_the_terminal_line_byte_limit() {
    // NextExpress bound, not captured behaviour: the flag entry
    // collector shares MAX_TERMINAL_LINE_BYTES (4096) with the
    // line reads — the 4097th printable is dropped unechoed.
    let services = services_with_demo_catalogue();
    let limit = crate::app::input_limits::MAX_TERMINAL_LINE_BYTES;
    let mut keys = vec![key(b'F')];
    keys.extend(std::iter::repeat_n(key(b'A'), limit + 1));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::FLAG_BY_NAME_PROMPT);
    expected.extend(std::iter::repeat_n(b'A', limit));
    expected.extend_from_slice(&flag_clear());
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn r_at_more_opens_the_distinct_flag_by_number_prompt() {
    // ae_tierd_aquascan3.txt S5 (:252-257): `R` uses the
    // `File number(s) to flag:` wording; the entry is typed
    // per-keystroke (probe P3) and finished with Enter. The number
    // `99` matches no listed row, so nothing is flagged and no
    // in-place repaint (Task 3.4b) fires — keeping this prompt-
    // wording pin byte-exact; the repaint is exercised by
    // `flagging_by_number_repaints_the_row`.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![
        key(b'R'),
        key(b'9'),
        key(b'9'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(super::super::wire::FLAG_BY_NUMBER_PROMPT);
    expected.extend_from_slice(b"99");
    expected.extend_from_slice(&flag_clear());
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[test]
fn displayed_selection_index_is_dense_and_replaced_for_each_directory() {
    use crate::domain::files::flagged::FlaggedKey;
    let row = |number, name| super::ListedRow {
        key: FlaggedKey::new(1, name),
        number: Some(number),
        aligned: true,
    };
    let mut displayed = super::DisplayedSelectionIndex::default();
    displayed.record(&row(1, "FIRST.LHA"));
    displayed.record(&super::ListedRow {
        key: FlaggedKey::new(1, "PLAINROW.LHA"),
        number: None,
        aligned: false,
    });
    displayed.record(&row(2, "SECOND.LHA"));
    assert_eq!(displayed.resolve(1), Some(&FlaggedKey::new(1, "FIRST.LHA")),);
    assert_eq!(
        displayed.resolve(2),
        Some(&FlaggedKey::new(1, "SECOND.LHA")),
    );

    displayed.begin_directory();
    displayed.record(&row(1, "REPLACEMENT.LHA"));
    assert_eq!(
        displayed.resolve(1),
        Some(&FlaggedKey::new(1, "REPLACEMENT.LHA")),
    );
    assert_eq!(displayed.resolve(2), None);
    assert_eq!(displayed.resolve(0), None);
}

#[test]
#[should_panic(expected = "displayed file numbers must be dense")]
fn displayed_selection_index_rejects_a_non_dense_first_number() {
    let mut displayed = super::DisplayedSelectionIndex::default();
    displayed.record(&super::ListedRow {
        key: crate::domain::files::flagged::FlaggedKey::new(1, "SECOND.LHA"),
        number: Some(2),
        aligned: true,
    });
}

#[test]
fn numeric_flag_plan_accepts_valid_tokens_and_ignores_invalid_tokens() {
    use crate::domain::files::flagged::FlaggedKey;
    let mut displayed = super::DisplayedSelectionIndex::default();
    for (number, name) in [(1, "FIRST.LHA"), (2, "SECOND.LHA")] {
        displayed.record(&super::ListedRow {
            key: FlaggedKey::new(1, name),
            number: Some(number),
            aligned: true,
        });
    }
    let flagged = FlaggedFiles::default();
    assert_eq!(
        super::plan_flags("1 2 1", true, 1, &displayed, &flagged),
        vec![
            FlaggedKey::new(1, "FIRST.LHA"),
            FlaggedKey::new(1, "SECOND.LHA"),
        ],
    );
    assert_eq!(
        super::plan_flags("1 garbage", true, 1, &displayed, &flagged),
        vec![FlaggedKey::new(1, "FIRST.LHA")],
    );
    for entry in ["", "   ", "0", "999", "abc", "+1", "-1", "1garbage"] {
        assert!(
            super::plan_flags(entry, true, 1, &displayed, &flagged).is_empty(),
            "{entry:?} is a silent no-op",
        );
    }

    let mut already_flagged = FlaggedFiles::default();
    already_flagged.flag(FlaggedKey::new(1, "FIRST.LHA"));
    assert_eq!(
        super::plan_flags("1 2", true, 1, &displayed, &already_flagged),
        vec![FlaggedKey::new(1, "SECOND.LHA")],
    );
}

#[test]
fn name_flag_plan_is_unchecked_trimmed_uppercase_whole_line() {
    use crate::domain::files::flagged::FlaggedKey;
    let displayed = super::DisplayedSelectionIndex::default();
    let flagged = FlaggedFiles::default();
    assert_eq!(
        super::plan_flags("  my demo.lha  ", false, 7, &displayed, &flagged),
        vec![FlaggedKey::new(7, "MY DEMO.LHA")],
    );
    for entry in ["", "   ", " a "] {
        assert!(super::plan_flags(entry, false, 7, &displayed, &flagged).is_empty());
    }
    let mut already_flagged = FlaggedFiles::default();
    already_flagged.flag(FlaggedKey::new(7, "MY DEMO.LHA"));
    assert!(super::plan_flags("my demo.lha", false, 7, &displayed, &already_flagged,).is_empty(),);
}

#[tokio::test]
async fn numeric_selection_is_replaced_when_the_scan_enters_the_next_directory() {
    // D10 live capture `F A`: each directory restarts at File #1, and
    // `R 1` in directory 2 selects that directory's row rather than
    // the stale first match from directory 1
    // (`ae_tierd_d10_selection.txt:188-398`).
    let services = services_with_two_small_areas();
    let mut session = menu_session();
    let mut terminal = keyed_terminal(vec![
        key(b'Y'),
        key(b'R'),
        key(b'1'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::All,
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("two-directory listing");
    }

    let flags = session.flagged_files_mut();
    assert!(
        flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1,
            "SECOND.LHA",
        )),
        "directory 2's displayed File #1 must win",
    );
    assert!(
        !flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1,
            "FIRST.LHA",
        )),
        "directory 1's stale File #1 must not remain selectable",
    );
}

#[tokio::test]
async fn page_boundary_row_is_registered_before_more_accepts_numeric_selection() {
    use crate::domain::files::flagged::FlaggedKey;

    let services = test_services();
    let mut terminal = keyed_terminal_no_ansi(vec![
        key(b'R'),
        key(b'1'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    let mut state = super::ScanState::new(false, 1);
    state.emitted = super::PAGE_LINES - 1;
    state.displayed.record(&super::ListedRow {
        key: FlaggedKey::new(1, "EARLIER.LHA"),
        number: Some(1),
        aligned: true,
    });
    let mut flagged = FlaggedFiles::default();
    let row = super::ScanLine {
        bytes: b"BOUNDARY.LHA".to_vec(),
        listed: Some(super::ListedRow {
            key: FlaggedKey::new(1, "BOUNDARY.LHA"),
            number: Some(2),
            aligned: true,
        }),
    };
    let flow = {
        let mut menu = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        menu.emit_scan_line(&mut state, row, &mut flagged)
            .await
            .expect("boundary row")
    };

    assert_eq!(flow, super::ScanFlow::Quit);
    assert!(
        flagged.contains(&FlaggedKey::new(1, "EARLIER.LHA")),
        "an earlier-page number remains selectable at the next boundary",
    );
    assert!(!flagged.contains(&FlaggedKey::new(1, "BOUNDARY.LHA")));
    assert_eq!(
        state.displayed.resolve(2),
        Some(&FlaggedKey::new(1, "BOUNDARY.LHA")),
        "the fully written boundary row is registered before More?",
    );
    assert!(terminal.output.starts_with(b"BOUNDARY.LHA\r\n"));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InjectedWriteFailure;

struct FailOnNthWriteTerminal {
    writes: usize,
    fail_on: usize,
}

impl Terminal for FailOnNthWriteTerminal {
    type Error = InjectedWriteFailure;

    fn write<'a>(&'a mut self, _bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move {
            self.writes += 1;
            if self.writes == self.fail_on {
                Err(InjectedWriteFailure)
            } else {
                Ok(())
            }
        })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn read_line(
        &mut self,
        _echo: TerminalEcho,
        _timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async { Ok(TerminalRead::Eof) })
    }
}

#[tokio::test]
async fn failed_row_or_crlf_write_does_not_register_the_row() {
    use crate::domain::files::flagged::FlaggedKey;

    let services = test_services();
    for fail_on in [1, 2] {
        let mut terminal = FailOnNthWriteTerminal { writes: 0, fail_on };
        let mut state = super::ScanState::new(false, 1);
        let mut flagged = FlaggedFiles::default();
        let row = super::ScanLine {
            bytes: b"PARTIAL.LHA".to_vec(),
            listed: Some(super::ListedRow {
                key: FlaggedKey::new(1, "PARTIAL.LHA"),
                number: Some(1),
                aligned: true,
            }),
        };
        let result = {
            let mut menu = crate::app::menu_flow::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            menu.emit_scan_line(&mut state, row, &mut flagged).await
        };

        assert_eq!(result, Err(InjectedWriteFailure), "write {fail_on}");
        assert_eq!(state.displayed.resolve(1), None, "write {fail_on}");
        assert!(state.page.is_empty(), "write {fail_on}");
        assert_eq!(state.emitted, 0, "write {fail_on}");
        assert!(flagged.is_empty(), "write {fail_on}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagFailurePoint {
    Clear,
    Repaint,
    More,
}

/// Injects one failure after a flag entry has been planned but before
/// its in-memory commit.
struct FailDuringFlagRedrawTerminal {
    keys: VecDeque<KeyRead>,
    fail_at: FlagFailurePoint,
    armed: bool,
}

impl Terminal for FailDuringFlagRedrawTerminal {
    type Error = InjectedWriteFailure;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move {
            if bytes == flag_clear() {
                if self.fail_at == FlagFailurePoint::Clear {
                    return Err(InjectedWriteFailure);
                }
                self.armed = true;
            }
            if self.armed
                && self.fail_at == FlagFailurePoint::Repaint
                && bytes
                    .windows(b"\x1b[14G[X]".len())
                    .any(|window| window == b"\x1b[14G[X]")
            {
                return Err(InjectedWriteFailure);
            }
            if self.armed
                && self.fail_at == FlagFailurePoint::More
                && bytes == super::super::wire::MORE_PROMPT
            {
                return Err(InjectedWriteFailure);
            }
            Ok(())
        })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn read_line(
        &mut self,
        _echo: TerminalEcho,
        _timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async { Ok(TerminalRead::Eof) })
    }

    fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        let key = self.keys.pop_front().unwrap_or(KeyRead::Eof);
        Box::pin(async move { Ok(key) })
    }
}

#[tokio::test]
async fn flag_is_not_committed_when_any_post_plan_redraw_write_fails() {
    // Hardening §10.4: accepting a complete entry is not the commit
    // point. A failure in the clear, repaint, or restored More? leaves
    // the session set unchanged.
    for fail_at in [
        FlagFailurePoint::Clear,
        FlagFailurePoint::Repaint,
        FlagFailurePoint::More,
    ] {
        let services = services_with_demo_catalogue();
        let mut session = menu_session();
        let mut terminal = FailDuringFlagRedrawTerminal {
            keys: VecDeque::from(vec![key(b'R'), key(b'1'), KeyRead::Key(KeyEvent::Enter)]),
            fail_at,
            armed: false,
        };
        let result = {
            let mut flow = crate::app::menu_flow::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.handle_file_list(
                &mut session,
                FileListArg::Span {
                    span: FileSpan::Dir(1),
                    non_stop: false,
                    reverse: false,
                    quick: false,
                    fr_banner: false,
                },
            )
            .await
        };

        assert_eq!(result, Err(InjectedWriteFailure), "{fail_at:?}");
        assert!(
            session.flagged_files_mut().is_empty(),
            "{fail_at:?} must not commit any key",
        );
    }
}

#[tokio::test]
async fn flag_entry_eof_or_idle_timeout_does_not_commit() {
    for abort in [KeyRead::Eof, KeyRead::IdleTimedOut] {
        let services = services_with_demo_catalogue();
        let mut session = menu_session();
        let mut terminal = keyed_terminal(vec![key(b'R'), abort]);
        {
            let mut flow = crate::app::menu_flow::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.handle_file_list(
                &mut session,
                FileListArg::Span {
                    span: FileSpan::Dir(1),
                    non_stop: false,
                    reverse: false,
                    quick: false,
                    fr_banner: false,
                },
            )
            .await
            .expect("carrier/idle exits the listing cleanly");
        }
        assert!(session.flagged_files_mut().is_empty(), "{abort:?}");
    }
}

#[tokio::test]
async fn flagging_a_file_makes_it_render_with_the_marker_on_re_list() {
    // End-to-end (slice D2f): flag a dir-1 file via the `F` verb,
    // then re-run `F 1` on the SAME session — the row now renders
    // the `[X]` slot the first listing showed blank. Proves the
    // lister reads the session flag set the verb mutated.
    let services = services_with_demo_catalogue();
    let mut session = menu_session();

    // First listing: pause at More?, flag ANSIPACK.LHA (dir-1 #1,
    // on page 1), then quit.
    let mut keys = vec![key(b'F')];
    keys.extend(b"ANSIPACK.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("first listing");
    }
    let first_len = terminal.output.len();
    let unflagged = b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m    P";
    assert!(
        terminal.output[..first_len]
            .windows(unflagged.len())
            .any(|w| w == unflagged),
        "the first listing must render ANSIPACK unflagged",
    );

    // Second listing on the same session: page, then quit.
    terminal.keys = VecDeque::from(vec![key(b'Q')]);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("second listing");
    }
    let flagged_row = b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m[X] P";
    assert!(
        terminal.output[first_len..]
            .windows(flagged_row.len())
            .any(|w| w == flagged_row),
        "re-listing must render ANSIPACK flagged: {:?}",
        String::from_utf8_lossy(&terminal.output[first_len..]),
    );
}

/// Derives the in-place repaint sequence for a row on the first
/// page of `F 1`: the row whose framed bytes start with `prefix`
/// sits `up` lines above the prompt (`up = 29 - index`), so the
/// expected wire is `\r ESC[<up>A <column_cmd> \r ESC[<up>B`.
/// Aligned rows take `ESC[14G[X]`; over-long rows a trailing slot.
fn f_1_repaint_sequence(services: &AppServices, prefix: &[u8]) -> Vec<u8> {
    let lines = f_1_emitted_lines(services);
    let page = &lines[..29];
    let index = page
        .iter()
        .position(|line| line.starts_with(prefix))
        .expect("the flagged row is on page 1");
    let up = 29 - index;
    let mut seq = format!("\r\x1b[{up}A").into_bytes();
    // The seed's ANSIPACK is framed (aligned), so the slot lands
    // at visible column 14.
    seq.extend_from_slice(b"\x1b[14G[X]");
    seq.extend_from_slice(format!("\r\x1b[{up}B").as_bytes());
    seq
}

#[tokio::test]
async fn flagging_a_visible_aligned_row_repaints_the_marker_in_place() {
    // Slice D2f (Task 3.4b): `F`-flagging ANSIPACK.LHA (dir-1 #1,
    // on page 1) paints `[X]` into its marker slot in place —
    // `\r`, cursor up to the row, `ESC[14G[X]`, cursor back —
    // emitted AFTER the 79-space flag-overprint clear and BEFORE
    // the More? redraw.
    let services = services_with_demo_catalogue();
    let mut keys = vec![key(b'F')];
    keys.extend(b"ANSIPACK.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;

    let repaint = f_1_repaint_sequence(&services, b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m");
    // The repaint lands between the flag clear and the More? redraw.
    let clear = flag_clear();
    let clear_at = terminal
        .output
        .windows(clear.len())
        .position(|w| w == clear.as_slice())
        .expect("the flag overprint clear is emitted");
    let after_clear = &terminal.output[clear_at + clear.len()..];
    assert!(
        after_clear.starts_with(&repaint),
        "the repaint sequence follows the flag clear immediately: {:?}",
        String::from_utf8_lossy(&after_clear[..repaint.len().min(after_clear.len())]),
    );
    let after_repaint = &after_clear[repaint.len()..];
    assert!(
        after_repaint.starts_with(super::super::wire::MORE_PROMPT),
        "More? redraws right after the repaint",
    );
    // `ESC[14G[X]` appears exactly once — only the one flagged row.
    let marker = b"\x1b[14G[X]";
    assert_eq!(
        terminal
            .output
            .windows(marker.len())
            .filter(|w| *w == marker)
            .count(),
        1,
        "exactly one aligned-slot repaint",
    );
}

#[tokio::test]
async fn flagging_by_number_repaints_the_row() {
    // Slice D2f (Task 3.4b): `R 1` flags ANSIPACK by its
    // `[ File #1 ]` number and repaints its row identically to the
    // by-name path.
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![
        key(b'R'),
        key(b'1'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;

    let repaint = f_1_repaint_sequence(&services, b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m");
    let clear = flag_clear();
    let clear_at = terminal
        .output
        .windows(clear.len())
        .position(|w| w == clear.as_slice())
        .expect("the flag overprint clear is emitted");
    let after_clear = &terminal.output[clear_at + clear.len()..];
    assert!(
        after_clear.starts_with(&repaint),
        "the R-path repaint follows the flag clear: {:?}",
        String::from_utf8_lossy(&after_clear[..repaint.len().min(after_clear.len())]),
    );
    assert!(
        after_clear[repaint.len()..].starts_with(super::super::wire::MORE_PROMPT),
        "More? redraws after the R-path repaint",
    );
}

#[tokio::test]
async fn flagging_an_unlisted_name_emits_no_repaint() {
    // D10 capture: AquaScan accepts an unchecked name absent from the
    // catalogue (`NOSUCH.LHA`) and `express.e:12523-12542` stores it,
    // but there is no visible row to repaint.
    let services = services_with_demo_catalogue();
    let mut session = menu_session();
    let mut keys = vec![key(b'F')];
    keys.extend(b"NOSUCH.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("listing");
    }

    assert!(
        session
            .flagged_files_mut()
            .contains(&crate::domain::files::flagged::FlaggedKey::new(
                1,
                "NOSUCH.LHA",
            )),
        "the unchecked whole-line name is still committed",
    );

    let clear = flag_clear();
    let clear_at = terminal
        .output
        .windows(clear.len())
        .position(|w| w == clear.as_slice())
        .expect("the flag overprint clear is emitted");
    let after_clear = &terminal.output[clear_at + clear.len()..];
    assert!(
        after_clear.starts_with(super::super::wire::MORE_PROMPT),
        "More? redraws directly after the clear with no repaint: {:?}",
        String::from_utf8_lossy(
            &after_clear[..super::super::wire::MORE_PROMPT.len().min(after_clear.len())]
        ),
    );
    // No aligned-slot repaint move anywhere — no listed row matched.
    assert!(
        !terminal
            .output
            .windows(b"\x1b[14G".len())
            .any(|w| w == b"\x1b[14G"),
        "no repaint CSI is emitted for an unlisted name",
    );
}

#[tokio::test]
async fn name_selection_preserves_one_trimmed_space_containing_whole_line() {
    // D10 edge capture E2 plus `express.e:12638`: the name prompt
    // passes its entire line once to addFlagToList. It is not a list of
    // catalogue-name tokens, even when each word is itself listed.
    let services = services_with_demo_catalogue();
    let mut session = menu_session();
    let entry = b"  ansipack.lha termv48.lha  ";
    let mut keys = vec![key(b'F')];
    keys.extend(entry.iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("listing");
    }

    let flags = session.flagged_files_mut();
    assert!(
        flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1,
            "ANSIPACK.LHA TERMV48.LHA",
        ))
    );
    assert!(
        !flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1,
            "ANSIPACK.LHA",
        ))
    );
    assert!(
        !flags.contains(&crate::domain::files::flagged::FlaggedKey::new(
            1,
            "TERMV48.LHA",
        ))
    );
}

#[tokio::test]
async fn repaint_is_suppressed_when_ansi_is_off() {
    // Slice D2f (Task 3.4b): with ANSI colour off the flag STILL
    // lands (the session set records it) but the cursor CSI is
    // suppressed — a non-ANSI client would garble on it.
    let services = services_with_demo_catalogue();
    let mut session = menu_session();
    let mut keys = vec![key(b'F')];
    keys.extend(b"ANSIPACK.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal_no_ansi(keys);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("listing with ansi off");
    }

    // The flag landed in the session set.
    assert!(
        session
            .flagged_files_mut()
            .contains(&crate::domain::files::flagged::FlaggedKey::new(
                1,
                "ANSIPACK.LHA",
            )),
        "the flag lands even with ANSI off",
    );
    // No repaint CSI: neither the aligned-slot move nor a cursor-up.
    assert!(
        !terminal
            .output
            .windows(b"\x1b[14G".len())
            .any(|w| w == b"\x1b[14G"),
        "the aligned-slot repaint is suppressed with ANSI off",
    );
}

#[tokio::test]
async fn flagging_a_visible_overlong_row_repaints_a_trailing_slot() {
    // Slice D2f (Task 3.4b): an over-long (unframeable) row has no
    // aligned slot, so the repaint appends ` [X]` at the column
    // just past its last visible column — `ESC[<vis+1>G [X]` —
    // rather than the `ESC[14G[X]` of the aligned branch. A one-
    // file area keeps the row on page 1 with a tiny page.
    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};
    let file = File::new(
        "ALONGFILENAME.LHA".to_string(),
        Bytes::new(77_777),
        FileStatus::Available,
        None,
        "Long filename breaks the columns".to_string(),
        SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
    );
    let services = services_with(InMemoryFileRepository::new(
        vec![FileArea::new(1, 1, "Main".to_string())],
        vec![(1, 1, file.clone())],
    ));
    let mut keys = vec![key(b'F')];
    keys.extend(b"ALONGFILENAME.LHA".iter().map(|&c| key(c)));
    keys.push(KeyRead::Key(KeyEvent::Enter));
    keys.push(key(b'Q'));
    let mut terminal = keyed_terminal(keys);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;

    // The page: 5 preamble lines, the over-long row (index 5), the
    // footer (index 6) — page.len() == 7, so up == 7 - 5 == 2. The
    // unflagged row's visible columns set the trailing-slot column.
    let unflagged_row = super::super::wire::assemble_dir_lines(
        std::slice::from_ref(&file),
        1,
        &FlaggedFiles::default(),
        false,
    );
    let vis = super::super::wire::visible_columns(&unflagged_row[0].bytes);
    let mut repaint = b"\r\x1b[2A".to_vec();
    repaint.extend_from_slice(format!("\x1b[{}G [X]", vis + 1).as_bytes());
    repaint.extend_from_slice(b"\r\x1b[2B");

    let clear = flag_clear();
    let clear_at = terminal
        .output
        .windows(clear.len())
        .position(|w| w == clear.as_slice())
        .expect("the flag overprint clear is emitted");
    let after_clear = &terminal.output[clear_at + clear.len()..];
    assert!(
        after_clear.starts_with(&repaint),
        "the over-long trailing-slot repaint follows the clear: {:?}",
        String::from_utf8_lossy(&after_clear[..repaint.len().min(after_clear.len())]),
    );
    assert!(
        after_clear[repaint.len()..].starts_with(super::super::wire::MORE_PROMPT),
        "More? redraws after the trailing-slot repaint",
    );
}

#[tokio::test]
async fn question_mark_at_more_shows_the_pause_help_and_redraws_the_page() {
    // ae_tierd_aquascan4.txt U2 (bare `?` keypress): `?` emits
    // the in-pager pause help (byte-exact, incl. the trailing
    // `~SP|`+FF marker) followed by a page redraw and More?
    // again. COSMETIC divergence: NextScan redraws exactly the
    // lines of the current page; the door redraws a drifted
    // window (its internal paging quirk — designs/NEXTSCAN.md
    // §1.5/§9).
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'?'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let lines = f_1_emitted_lines(&services);
    let mut expected = joined(&lines[..29]);
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(super::super::wire::PAUSE_HELP);
    expected.extend_from_slice(&joined(&lines[..29]));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn paged_listing_shows_the_post_end_more_and_held_n_then_q_exits() {
    // ae_tierd_aquascan3.txt S2 + :158-163: the More? appears
    // right after the footer even for a listing far below a
    // page; `n` (bare keypress) is held and the following `Q`
    // erases it — `n` … `\x08 \x08Quit` (U1, identical mid-list
    // and post-End).
    let services = services_with_demo_catalogue();
    let mut terminal = keyed_terminal(vec![key(b'n'), key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(2),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = joined(&[
        b"\x1b[0m".to_vec(),
        super::super::wire::LISTING_BANNER.to_vec(),
        Vec::new(),
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]);
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(b"n");
    expected.extend_from_slice(b"\x08 \x08");
    expected.extend_from_slice(b"Quit\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_a_transitions_between_dirs_through_the_post_end_more() {
    // ae_tierd_aquascan3.txt S8 (repr :673): every non-empty dir
    // gets its own footer + post-End More?; `Y` (bare keypress)
    // at a non-last dir clears, emits CRLF and the next Scanning
    // header; `Y` at the last dir clears straight into the exit
    // tail (ae_tierd_aquascan5.txt V1).
    let services = services_with_two_small_areas();
    let mut terminal = keyed_terminal(vec![key(b'Y'), key(b'Y')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::All,
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = joined(&[
        b"\x1b[0m".to_vec(),
        super::super::wire::LISTING_BANNER.to_vec(),
        Vec::new(),
        b"Scanning dir 1 from top... Ok!".to_vec(),
        Vec::new(),
    ]);
    expected.extend_from_slice(&joined(&area_lines(&services, 1)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(&joined(&[
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]));
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn f_a_with_an_empty_first_dir_runs_its_headers_back_to_back() {
    // ae_tierd_aquascan5.txt V1: the empty dir emits exactly its
    // Nothing-found line with the next dir's Scanning line
    // directly beneath — no blank, no More? between; `Y` is a
    // bare keypress.
    use crate::domain::files::area::FileArea;
    let services = {
        use crate::domain::bytes::Bytes;
        use crate::domain::files::file::{File, FileStatus};
        services_with(InMemoryFileRepository::new(
            vec![
                FileArea::new(1, 1, "Main".to_string()),
                FileArea::new(1, 2, "Uploads".to_string()),
            ],
            vec![(
                1,
                2,
                File::new(
                    "ONLY.LHA".to_string(),
                    Bytes::new(1_000),
                    FileStatus::Available,
                    Some(b'P'),
                    "Only file".to_string(),
                    SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
                ),
            )],
        ))
    };
    let mut terminal = keyed_terminal(vec![key(b'Y')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::All,
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = joined(&[
        b"\x1b[0m".to_vec(),
        super::super::wire::LISTING_BANNER.to_vec(),
        Vec::new(),
        b"Scanning dir 1 from top... Nothing found!".to_vec(),
        b"Scanning dir 2 from top... Ok!".to_vec(),
        Vec::new(),
    ]);
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(super::super::wire::MORE_PROMPT);
    expected.extend_from_slice(&more_clear());
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn paged_hold_listing_quits_cleanly_at_the_mid_list_more() {
    // Held files render through the same framed body as normal
    // listings (no live capture exists with held files — the
    // seed board held none — so this pins the unit-level
    // inference). A `Q` at the mid-list More? must end the
    // listing with the Quit echo and the exit tail and nothing
    // else: quitting must skip the post-End pause entirely.
    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};
    let held: Vec<(u32, u32, File)> = (0u64..30)
        .map(|i| {
            (
                1,
                1,
                File::new(
                    format!("HELD{i:02}.LHA"),
                    Bytes::new(1_000),
                    FileStatus::HeldForReview,
                    Some(b'P'),
                    format!("Held file {i}"),
                    SystemTime::from(time::macros::datetime!(2026-05-01 12:00 UTC))
                        + Duration::from_secs(86_400 * i),
                ),
            )
        })
        .collect();
    let services = services_with(InMemoryFileRepository::new(
        vec![FileArea::new(1, 1, "Main".to_string())],
        held,
    ));
    // `Q` at the mid-list More? is a bare keypress (D2b;
    // ae_tierd_aquascan3.txt:321).
    let mut terminal = keyed_terminal(vec![key(b'Q')]);
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Hold,
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let output = String::from_utf8_lossy(&terminal.output);
    assert!(
        output.contains("Scanning HOLD dir from top... Ok!"),
        "held files report Ok!: {output}",
    );
    assert!(
        output.contains("HELD00.LHA"),
        "held files render framed rows: {output}",
    );
    let mut quit_tail = b"Quit\r\n".to_vec();
    quit_tail.extend_from_slice(EXIT_TAIL);
    assert!(
        terminal.output.ends_with(&quit_tail),
        "Q ends the listing immediately — no post-End pause after a quit: {output}",
    );
    let more_count = terminal
        .output
        .windows(super::super::wire::MORE_PROMPT.len())
        .filter(|w| *w == super::super::wire::MORE_PROMPT)
        .count();
    assert_eq!(more_count, 1, "exactly one More? before the quit: {output}");
}

#[tokio::test]
async fn empty_dir_reports_nothing_found_with_no_footer() {
    // ae_tierd_aquascan.txt:515-527 (E2): the Nothing-found line
    // goes straight to the exit tail — no blank, no footer.
    let services = services_with(InMemoryFileRepository::new(
        vec![crate::domain::files::area::FileArea::new(
            1,
            1,
            "Main".to_string(),
        )],
        Vec::new(),
    ));
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Dir(1),
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(b"Scanning dir 1 from top... Nothing found!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn failing_repository_renders_like_an_empty_catalogue() {
    // The row-5 error policy (July 2026 review, item 18): a repository
    // backend failure must never take the listing down — it logs and
    // renders byte-for-byte what an empty catalogue renders (the
    // legacy wire for an unreadable DIR file is the empty listing).
    // Pinned by equivalence so the test needs no knowledge of which
    // internal path (highest-dir check, Nothing found!) fires.
    use crate::domain::files::repository::{FileRepository, FileRepositoryError};

    fn backend_failure() -> FileRepositoryError {
        FileRepositoryError::Backend {
            source: "backing store unavailable".into(),
        }
    }

    struct FailingFileRepository;
    impl FileRepository for FailingFileRepository {
        fn areas_in_conference(
            &self,
            _: u32,
        ) -> Result<Vec<crate::domain::files::area::FileArea>, FileRepositoryError> {
            Err(backend_failure())
        }
        fn find_in_area(
            &self,
            _: FileAreaRef,
        ) -> Result<Vec<crate::domain::files::file::File>, FileRepositoryError> {
            Err(backend_failure())
        }
        fn list_held(
            &self,
            _: u32,
        ) -> Result<Vec<crate::domain::files::file::File>, FileRepositoryError> {
            Err(backend_failure())
        }
        fn list_new_since(
            &self,
            _: FileAreaRef,
            _: SystemTime,
        ) -> Result<Vec<crate::domain::files::file::File>, FileRepositoryError> {
            Err(backend_failure())
        }
    }

    let span_arg = || FileListArg::Span {
        span: FileSpan::Dir(1),
        non_stop: false,
        reverse: false,
        quick: false,
        fr_banner: false,
    };

    let empty = services_with(InMemoryFileRepository::new(Vec::new(), Vec::new()));
    let mut empty_terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(&empty, &mut empty_terminal, span_arg()).await;

    let mut failing = test_services();
    failing.conferences = Arc::new(vec![conference(1)]);
    failing.file_repo = Arc::new(FailingFileRepository);
    let mut failing_terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(&failing, &mut failing_terminal, span_arg()).await;

    assert_eq!(
        failing_terminal.output, empty_terminal.output,
        "a backend failure renders exactly like an empty catalogue"
    );
}

#[tokio::test]
async fn hold_span_reports_nothing_found_when_no_files_are_held() {
    // ae_tierd_aquascan3.txt:675-687 (S9).
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::with_lines(Vec::new());
    run_file_list(
        &services,
        &mut terminal,
        FileListArg::Span {
            span: FileSpan::Hold,
            non_stop: false,
            reverse: false,
            quick: false,
            fr_banner: false,
        },
    )
    .await;
    let mut expected = listing_preamble();
    expected.extend_from_slice(b"Scanning HOLD dir from top... Nothing found!\r\n");
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(terminal.output, expected);
}

#[tokio::test]
async fn populated_hold_listing_permits_dense_numeric_selection() {
    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};

    // Human gate A: preserve compatibility by registering displayed
    // HOLD numbers. This is an uncaptured extrapolation and explicit
    // Allium status departure, not a live-board MATCH claim.
    let held = File::new(
        "HELD.LHA".to_string(),
        Bytes::new(1_000),
        FileStatus::HeldForReview,
        Some(b'P'),
        "Held file".to_string(),
        SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
    );
    let services = services_with(InMemoryFileRepository::new(
        vec![FileArea::new(1, 1, "Main".to_string())],
        vec![(1, 1, held)],
    ));
    let mut session = menu_session();
    let mut terminal = keyed_terminal(vec![
        key(b'L'),
        key(b'R'),
        key(b'1'),
        KeyRead::Key(KeyEvent::Enter),
        key(b'Q'),
    ]);
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.handle_file_list(
            &mut session,
            FileListArg::Span {
                span: FileSpan::Hold,
                non_stop: false,
                reverse: false,
                quick: false,
                fr_banner: false,
            },
        )
        .await
        .expect("populated HOLD listing");
    }

    assert!(
        session
            .flagged_files_mut()
            .contains(&crate::domain::files::flagged::FlaggedKey::new(
                1, "HELD.LHA",
            )),
        "the displayed HOLD File #1 remains selectable",
    );
}

#[tokio::test]
async fn directory_order_is_independent_from_reverse_row_order() {
    // The live AquaScan `FR A` capture proves these are distinct axes,
    // even though human gate A keeps the shipped FR/N-R constructors
    // paired as reverse rows + descending dirs.
    let services = services_with_two_small_areas();
    let areas = services.file_repo.areas_in_conference(1).expect("areas");
    let mut terminal = CaptureTerminal::default();
    let mut state = super::ScanState::new(true, 1);
    let mut flagged = FlaggedFiles::default();
    let mode = super::ScanMode {
        kind: super::ScanKind::Full { reverse: true },
        directory_order: super::DirectoryOrder::Forward,
        quick: false,
    };
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.run_span(&mut state, 1, FileSpan::All, &areas, &mut flagged, &mode)
            .await
            .expect("mixed ordering mode");
    }

    let output = String::from_utf8_lossy(&terminal.output);
    let dir1 = output.find("Reverse-scanning dir 1").expect("dir 1 header");
    let dir2 = output.find("Reverse-scanning dir 2").expect("dir 2 header");
    assert!(
        dir1 < dir2,
        "explicit Forward order must win over row reversal"
    );
}

#[tokio::test]
async fn run_span_full_mode_streams_the_dir_through_the_engine_api() {
    // Item 17's generalised engine API: `run_span` takes a `&ScanMode`
    // — `Full { reverse: false }` over dir 2 streams the same header +
    // body + exit tail F's span path emits after its preamble.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    let areas = services.file_repo.areas_in_conference(1).expect("areas");
    let mut state = super::ScanState::new(true, 1);
    let mode = super::ScanMode {
        kind: super::ScanKind::Full { reverse: false },
        directory_order: super::DirectoryOrder::Forward,
        quick: false,
    };
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.run_span(
            &mut state,
            1,
            FileSpan::Dir(2),
            &areas,
            session.flagged_files_mut(),
            &mode,
        )
        .await
        .expect("scan");
    }
    let mut expected = joined(&[b"Scanning dir 2 from top... Ok!".to_vec(), Vec::new()]);
    expected.extend_from_slice(&joined(&area_lines(&services, 2)));
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}

#[tokio::test]
async fn run_span_full_reverse_mode_lists_newest_first_through_the_engine_api() {
    // The reverse arm of `ScanKind::Full` — the FR/`N R` mode: the
    // reverse header and the dir's rows newest-first.
    let services = services_with_demo_catalogue();
    let mut terminal = CaptureTerminal::default();
    let mut session = menu_session();
    let areas = services.file_repo.areas_in_conference(1).expect("areas");
    let mut state = super::ScanState::new(true, 1);
    let mode = super::ScanMode {
        kind: super::ScanKind::Full { reverse: true },
        directory_order: super::DirectoryOrder::Reverse,
        quick: false,
    };
    {
        let mut flow = crate::app::menu_flow::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };
        flow.run_span(
            &mut state,
            1,
            FileSpan::Dir(2),
            &areas,
            session.flagged_files_mut(),
            &mode,
        )
        .await
        .expect("scan");
    }
    let mut trio = services
        .file_repo
        .find_in_area(FileAreaRef::new(1, 2))
        .expect("files");
    trio.reverse();
    let reversed: Vec<Vec<u8>> =
        super::super::wire::assemble_dir_lines(&trio, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
    let mut expected = joined(&[b"Reverse-scanning dir 2... Ok!".to_vec(), Vec::new()]);
    expected.extend_from_slice(&joined(&reversed));
    expected.extend_from_slice(EXIT_TAIL);
    assert_eq!(
        String::from_utf8_lossy(&terminal.output),
        String::from_utf8_lossy(&expected),
    );
}
