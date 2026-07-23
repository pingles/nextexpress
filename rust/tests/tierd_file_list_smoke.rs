//! Tier D `F` (`NextScan` file listings) in-process integration tests.
//!
//! Each scenario boots a [`TelnetListener`] in-process, signs in as
//! the seeded sysop, drives the `NextScan` lister over a real telnet
//! client, and asserts the captured wire bytes (parity target: the
//! `AquaScan` door with `NextScan` branding —
//! `comparison/evidence-tierD/live-observations.md`; cleanest capture
//! `comparison/transcripts/ae_tierd_aquascan3.txt`). The expected
//! literals are restated here independently of the production
//! constants on purpose: the smoke guards them against drift.

mod support;

use std::sync::Arc;
use std::time::Duration;

use nextexpress::adapters::in_memory_file_repository::InMemoryFileRepository;
use nextexpress::app::seed;
use nextexpress::app::services::SharedFileRepo;
use nextexpress::domain::conference::{Conference, MessageBase};

use tokio::net::TcpStream;

use support::{
    contains, drain_until, end_session_forced, read_idle, sign_in_seeded_sysop, write_key,
    write_line, TestRuntime,
};

/// The `NextScan` listing banner (branding per `designs/NEXTSCAN.md` §7).
const LISTING_BANNER: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m\r\n";

/// The `FR` (reverse) listing banner — `'fr ?'` label, dash run flexed
/// 40→39 to hold the 77-col frame (`ae_tierd_aquascan3.txt` S10/S11).
const LISTING_BANNER_REVERSE: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]---------------------------------------[ \x1b[36m'fr ?' for options \x1b[34m]--\x1b[0m\r\n";

/// The `More?` pager prompt (`ae_tierd_aquascan3.txt:158`).
const MORE_PROMPT: &[u8] =
    b"\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m ";

/// The genuine internal `checkFlagged()` + `yesNo(2)` leave confirm
/// (`amiexpress/express.e:12670`, `:2134`). Server bytes, live-captured
/// (`comparison/transcripts/ae_tierd_g_confirm.txt:146`).
const LEAVE_FLAGGED_CONFIRM: &[u8] =
    b"\r\nYou have flagged files still not downloaded.\r\nDo you leave without them? \x1b[32m(\x1b[33my\x1b[32m/\x1b[33mN\x1b[32m)\x1b[32m?\x1b[0m ";

/// `saveFlagged`'s autosave banner + BEL, emitted on logoff with a
/// non-empty flag set (`amiexpress/express.e:2803`). Live-captured
/// (`comparison/transcripts/ae_tierd_g_confirm.txt:177`).
const AUTOSAVING_FILE_FLAGS: &[u8] = b"\r\n** AutoSaving File Flags **\r\n\x07\r\n";

/// `loadFlagged`'s logon restore banner (`amiexpress/express.e:2792`),
/// live-captured at login (`ae_tierd_alterflags.txt:77-81`).
const FLAGGED_FILES_EXIST: &[u8] = b"\r\n** Flagged File(s) Exist **\r\n\x07\r\n";

/// `flagFiles`'s main prompt (`amiexpress/express.e:12601`), live-captured
/// (`comparison/transcripts/ae_tierd_alterflags.txt:114`): the `A`-loop
/// `Filename(s) to flag: (F)rom, (C)lear, (Enter)=none? ` line (slice D6b).
const FLAG_PROMPT: &[u8] =
    b"\x1b[36mFilename(s) to flag: \x1b[32m(\x1b[33mF\x1b[32m)\x1b[36mrom, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none\x1b[0m? ";

/// `flagFiles`'s clear sub-prompt (`amiexpress/express.e:12614`),
/// live-captured (`comparison/transcripts/ae_tierd_alterflags.txt:122`):
/// `Filename(s) to Clear: (*)All, (Enter)=none? ` (slice D6b).
const CLEAR_PROMPT: &[u8] =
    b"\x1b[36mFilename(s) to Clear: \x1b[32m(\x1b[33m*\x1b[32m)\x1b[36mAll, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none\x1b[0m? ";

/// The shared tail of both flag prompts (`=none? ` with the reset),
/// used to drain the connection up to a completed prompt.
const PROMPT_TAIL: &[u8] = b"=none\x1b[0m? ";

#[tokio::test]
async fn f_1_pages_the_seeded_corpus_and_q_quits() {
    // ae_tierd_aquascan3.txt S4: banner, scan header, framed rows;
    // the first More? lands exactly after the 02-03-26 separator
    // block (the captured 29-line page); Q echoes Quit and exits
    // through the two-reset tail.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 1").await;
    let page = drain_until(&mut stream, MORE_PROMPT).await;

    let mut expected_head = b"F 1\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(LISTING_BANNER);
    expected_head.extend_from_slice(b"\r\nScanning dir 1 from top... Ok!\r\n\r\n");
    assert!(
        page.starts_with(&expected_head),
        "F 1 must open with the NextScan preamble, got {:?}",
        String::from_utf8_lossy(&page[..expected_head.len().min(page.len())]),
    );
    assert!(
        contains(
            &page,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m    P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the\r\n",
        ),
        "first framed row missing: {:?}",
        String::from_utf8_lossy(&page),
    );
    // The captured page-1 boundary: the 02-03-26 separator block's
    // closing blank, then More? — mid-frame, before File #4's header.
    let mut expected_tail = b" 02-03-26\r\n\x1b[0m\r\n".to_vec();
    expected_tail.extend_from_slice(MORE_PROMPT);
    assert!(
        page.ends_with(&expected_tail),
        "page 1 must pause after the 02-03-26 separator block, got tail {:?}",
        String::from_utf8_lossy(&page[page.len().saturating_sub(120)..]),
    );

    // D2b re-pin: `Q` is a single bare keypress, no Enter
    // (ae_tierd_aquascan3.txt:321 — the capture harness sent the
    // lone byte); the echoed Quit and tail bytes are unchanged.
    write_key(&mut stream, b"Q").await;
    let quit = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        quit.starts_with(b"Quit\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "Q must echo Quit into the two-reset tail, got {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn f_2_butt_joins_same_date_files_and_post_end_n_is_erased_by_q() {
    // ae_tierd_aquascan3.txt S2 + :158-163: the same-date pair
    // butt-joins (no separator), the footer is followed by the
    // post-End More?, and a held `n` is erased by the next verb.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 2").await;
    let listing = drain_until(&mut stream, MORE_PROMPT).await;
    assert!(
        contains(
            &listing,
            b"Greets to everyone on node 1.\r\n\x1b[0m\x1b[34m[\x1b[0m File #3 ",
        ),
        "same-date TOOLPACK must butt-join after MYDEMO's continuation: {:?}",
        String::from_utf8_lossy(&listing),
    );
    let mut footer_then_more =
        b"\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m\r\n".to_vec();
    footer_then_more.extend_from_slice(MORE_PROMPT);
    assert!(
        listing.ends_with(&footer_then_more),
        "the post-End More? must follow the footer directly: {:?}",
        String::from_utf8_lossy(&listing[listing.len().saturating_sub(160)..]),
    );

    // D2b re-pin: bare keys, no terminators (ae_tierd_aquascan4.txt
    // U1 :133 — `n` echoes on its own keypress and holds; a
    // terminated `n\r\n` would now quit via probe P1 instead).
    write_key(&mut stream, b"n").await;
    let held = drain_until(&mut stream, b"n").await;
    assert!(
        held.ends_with(b"n"),
        "lone n echoes and holds, got {:?}",
        String::from_utf8_lossy(&held),
    );
    write_key(&mut stream, b"Q").await;
    let quit = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        quit.starts_with(b"\x08 \x08Quit\r\n"),
        "the next verb erases the held n before running: {:?}",
        String::from_utf8_lossy(&quit[..quit.len().min(40)]),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn bare_f_opens_the_directories_prompt_and_enter_aborts() {
    // ae_tierd_aquascan3.txt S3: the door's own Directories prompt
    // with the live (1-2) range; Enter aborts silently with a single
    // reset.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F").await;
    let prompt = drain_until(&mut stream, b"=None ?\x1b[0m ").await;
    assert!(
        contains(
            &prompt,
            b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, ",
        ),
        "the Directories prompt must flex to (1-2): {:?}",
        String::from_utf8_lossy(&prompt),
    );
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn fr_1_opens_the_reverse_banner_and_header() {
    // ae_tierd_aquascan3.txt S10: `FR 1` opens with the reverse banner
    // (`'fr ?'`, dash run flexed 40→39) and the `Reverse-scanning dir
    // 1... Ok!` header (no "from top"), then pages the dir-1 rows.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"FR 1").await;
    let page = drain_until(&mut stream, MORE_PROMPT).await;
    let mut expected_head = b"FR 1\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(LISTING_BANNER_REVERSE);
    expected_head.extend_from_slice(b"\r\nReverse-scanning dir 1... Ok!\r\n\r\n");
    assert!(
        page.starts_with(&expected_head),
        "FR 1 must open with the reverse NextScan preamble, got {:?}",
        String::from_utf8_lossy(&page[..expected_head.len().min(page.len())]),
    );

    write_key(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn bare_fr_opens_the_directories_prompt_with_the_reverse_banner() {
    // express.e:27645-27648 (`getDirSpan('')`): bare `FR`, like bare
    // `F`, opens the Directories prompt — but with the reverse banner
    // (`'fr ?'`). We follow the original here over the AquaScan capture
    // (S11), which skips the prompt for `FR`. Answering `2` then
    // reverse-scans dir 2.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"FR").await;
    let prompt = drain_until(&mut stream, b"=None ?\x1b[0m ").await;
    let mut expected_head = b"FR\r\n\x1b[0m\r\n".to_vec();
    expected_head.extend_from_slice(LISTING_BANNER_REVERSE);
    expected_head.extend_from_slice(b"\r\n");
    assert!(
        prompt.starts_with(&expected_head),
        "bare FR must open with the reverse banner, got {:?}",
        String::from_utf8_lossy(&prompt[..expected_head.len().min(prompt.len())]),
    );
    assert!(
        contains(
            &prompt,
            b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, ",
        ),
        "bare FR must open the Directories (1-2) prompt: {:?}",
        String::from_utf8_lossy(&prompt),
    );

    write_line(&mut stream, b"2").await;
    let listing = drain_until(&mut stream, MORE_PROMPT).await;
    assert!(
        contains(&listing, b"Reverse-scanning dir 2... Ok!\r\n"),
        "answering 2 must reverse-scan dir 2: {:?}",
        String::from_utf8_lossy(&listing),
    );

    write_key(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn f_99_reports_the_highest_directory() {
    // ae_tierd_aquascan.txt A7 (:330-342), max flexed to 2.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 99").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"The highest directory number is 2!\r\n"),
        "F 99 must report the highest dir: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn f_h_reports_nothing_held() {
    // ae_tierd_aquascan3.txt S9 (:675-687).
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F H").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning HOLD dir from top... Nothing found!\r\n"),
        "F H must report the empty hold dir: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn f_in_an_unseeded_conference_reports_nothing_found() {
    // The demo catalogue seeds only the landing conference; other
    // conferences carry one empty area (ae_tierd_aquascan.txt E2's
    // Nothing-found shape).
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"J 2").await;
    drain_until(&mut stream, b"mins. left): ").await;
    write_line(&mut stream, b"F 1").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning dir 1 from top... Nothing found!\r\n"),
        "the unseeded conference must list nothing: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn utf8_gate_every_session_byte_decodes() {
    // Encoding policy (AGENTS.md): the wire is valid UTF-8. Drive the
    // listing body (wave art) and the F ? help (©) and assert the
    // entire received stream decodes. The More?/flag prompt constants
    // are pinned in wire.rs unit tests and join this gate once the
    // hotkey pager lands; the login banner is gated by its own slice.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;
    let mut all = Vec::new();
    // sign_in_seeded_sysop already drains through "mins. left): ";
    // the F surfaces below are what this gate owns.
    write_line(&mut stream, b"F ?").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"F A NS").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    // Hostile-client input: a raw Latin-1 high byte (0xA9 `©`) typed at
    // the prompt must be echoed as valid UTF-8 (0xC2 0xA9), not a lone
    // 0xA9. Before the inbound codec fix this put invalid UTF-8 on the
    // wire pre-nothing — the server-output-only gate could not see it.
    write_line(&mut stream, b"\xa9").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    assert!(
        std::str::from_utf8(&all).is_ok(),
        "session stream contains non-UTF-8 bytes: {:?}",
        String::from_utf8_lossy(&all)
    );
    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn hotkey_n_echoes_on_keypress_and_enter_quits() {
    // Char-mode proof: at More?, a lone `n` echoes on its own
    // keypress — before any Enter — and a following bare CR runs the
    // probe-P1 quit (held-n + Enter, ae_tierd_probes.txt:100-138). The
    // CR echoes \r\n then the two-reset exit tail; the Quit *word*
    // never appears (held n leaves the pager via Enter, not via Q).
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, MORE_PROMPT).await;

    write_key(&mut stream, b"n").await;
    let echoed = read_idle(&mut stream, Duration::from_millis(300)).await;
    assert_eq!(
        echoed,
        b"n",
        "n must echo on the keypress itself, before any Enter, got {:?}",
        String::from_utf8_lossy(&echoed),
    );

    write_key(&mut stream, b"\r").await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "CR must echo \\r\\n then the exit tail, got {:?}",
        String::from_utf8_lossy(&after),
    );
    assert!(
        !contains(&after, b"Quit"),
        "held-n + Enter must NOT echo the Quit word, got {:?}",
        String::from_utf8_lossy(&after),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn hotkey_q_acts_on_a_single_keypress_without_enter() {
    // Char-mode proof: `Q` quits the pager on one bare keypress, no
    // terminator (ae_tierd_aquascan3.txt:321). The pre-D2b line-read
    // pager would have waited for Enter and never seen this.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, MORE_PROMPT).await;

    write_key(&mut stream, b"Q").await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"Quit\r\n"),
        "Q must quit with no terminator, got {:?}",
        String::from_utf8_lossy(&after),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn hotkey_flag_entry_echoes_each_typed_byte() {
    // Char-mode proof: `F` opens the flag-by-name prompt, and each
    // typed byte of the entry echoes as it arrives (probe P3). The
    // entry is discarded in D2b; Enter overprints and More? redraws.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, MORE_PROMPT).await;

    write_key(&mut stream, b"F").await;
    drain_until(&mut stream, b"to flag:\x1b[0m ").await;

    for &byte in b"TERMV48.LHA" {
        write_key(&mut stream, &[byte]).await;
        let echoed = read_idle(&mut stream, Duration::from_millis(300)).await;
        assert_eq!(
            echoed,
            vec![byte],
            "each flag byte must echo as typed; byte {:?} got {:?}",
            byte as char,
            String::from_utf8_lossy(&echoed),
        );
    }

    write_key(&mut stream, b"\r").await;
    drain_until(&mut stream, MORE_PROMPT).await;

    write_key(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn plain_g_with_a_flagged_file_confirms_then_n_stays_and_y_leaves() {
    // End-to-end (slice D5/Ga). Reference:
    // `comparison/transcripts/ae_tierd_g_confirm.txt`. Flag a file via
    // the D2f `F` verb, then plain `G` runs the genuine internal
    // checkFlagged() confirm over the real telnet single-key adapter:
    // `N` keeps the caller in the menu; a second `G` + `Y` leaves.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Flag ANSIPACK.LHA (dir-1 #1, page 1) through the More? `F` verb.
    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, MORE_PROMPT).await;
    write_key(&mut stream, b"F").await;
    drain_until(&mut stream, b"to flag:\x1b[0m ").await;
    for &byte in b"ANSIPACK.LHA" {
        write_key(&mut stream, &[byte]).await;
    }
    write_key(&mut stream, b"\r").await;
    drain_until(&mut stream, MORE_PROMPT).await;
    write_key(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // Plain `G` must print the confirm and NOT log off.
    write_line(&mut stream, b"G").await;
    let confirm = drain_until(&mut stream, LEAVE_FLAGGED_CONFIRM).await;
    assert!(
        contains(&confirm, LEAVE_FLAGGED_CONFIRM),
        "plain G with a flagged file must print the leave confirm, got {:?}",
        String::from_utf8_lossy(&confirm),
    );

    // `N` echoes `No` and returns to the menu prompt (stay).
    write_key(&mut stream, b"N").await;
    let after_n = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_n, b"No\r\n"),
        "N must echo No and return to the menu, got {:?}",
        String::from_utf8_lossy(&after_n),
    );

    // A second `G` + `Y` echoes `Yes` and logs off.
    write_line(&mut stream, b"G").await;
    drain_until(&mut stream, LEAVE_FLAGGED_CONFIRM).await;
    write_key(&mut stream, b"Y").await;
    let after_y = drain_until(&mut stream, b"Goodbye").await;
    assert!(
        contains(&after_y, b"Yes\r\n"),
        "Y must echo Yes before logging off, got {:?}",
        String::from_utf8_lossy(&after_y),
    );
    // saveFlagged's autosave banner + BEL lands between the `Yes` echo
    // and the goodbye tail (ae_tierd_g_confirm.txt:177).
    assert!(
        contains(&after_y, AUTOSAVING_FILE_FLAGS),
        "leaving with a flagged file must emit the AutoSaving banner before goodbye, got {:?}",
        String::from_utf8_lossy(&after_y),
    );
}

#[tokio::test]
async fn a_lists_the_session_flag_set_over_telnet() {
    // Slice D6a/D6b. `A` runs the genuine internal alterFlags -> showFlags
    // (express.e:24601 -> :12486 -> :12598). Reference:
    // `comparison/transcripts/ae_tierd_alterflags.txt` — each `A` shows
    // the listing (empty `No file flags`, else upper-cased space-joined
    // names) framed by `\r\n`, then the `Filename(s) to flag:` prompt
    // (slice D6b). `<CR>` (=none) returns to the menu.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Nothing flagged yet: `A` lists `No file flags`, then the prompt.
    write_line(&mut stream, b"A").await;
    let empty = drain_until(&mut stream, PROMPT_TAIL).await;
    assert!(
        contains(&empty, b"\r\nNo file flags\r\n") && contains(&empty, FLAG_PROMPT),
        "bare A with no flags must list `No file flags` then the flag prompt, got {:?}",
        String::from_utf8_lossy(&empty),
    );
    // `<CR>` (=none) ends the loop, back to the menu.
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // Flag ANSIPACK.LHA (dir-1 #1) through the More? `F` verb.
    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, MORE_PROMPT).await;
    write_key(&mut stream, b"F").await;
    drain_until(&mut stream, b"to flag:\x1b[0m ").await;
    for &byte in b"ANSIPACK.LHA" {
        write_key(&mut stream, &[byte]).await;
    }
    write_key(&mut stream, b"\r").await;
    drain_until(&mut stream, MORE_PROMPT).await;
    write_key(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // `A` now lists the flagged name, then the prompt; `<CR>` exits.
    write_line(&mut stream, b"A").await;
    let listed = drain_until(&mut stream, PROMPT_TAIL).await;
    assert!(
        contains(&listed, b"\r\nANSIPACK.LHA\r\n") && contains(&listed, FLAG_PROMPT),
        "A must list the flagged name then the flag prompt, got {:?}",
        String::from_utf8_lossy(&listed),
    );
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn a_flag_prompt_loop_flags_a_name_then_clears_over_telnet() {
    // Slice D6b. The `A` flagFiles loop (express.e:12594) driven over the
    // compiled listener against `ae_tierd_alterflags.txt`: type a name to
    // flag it (addFlagToList does NOT check the file exists, :12555), see
    // it listed, then `C` -> `*` clears the set and re-prompts, and `<CR>`
    // returns to the menu.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // `A` -> empty listing + the flag prompt.
    write_line(&mut stream, b"A").await;
    drain_until(&mut stream, PROMPT_TAIL).await;

    // Type a name: it is flagged (upper-cased) and the loop returns to
    // the menu with no trailing blank line (RESULT_FAILURE exit, :12642).
    write_line(&mut stream, b"mydemo.dms").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // `A` again -> the flagged name is listed before the prompt.
    write_line(&mut stream, b"A").await;
    let listed = drain_until(&mut stream, PROMPT_TAIL).await;
    assert!(
        contains(&listed, b"\r\nMYDEMO.DMS\r\n"),
        "the typed name lists upper-cased, got {:?}",
        String::from_utf8_lossy(&listed),
    );

    // `C` opens the clear sub-prompt; `*` clears all and re-shows the
    // empty listing + the flag prompt.
    write_line(&mut stream, b"C").await;
    let clear = drain_until(&mut stream, PROMPT_TAIL).await;
    assert!(
        contains(&clear, CLEAR_PROMPT),
        "bare C must open the clear sub-prompt, got {:?}",
        String::from_utf8_lossy(&clear),
    );
    write_line(&mut stream, b"*").await;
    let cleared = drain_until(&mut stream, PROMPT_TAIL).await;
    assert!(
        contains(&cleared, b"\r\nNo file flags\r\n"),
        "`*` must clear the set and re-show `No file flags`, got {:?}",
        String::from_utf8_lossy(&cleared),
    );

    // `<CR>` (=none) returns to the menu; the set is already empty so the
    // logoff teardown takes the plain path.
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b"mins. left): ").await;

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn flags_persist_across_logoff_and_logon_over_telnet() {
    // Slice D5-persist: flag a name, log off (saveFlagged), then sign in
    // again as the same sysop on the same listener (shared in-memory
    // store) and see the `** Flagged File(s) Exist **` banner before the
    // menu, with `A` listing the restored name.
    let addr = spawn_listener_with_demo_files().await;

    // --- Session 1: flag MYDEMO.DMS via the A loop, then log off ---
    let mut s1 = sign_in_seeded_sysop(&addr).await;
    write_line(&mut s1, b"A").await;
    drain_until(&mut s1, PROMPT_TAIL).await;
    write_line(&mut s1, b"mydemo.dms").await; // flags + returns to menu
    drain_until(&mut s1, b"mins. left): ").await;
    write_line(&mut s1, b"G Y").await;
    drain_until(&mut s1, b"Goodbye").await;
    drop(s1);

    // --- Session 2: same user, same listener -> restored + banner ---
    let mut s2 = TcpStream::connect(addr).await.expect("reconnect");
    // Drive the login by hand so we can scan the whole logon stream for
    // the banner (sign_in_seeded_sysop drains past it to the menu).
    let login = drive_login_capturing(&mut s2).await;
    assert!(
        contains(&login, FLAGGED_FILES_EXIST),
        "the restored non-empty set announces at logon, got {:?}",
        String::from_utf8_lossy(&login),
    );

    write_line(&mut s2, b"A").await;
    let listed = drain_until(&mut s2, PROMPT_TAIL).await;
    assert!(
        contains(&listed, b"\r\nMYDEMO.DMS\r\n"),
        "A lists the restored flag, got {:?}",
        String::from_utf8_lossy(&listed),
    );
    // Clear so teardown is clean, then log off.
    write_line(&mut s2, b"C").await;
    drain_until(&mut s2, PROMPT_TAIL).await;
    write_line(&mut s2, b"*").await;
    drain_until(&mut s2, PROMPT_TAIL).await;
    write_line(&mut s2, b"").await;
    drain_until(&mut s2, b"mins. left): ").await;
    end_session_forced(&mut s2).await;
}

/// Signs in `sysop`/`sysop` and returns the full byte stream from the
/// graphics prompt through to the menu prompt (so a caller can scan the
/// logon banners). Mirrors `sign_in_seeded_sysop` but captures and
/// returns all bytes received up to the menu prompt.
async fn drive_login_capturing(stream: &mut TcpStream) -> Vec<u8> {
    let mut all = Vec::new();
    all.extend(drain_until(stream, b"ANSI Graphics (Y/n)? ").await);
    write_line(stream, b"Y").await;
    all.extend(drain_until(stream, b"Enter your Name: ").await);
    write_line(stream, b"sysop").await;
    all.extend(drain_until(stream, b"PassWord: ").await);
    write_line(stream, b"sysop").await;
    all.extend(drain_until(stream, b"mins. left): ").await);
    all
}

/// Boots an in-process listener whose file catalogue is the seeded
/// demo corpus (landing conference 1: areas 1-2; conference 2: one
/// empty area) — the same wiring `bootstrap::run` performs.
async fn spawn_listener_with_demo_files() -> std::net::SocketAddr {
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
    support::spawn_seeded_sysop(TestRuntime::new(
        std::env::temp_dir(),
        conferences,
        support::empty_mail_stores(),
        file_repo,
    ))
    .await
}

#[tokio::test]
async fn zippy_inline_search_dumps_the_matching_block_over_telnet() {
    // Slice D4 (`Z`): the internal zippy search, reachable from the
    // compiled listener. `Z <token>` opens the internal getDirSpan
    // Directories prompt (lowercase `=none?`, no trailing space — the
    // genuine-internal form, distinct from AquaScan's `=None ?`), and a
    // dir answer dumps the matching file's raw DIR block.
    // (comparison/transcripts/ae_tierd_zippy.txt Z1/Z2.)
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"Z STARVIEW").await;
    let prompt = drain_until(&mut stream, b"=none? \x1b[0m").await;
    assert!(
        contains(
            &prompt,
            b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, ",
        ),
        "Z must open the internal Directories (1-2) prompt: {:?}",
        String::from_utf8_lossy(&prompt),
    );

    write_line(&mut stream, b"1").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning directory 1\r\n"),
        "the chosen dir's scan header must appear: {:?}",
        String::from_utf8_lossy(&out),
    );
    assert!(
        contains(
            &out,
            b"STARVIEW.LHA P 198765  05-28-26  StarView 2.4 - astronomy program\r\n",
        ),
        "the matching row must be dumped raw (no NextScan frames): {:?}",
        String::from_utf8_lossy(&out),
    );
    assert!(
        contains(
            &out,
            b"                                 Plots 9000 stars, needs FPU.\r\n",
        ),
        "the whole block (continuation line) must be dumped: {:?}",
        String::from_utf8_lossy(&out),
    );
    assert!(
        std::str::from_utf8(&out).is_ok(),
        "the zippy wire must be valid UTF-8: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn bare_zippy_prompts_for_the_search_string_over_telnet() {
    // Slice D4: bare `Z` first prompts `Enter string to search for: `
    // (ae_tierd_zippy.txt Z1), then runs the getDirSpan prompt and the
    // scan. PROTRACKER matches PTREPLAY's description in dir 1.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"Z").await;
    drain_until(&mut stream, b"Enter string to search for: ").await;
    write_line(&mut stream, b"PROTRACKER").await;
    drain_until(&mut stream, b"=none? \x1b[0m").await;
    write_line(&mut stream, b"1").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Protracker replay routine, asm source\r\n"),
        "PROTRACKER must match PTREPLAY's description: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}

#[tokio::test]
async fn zippy_inline_directory_scans_immediately_without_a_prompt_over_telnet() {
    // Slice D7: `Z <term> <dir>` resolves the directory inline and scans
    // immediately — NO `Directories:` prompt (the bug a user hit when
    // D4 ignored the inline dir). Pinned to ae_tierd_zippy3.txt.
    let addr = spawn_listener_with_demo_files().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"Z ART 1").await;
    let out = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&out, b"Scanning directory 1\r\n"),
        "Z ART 1 must scan dir 1 immediately: {:?}",
        String::from_utf8_lossy(&out),
    );
    assert!(
        contains(
            &out,
            b"ANSIPACK.LHA P 234567  01-15-26  Collection of 40 ANSI screens from the\r\n",
        ),
        "the inline scan must dump the matching row: {:?}",
        String::from_utf8_lossy(&out),
    );
    assert!(
        !contains(&out, b"=none?"),
        "the inline dir form must NOT show the Directories prompt: {:?}",
        String::from_utf8_lossy(&out),
    );

    end_session_forced(&mut stream).await;
}
