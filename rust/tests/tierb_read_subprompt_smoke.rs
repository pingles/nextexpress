//! Tier B (Slice B4) in-process smoke: the `R` read sub-prompt
//! scaffolding.
//!
//! Boots a [`TelnetListener`] in-process (the `tierb_mail_scan_smoke.rs`
//! shape) with one conference ("One") whose message base carries two
//! public messages addressed to the seeded sysop. After signing in the
//! sysop types `R 1`:
//!
//!   * message 1 is displayed (legacy header block), then the legacy
//!     `readMSG` sub-prompt appears with the runtime range `1+2`
//!     (`amiexpress/express.e:12016-12021`);
//!   * pressing `<CR>` (an empty line) advances to message 2, which is
//!     displayed and followed by the sub-prompt re-rendered at range
//!     `2+2`;
//!   * pressing `Q` returns to the main conference menu prompt.
//!
//! This pins the verbatim sub-prompt wire bytes and the `<CR>`-advance /
//! `Q`-quit navigation that Slice B4 introduces. Options other than
//! `<CR>` / `Q` (A/F/R/L/D/M/EH/?/??) land in B5 and are not exercised
//! here.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use nextexpress::adapters::file_mail_store::FileMailStore;
use nextexpress::adapters::in_memory_caller_log::InMemoryCallerLog;
use nextexpress::adapters::in_memory_mail_stores::InMemoryMailStores;
use nextexpress::adapters::in_memory_user_repository::InMemoryUserRepository;
use nextexpress::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use nextexpress::adapters::telnet_listener::TelnetListener;
use nextexpress::app::config::Config;
use nextexpress::app::mail_stores::MailStores;
use nextexpress::app::seed;
use nextexpress::app::services::{
    SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedUserRepo,
};
use nextexpress::bootstrap;
use nextexpress::domain::caller_log::CallerLogAppender;
use nextexpress::domain::conference::{
    Conference, ConferenceMembership, MessageBase, MessageBaseRef, ScanFlag,
};
use nextexpress::domain::password::{PasswordHashKind, PasswordHasher};
use nextexpress::domain::user::User;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

#[tokio::test]
async fn r_enters_sub_prompt_then_cr_advances_then_q_quits_over_telnet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // `R 1` displays message 1, then the sub-prompt opens at range `2+2`
    // — the lower bound is the NEXT message to read (legacy increments
    // `msgNum` after `displayMessage`, `express.e:12372`).
    write_line(&mut stream, b"R 1").await;
    let first = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&first, b"First Subject"),
        "R 1 must display message 1, got {:?}",
        String::from_utf8_lossy(&first)
    );
    assert!(
        contains(&first, &sub_prompt(b"2+2", true, true)),
        "missing the legacy sub-prompt at range 2+2, got {:?}",
        String::from_utf8_lossy(&first)
    );

    // `<CR>` reads the next message (2), which is displayed; the pointer
    // then advances past the last message so the prompt collapses to the
    // `( QUIT )` form (`express.e:12012`).
    write_line(&mut stream, b"").await;
    let second = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&second, b"Second Subject"),
        "<CR> must advance to and display message 2, got {:?}",
        String::from_utf8_lossy(&second)
    );
    assert!(
        contains(&second, &sub_prompt(b"QUIT", true, true)),
        "missing the QUIT sub-prompt after the last message, got {:?}",
        String::from_utf8_lossy(&second)
    );

    // `Q` returns to the main conference menu prompt.
    write_line(&mut stream, b"Q").await;
    let back = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&back, b"[\x1b[36m1\x1b[34m:\x1b[36mOne - general\x1b[0m]"),
        "Q must return to the conference 1 menu prompt, got {:?}",
        String::from_utf8_lossy(&back)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn cr_past_the_last_message_returns_to_the_menu() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Read the last message (2 of 2): the pointer advances to 3, past
    // the highest, so the sub-prompt opens at the `( QUIT )` form
    // (`express.e:12012`).
    write_line(&mut stream, b"R 2").await;
    let at_last = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&at_last, &sub_prompt(b"QUIT", true, true)),
        "expected the QUIT sub-prompt at the last message, got {:?}",
        String::from_utf8_lossy(&at_last)
    );

    // `<CR>` at the QUIT prompt returns silently to the menu — the
    // legacy implicit-advance path sets `noDirF = 1`, so `noMorePlus`
    // prints nothing (`express.e:12082`/`:12302`). It must NOT probe a
    // non-existent message 3.
    write_line(&mut stream, b"").await;
    let back = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&back, b"Message not found"),
        "advancing past the last message must not probe a missing message, got {:?}",
        String::from_utf8_lossy(&back)
    );
    assert!(
        contains(&back, b"[\x1b[36m1\x1b[34m:\x1b[36mOne - general\x1b[0m]"),
        "expected return to the conference 1 menu prompt, got {:?}",
        String::from_utf8_lossy(&back)
    );

    end_session(&mut stream).await;
}

/// Seeds a two-message file mail base: two unread public messages
/// addressed to the seeded sysop (slot 1), numbered 1 and 2.
fn seed_two_message_base(msgbase: &std::path::Path) {
    std::fs::create_dir_all(msgbase).expect("create msgbase");
    std::fs::write(
        msgbase.join("0000001.json"),
        seeded_mail_json(1, "Carol", "First Subject", "First message body."),
    )
    .expect("seed message 1");
    std::fs::write(
        msgbase.join("0000002.json"),
        seeded_mail_json(2, "Dave", "Second Subject", "Second message body."),
    )
    .expect("seed message 2");
}

/// Tier B B10: bare `R` (no message number) opens the read sub-prompt
/// PROMPT-FIRST at the caller's resume point instead of emitting the
/// usage error. With a fresh read-pointer (nothing read yet) the resume
/// point is the base's lowest message, so the prompt opens at range
/// `1+2` WITHOUT displaying a message; the first `<CR>` then reads
/// message 1 (legacy `msgNum := lastMsgReadConf + 1`,
/// `amiexpress/express.e:11984-12021`).
#[tokio::test]
async fn bare_r_with_a_fresh_pointer_opens_at_the_lowest_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R").await;
    let out = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&out, b"Usage: R <message-number>"),
        "bare R must not emit the usage error, got {:?}",
        String::from_utf8_lossy(&out)
    );
    // Prompt-first: the entry blank line + the prompt's own CRLF give the
    // legacy three-CRLF prefix, and no message body precedes the prompt.
    assert!(
        contains(&out, b"R\r\n\r\n\r\n\x1b[32mMsg. Options:"),
        "bare R must render the prompt first (3-CRLF prefix), got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        contains(&out, &sub_prompt(b"1+2", true, true)),
        "bare R must show the sub-prompt at range 1+2, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !contains(&out, b"First Subject"),
        "bare R must NOT display a message before the prompt, got {:?}",
        String::from_utf8_lossy(&out)
    );

    // The first `<CR>` reads message 1 and re-prompts at `2+2`.
    write_line(&mut stream, b"").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"First Subject") && contains(&after, &sub_prompt(b"2+2", true, true)),
        "the first <CR> must display message 1 and re-prompt at 2+2, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

/// Tier B B10: bare `R` resumes from the read-pointer, not the lowest
/// message. After reading message 1 the resume pointer sits at 1, so a
/// later bare `R` opens at message 2 (range `2+2`) and does not replay
/// message 1. The start is `last_read + 1` (`amiexpress/express.e:11984`),
/// the sequential read-resume pointer.
#[tokio::test]
async fn bare_r_resumes_from_the_read_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Read message 1 (advances the read-pointer to 1), then return to
    // the menu.
    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;
    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // Bare `R` now resumes at range `2+2` (next = last_read + 1 = 2),
    // prompt-first and WITHOUT replaying message 1.
    write_line(&mut stream, b"R").await;
    let out = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&out, &sub_prompt(b"2+2", true, true)),
        "bare R must resume at range 2+2, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !contains(&out, b"First Subject") && !contains(&out, b"Second Subject"),
        "bare R must not display (or replay) a message before the prompt, got {:?}",
        String::from_utf8_lossy(&out)
    );

    // The resume `<CR>` reads message 2, not message 1.
    write_line(&mut stream, b"").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Second Subject") && !contains(&after, b"First Subject"),
        "the resume <CR> must display message 2, not replay 1, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

/// Tier B B10: once every message has been read the resume pointer is
/// past the highest message, so bare `R` opens the prompt-first
/// sub-prompt at the `( QUIT )` range (`express.e:12012`) — it must NOT
/// wrap back to message 1, nor leak a `Message not found.` probe. `Q`
/// then returns silently to the menu. This also pins the read-pointer
/// seam: routing through `scan_mail::first_unread_number_for` would
/// return `None` here (no unread mail addressed to the reader) and a
/// lowest-key fallback would wrongly replay message 1.
#[tokio::test]
async fn bare_r_with_an_exhausted_pointer_returns_to_the_menu_without_replaying() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Read both messages so the pointer is exhausted (last_read = 2):
    // `R 1` displays message 1, `<CR>` reads message 2, `Q` to the menu.
    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;
    write_line(&mut stream, b"").await;
    drain_until(&mut stream, b">: ").await;
    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // Bare `R` with the pointer exhausted (start = 3 > highest 2) opens
    // the prompt-first sub-prompt at the `( QUIT )` range. With no current
    // message, D/M are hidden (per-message gating). It must NOT replay a
    // message nor leak the `Message not found.` probe.
    write_line(&mut stream, b"R").await;
    let out = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&out, &sub_prompt(b"QUIT", false, false)),
        "exhausted bare R must show the QUIT prompt (D/M hidden), got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !contains(&out, b"First Subject") && !contains(&out, b"Second Subject"),
        "exhausted bare R must not replay a message, got {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        !contains(&out, b"Message not found.") && !contains(&out, b"Usage: R <message-number>"),
        "exhausted bare R must not leak a notice, got {:?}",
        String::from_utf8_lossy(&out)
    );

    // `Q` at the QUIT prompt returns silently to the menu (no
    // "last message" text — `noDirF = 1`, `express.e:12227`/`:12302`).
    write_line(&mut stream, b"Q").await;
    let back = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&back, b"The last message in this conference"),
        "Q at the QUIT prompt must be silent, got {:?}",
        String::from_utf8_lossy(&back)
    );

    end_session(&mut stream).await;
}

/// Tier B B10: at the prompt-first bare-R prompt no message has been
/// displayed yet (legacy `tempFlag = 0`, `express.e:12087`), so the
/// message-operating options are inert — only `<CR>`/`L`/`Q`/`?` act.
/// `A` must not display a message and `R` must not open the reply editor.
#[tokio::test]
async fn bare_r_options_are_inert_before_the_first_message_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R").await;
    drain_until(&mut stream, b">: ").await;

    // `A` before any read is inert: no message body, prompt re-renders.
    write_line(&mut stream, b"A").await;
    let after_a = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_a, b"First Subject")
            && contains(&after_a, &sub_prompt(b"1+2", true, true)),
        "`A` before the first read must be inert, got {:?}",
        String::from_utf8_lossy(&after_a)
    );

    // `R` (reply) before any read is inert: no editor prompt.
    write_line(&mut stream, b"R").await;
    let after_r = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_r, b"End with a single '.'")
            && contains(&after_r, &sub_prompt(b"1+2", true, true)),
        "`R` before the first read must be inert, got {:?}",
        String::from_utf8_lossy(&after_r)
    );

    // `L`, by contrast, DOES work before the first read (it lists the
    // base, needing no loaded message): the starting-message prompt
    // appears and the table renders.
    write_line(&mut stream, b"L").await;
    drain_until(
        &mut stream,
        b"Starting message \x1b[33m[\x1b[0m1\x1b[33m]\x1b[0m: ",
    )
    .await;
    write_line(&mut stream, b"").await;
    let after_l = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after_l, b"\x1b[32mMsg    Type"),
        "`L` before the first read must list the base, got {:?}",
        String::from_utf8_lossy(&after_l)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

/// Tier B B10: at the `( QUIT )` prompt the `?` help reuses the same
/// range, so its `<CR>=Next ( ... )?` tail reads `QUIT` too (legacy
/// reuses `str`, `express.e:12031`/`:12059`).
#[tokio::test]
async fn help_tail_shows_quit_when_out_of_range() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // `R 2` reads the last message; the pointer is now past the end, so
    // the prompt is `( QUIT )`.
    write_line(&mut stream, b"R 2").await;
    drain_until(&mut stream, b">: ").await;

    // `?` short help reuses the QUIT range in its Next-tail.
    write_line(&mut stream, b"?").await;
    let help = drain_until(&mut stream, b")\x1b[0m? ").await;
    assert!(
        contains(&help, b"Next \x1b[32m(\x1b[0m QUIT \x1b[32m )\x1b[0m? "),
        "the help tail must carry the QUIT range, got {:?}",
        String::from_utf8_lossy(&help)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn again_re_displays_the_current_message_and_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `A`gain re-displays the current message (1) and stays on it — the
    // range remains `1+2` (legacy `displayMessage` then `nextMenu`,
    // `express.e:12102-12105`).
    write_line(&mut stream, b"A").await;
    let again = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&again, b"First Subject"),
        "`A` must re-display the current message, got {:?}",
        String::from_utf8_lossy(&again)
    );
    assert!(
        contains(&again, &sub_prompt(b"2+2", true, true)),
        "`A` must stay on the current message (range 2+2), got {:?}",
        String::from_utf8_lossy(&again)
    );
    assert!(
        !contains(&again, b"Second Subject"),
        "`A` must not advance to message 2, got {:?}",
        String::from_utf8_lossy(&again)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn reply_opens_the_editor_then_advances_to_the_next_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `R`eply drops into the body editor (proves the sub-prompt key
    // reaches `handle_reply`).
    write_line(&mut stream, b"R").await;
    drain_until(&mut stream, b"End with a single '.'").await;
    write_line(&mut stream, b"Replying from the sub-prompt.").await;
    write_line(&mut stream, b".").await;

    // After the reply the loop advances to message 2 and re-renders the
    // sub-prompt (legacy `R` -> `goNextMsg`, `express.e:12161-12168`).
    // The reply itself posted message 3, so the live range upper bound
    // is now `+3`.
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Second Subject"),
        "reply must advance to and display message 2, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, &sub_prompt(b"3+3", true, true)),
        "reply must advance to message 2 with the range reflecting the posted reply, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn forward_posts_then_stays_on_the_current_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `F`orward to the seeded sysop, with no note.
    write_line(&mut stream, b"F").await;
    drain_until(&mut stream, b"Forward to: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"blank line skips").await;
    write_line(&mut stream, b".").await;

    // The forward posts message 3, then the loop STAYS on message 1
    // (legacy `F` -> `nextMenu`, `express.e:12153-12160`): message 2 is
    // not displayed, and the live range upper bound is now `+3` from the
    // just-posted forward.
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Message #3 saved."),
        "forward must post message 3, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, &sub_prompt(b"2+3", true, true)),
        "forward must stay on message 1 with the range reflecting the posted forward, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        !contains(&after, b"Second Subject"),
        "forward must not advance to message 2, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn delete_removes_the_current_message_and_advances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `D`elete the current message (sysop is permitted), then advance to
    // the next one (legacy `D` -> `goNextMsg`, `express.e:12147-12152`).
    write_line(&mut stream, b"D").await;
    drain_until(&mut stream, b"Delete message (y/N)? ").await;
    write_line(&mut stream, b"y").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Message deleted."),
        "`D` must delete the current message, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, b"Second Subject") && contains(&after, &sub_prompt(b"QUIT", true, true)),
        "`D` must advance to message 2, then collapse to the QUIT prompt, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn move_relocates_then_advances_on_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `M`ove message 1 to base (1,2). On success the loop advances to
    // message 2 (legacy `M` -> `goNextMsg` on success, `express.e:12172`).
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, b"Target conference number: ").await;
    write_line(&mut stream, b"1").await;
    drain_until(&mut stream, b"Target msgbase number: ").await;
    write_line(&mut stream, b"2").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Message moved. New number 1."),
        "`M` must move the message, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, b"Second Subject") && contains(&after, &sub_prompt(b"QUIT", true, true)),
        "a successful `M` must advance to message 2, then collapse to the QUIT prompt, got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn move_to_a_missing_target_stays_on_the_current_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // A move to a non-existent target fails; the loop STAYS on message 1
    // (legacy `M` only jumps to `goNextMsg` on `RESULT_SUCCESS`,
    // `express.e:12172`).
    write_line(&mut stream, b"M").await;
    drain_until(&mut stream, b"Target conference number: ").await;
    write_line(&mut stream, b"9").await;
    drain_until(&mut stream, b"Target msgbase number: ").await;
    write_line(&mut stream, b"9").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"No such target message base."),
        "an unknown move target must be reported, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, &sub_prompt(b"2+2", true, true)) && !contains(&after, b"Second Subject"),
        "a failed `M` must stay on message 1 (range 2+2), got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn edit_header_updates_the_subject_then_re_displays_and_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `EH` edits the header (sysop is permitted), changing the subject
    // and keeping the addressee, then re-displays the edited message and
    // STAYS on it (legacy `EH` -> `displayMessage` -> `nextMenu`,
    // `express.e:12179-12193`).
    write_line(&mut stream, b"EH").await;
    drain_until(&mut stream, b"New subject (blank = unchanged): ").await;
    write_line(&mut stream, b"Edited Subject").await;
    drain_until(&mut stream, b"New To (blank = unchanged): ").await;
    write_line(&mut stream, b"").await;
    let after = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&after, b"Header updated."),
        "`EH` must report the header update, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, b"Edited Subject"),
        "`EH` must re-display the message with the new subject, got {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        contains(&after, &sub_prompt(b"2+2", true, true)) && !contains(&after, b"Second Subject"),
        "`EH` must stay on message 1 (range 2+2), got {:?}",
        String::from_utf8_lossy(&after)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn a_regular_user_cannot_delete_or_edit_others_mail_from_the_sub_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    // The regular user (slot 3) is neither author (slot 2) nor addressee
    // (slot 1) of the seeded mail, and sits below access 210.
    let mut stream = sign_in(&addr, b"regular", b"regular").await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `D`elete is gated on the per-message delete permission — denied
    // here, so the key is ignored (no confirm prompt) and the loop
    // re-renders the sub-prompt on the same message.
    write_line(&mut stream, b"D").await;
    let after_d = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_d, b"Delete message"),
        "`D` must be ignored for a non-owner regular user, got {:?}",
        String::from_utf8_lossy(&after_d)
    );
    assert!(
        contains(&after_d, &sub_prompt(b"2+2", false, true)),
        "the sub-prompt must re-render on message 1 after the ignored `D`, got {:?}",
        String::from_utf8_lossy(&after_d)
    );

    // `EH` is gated on edit-header access (sysop / access >= 210) — also
    // denied, so it too is ignored (no `New subject` prompt).
    write_line(&mut stream, b"EH").await;
    let after_eh = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_eh, b"New subject"),
        "`EH` must be ignored for a non-privileged user, got {:?}",
        String::from_utf8_lossy(&after_eh)
    );
    assert!(
        contains(&after_eh, &sub_prompt(b"2+2", false, true)),
        "the sub-prompt must re-render on message 1 after the ignored `EH`, got {:?}",
        String::from_utf8_lossy(&after_eh)
    );

    // The long help is gated the same way: this caller sees `M`ove but
    // neither the `D`elete entry (not their message) nor the `EH`
    // entry (below the edit-header access tier).
    write_line(&mut stream, b"??").await;
    let help = drain_until(&mut stream, b")\x1b[0m? ").await;
    assert!(
        contains(&help, b"\x1b[33mM\x1b[32m>\x1b[36move Message\x1b[0m"),
        "the regular user's long help must still offer `M`ove, got {:?}",
        String::from_utf8_lossy(&help)
    );
    assert!(
        !contains(&help, b"Delete Message") && !contains(&help, b"Edit Message Header"),
        "the regular user's help must omit the ungranted `D` / `EH` entries, got {:?}",
        String::from_utf8_lossy(&help)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn question_mark_shows_the_short_help_then_double_shows_the_long_help() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `?` renders the short help (`express.e:12023-12032`) as the next
    // prompt. The sysop sees the gated `D` / `M` entries; the short
    // forms spell `Move` / `List` tersely and the long-only strings are
    // absent.
    write_line(&mut stream, b"?").await;
    let short = drain_until(&mut stream, b")\x1b[0m? ").await;
    assert!(
        contains(&short, b"\x1b[33mA\x1b[32m>\x1b[36mgain\x1b[0m")
            && contains(&short, b"\x1b[33mD\x1b[32m>\x1b[36melete Message\x1b[0m")
            && contains(&short, b"\x1b[33mM\x1b[32m>\x1b[36move\x1b[0m")
            && contains(&short, b"\x1b[33mL\x1b[32m>\x1b[36mist\x1b[0m")
            && contains(&short, b"\x1b[33mQ\x1b[32m>\x1b[36muit\x1b[0m")
            && contains(&short, b"Next \x1b[32m(\x1b[0m 2+2 \x1b[32m )\x1b[0m? "),
        "short help must list the gated options + range, got {:?}",
        String::from_utf8_lossy(&short)
    );
    assert!(
        !contains(&short, b"Move Message") && !contains(&short, b"Edit Message Header"),
        "short help must not carry the long-only wording, got {:?}",
        String::from_utf8_lossy(&short)
    );

    // `??` renders the long help (`:12034-12060`): fuller wording for
    // `Move` / `List` and the sysop-gated `EH` entry.
    write_line(&mut stream, b"??").await;
    let long = drain_until(&mut stream, b")\x1b[0m? ").await;
    assert!(
        contains(&long, b"\x1b[33mM\x1b[32m>\x1b[36move Message\x1b[0m")
            && contains(&long, b"\x1b[33mL\x1b[32m>\x1b[36mist all messages\x1b[0m")
            && contains(
                &long,
                b"\x1b[33mEH\x1b[32m>\x1b[36m Edit Message Header\x1b[0m"
            )
            && contains(&long, b"Next \x1b[32m(\x1b[0m 2+2 \x1b[32m )\x1b[0m? "),
        "long help must list the fuller wording + EH, got {:?}",
        String::from_utf8_lossy(&long)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn list_renders_the_paginated_message_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    // Seed more messages than fit a default 22-line page so the listing
    // is forced to pause.
    seed_n_messages(&msgbase, 25);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `L` prompts for a starting message, defaulting to the lowest (1).
    write_line(&mut stream, b"L").await;
    drain_until(
        &mut stream,
        b"Starting message \x1b[33m[\x1b[0m1\x1b[33m]\x1b[0m: ",
    )
    .await;
    write_line(&mut stream, b"").await;

    // The `listMSGs` table renders with the message number FIRST (unlike
    // the scan table) and pauses once the page fills.
    let page = drain_until(&mut stream, b"(Pause)...More(y/n/ns)? ").await;
    assert!(
        contains(&page, b"\x1b[32mMsg    Type"),
        "expected the Msg-first list header, got {:?}",
        String::from_utf8_lossy(&page)
    );
    assert!(
        contains(&page, b"000001 Public "),
        "expected message 1's row, got {:?}",
        String::from_utf8_lossy(&page)
    );

    // `ns` finishes the listing without further pauses, then the loop
    // returns to the sub-prompt.
    write_line(&mut stream, b"ns").await;
    let rest = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&rest, b"000025 Public "),
        "non-stop must render the rest of the list, got {:?}",
        String::from_utf8_lossy(&rest)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

/// Seeds `n` unread public messages addressed to the seeded sysop,
/// numbered 1..=n.
fn seed_n_messages(msgbase: &std::path::Path, n: u32) {
    std::fs::create_dir_all(msgbase).expect("create msgbase");
    for i in 1..=n {
        std::fs::write(
            msgbase.join(format!("{i:07}.json")),
            seeded_mail_json(i, "Carol", &format!("Subject {i}"), "Body."),
        )
        .expect("seed message");
    }
}

#[tokio::test]
async fn list_excludes_mail_not_addressed_to_the_reader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    std::fs::create_dir_all(&msgbase).expect("create msgbase");
    // 1: to the sysop (shown). 2: to a stranger (excluded). 3: deleted
    // (excluded).
    std::fs::write(
        msgbase.join("0000001.json"),
        mail_json(1, "sysop", 1, "public", "MineSubject"),
    )
    .expect("seed 1");
    std::fs::write(
        msgbase.join("0000002.json"),
        mail_json(2, "stranger", 99, "public", "StrangerSubject"),
    )
    .expect("seed 2");
    std::fs::write(
        msgbase.join("0000003.json"),
        mail_json(3, "sysop", 1, "deleted", "DeletedSubject"),
    )
    .expect("seed 3");

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    write_line(&mut stream, b"L").await;
    drain_until(&mut stream, b"\x1b[33m]\x1b[0m: ").await;
    write_line(&mut stream, b"").await;
    // Only one row fits, so the listing finishes and returns to the
    // sub-prompt without a pause.
    let list = drain_until(&mut stream, b">: ").await;
    assert!(
        contains(&list, b"MineSubject"),
        "the reader's own message must be listed, got {:?}",
        String::from_utf8_lossy(&list)
    );
    assert!(
        !contains(&list, b"StrangerSubject") && !contains(&list, b"DeletedSubject"),
        "mail addressed elsewhere or deleted must be excluded, got {:?}",
        String::from_utf8_lossy(&list)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

/// JSON for one message with a chosen addressee slot and visibility, in
/// the [`FileMailStore`] format.
fn mail_json(
    number: u32,
    to_name: &str,
    addressee_slot: u32,
    visibility: &str,
    subject: &str,
) -> String {
    format!(
        r#"{{
            "conference_number": 1,
            "msgbase_number": 1,
            "number": {number},
            "visibility": "{visibility}",
            "from_name": "Carol",
            "to_name": "{to_name}",
            "broadcast_to": "none",
            "subject": "{subject}",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": {addressee_slot},
            "body": "Body."
        }}"#
    )
}

#[tokio::test]
async fn sub_prompt_reply_and_forward_abort_silently() {
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"R 1").await;
    drain_until(&mut stream, b">: ").await;

    // `R`eply, then `/A` aborts the body. The legacy `readMSG` reply is
    // silent on abort (B6) — no `Message aborted.` notice. `R` still
    // advances afterwards (legacy `goNextMsg`).
    write_line(&mut stream, b"R").await;
    drain_until(&mut stream, b"End with a single '.'").await;
    write_line(&mut stream, b"/A").await;
    let after_reply = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_reply, b"Message aborted"),
        "an aborted sub-prompt reply must be silent, got {:?}",
        String::from_utf8_lossy(&after_reply)
    );

    // `F`orward, then a blank `Forward to:` aborts — also silent.
    write_line(&mut stream, b"F").await;
    drain_until(&mut stream, b"Forward to: ").await;
    write_line(&mut stream, b"").await;
    let after_forward = drain_until(&mut stream, b">: ").await;
    assert!(
        !contains(&after_forward, b"Message aborted"),
        "an aborted sub-prompt forward must be silent, got {:?}",
        String::from_utf8_lossy(&after_forward)
    );

    write_line(&mut stream, b"Q").await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}

#[tokio::test]
async fn entering_mail_with_a_blank_subject_still_shows_the_abort_notice() {
    // The silent-abort change (B6) is scoped to the sub-prompt reply /
    // forward; the top-level `E` composer keeps its `Message aborted.`
    // notice on a blank subject.
    let dir = tempfile::tempdir().expect("tempdir");
    let msgbase = dir.path().join("conf1_msgbase");
    seed_two_message_base(&msgbase);

    let addr = spawn_one_conference_listener(dir.path().to_path_buf(), &msgbase).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"E sysop").await;
    drain_until(&mut stream, b"Subject: ").await;
    write_line(&mut stream, b"").await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after, b"Message aborted."),
        "a blank `E` subject must still report `Message aborted.`, got {:?}",
        String::from_utf8_lossy(&after)
    );

    end_session(&mut stream).await;
}

/// The verbatim `readMSG` sub-prompt skeleton
/// (`amiexpress/express.e:12016-12021`) with `range` substituted into
/// the `( <range> )` slot. `show_delete` / `show_move` insert the `D` /
/// `M` options after `A` for a permitted caller; with neither, the two
/// `ESC[36m` codes collapse into the legacy doubled-seam where `A`
/// joins `F`. ANSI escapes are emitted literally.
fn sub_prompt(range: &[u8], show_delete: bool, show_move: bool) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m");
    if show_delete {
        v.extend_from_slice(b",\x1b[33mD");
    }
    if show_move {
        v.extend_from_slice(b",\x1b[33mM");
    }
    v.extend_from_slice(
        b"\x1b[36m,\x1b[33mF\x1b[36m,\x1b[33mR\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mQ\x1b[36m,\x1b[33m?\x1b[36m,\x1b[33m??\x1b[36m,\x1b[32m<\x1b[33mCR\x1b[32m> \x1b[32m(\x1b[0m ",
    );
    v.extend_from_slice(range);
    v.extend_from_slice(b" \x1b[32m )\x1b[0m>: ");
    v
}

/// JSON payload for one public message addressed to the seeded sysop
/// (slot 1, handle "sysop"), in the [`FileMailStore`] on-disk format.
fn seeded_mail_json(number: u32, from: &str, subject: &str, body: &str) -> String {
    format!(
        r#"{{
            "conference_number": 1,
            "msgbase_number": 1,
            "number": {number},
            "visibility": "public",
            "from_name": "{from}",
            "to_name": "sysop",
            "broadcast_to": "none",
            "subject": "{subject}",
            "posted_at": "1970-01-01T00:00:01Z",
            "received_at": null,
            "author_slot": 2,
            "addressee_slot": 1,
            "body": "{body}"
        }}"#
    )
}

/// Builds a `Runtime` with one conference accessible to the seeded
/// sysop, backing its message base with a file-backed store rooted at
/// the supplied temp directory, then binds a [`TelnetListener`] and
/// spawns its accept loop.
async fn spawn_one_conference_listener(
    bbs_path: std::path::PathBuf,
    msgbase: &std::path::Path,
) -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    // Two message bases: (1,1) carries the seeded mail; (1,2) is an
    // empty move target so the sub-prompt `M`ove option has somewhere
    // to land.
    let conferences = vec![Conference::new(
        1,
        "One".to_string(),
        vec![
            MessageBase::new(1, 1, "general".to_string()),
            MessageBase::new(1, 2, "second".to_string()),
        ],
    )
    .expect("valid conference")];

    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &conferences);
    // This smoke isolates the `R` sub-prompt. Opt the sysop out of the
    // logon conference scan — which would otherwise surface the seeded
    // mail with a read-it-now offer before the menu — by clearing the
    // per-conference `mail_scan` flag. The logon scan has its own
    // dedicated smoke (`logon_conference_scan_smoke.rs`).
    for membership in sysop.memberships_mut() {
        membership.set_scan_flag(ScanFlag::MailScan, false);
    }
    // A validated, non-sysop regular user (slot 3, access 100) used to
    // prove the sub-prompt's `D` / `EH` gates deny a caller who is
    // neither the message owner nor privileged.
    let regular_pw = hasher
        .compute_password_hash("regular", PasswordHashKind::Pbkdf210000)
        .expect("hash regular password");
    let mut regular = User::new(
        3,
        "regular".to_string(),
        PasswordHashKind::Pbkdf210000,
        regular_pw.hash,
        regular_pw.salt,
        SystemTime::UNIX_EPOCH,
        100,
    )
    .expect("valid regular user");
    regular.upsert_membership(ConferenceMembership::new(1, true));
    let user_repo: SharedUserRepo = Arc::new(InMemoryUserRepository::new(vec![sysop, regular]))
        as Arc<dyn UserRepository + Send + Sync>;
    let hasher_shared: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;

    let msgbase2 = bbs_path.join("conf1_msgbase2");
    std::fs::create_dir_all(&msgbase2).expect("create second msgbase");
    let mut registry = InMemoryMailStores::new();
    registry.register(
        MessageBaseRef::new(1, 1),
        Box::new(
            FileMailStore::open(msgbase.to_path_buf(), MessageBaseRef::new(1, 1))
                .expect("open conf1 store"),
        ),
    );
    registry.register(
        MessageBaseRef::new(1, 2),
        Box::new(
            FileMailStore::open(msgbase2, MessageBaseRef::new(1, 2)).expect("open conf1 base 2"),
        ),
    );
    let mail_stores: SharedMailStores = Arc::new(registry) as Arc<dyn MailStores + Send + Sync>;
    let conferences_handle: SharedConferences = Arc::new(conferences);

    let config = Config {
        max_nodes: 1,
        max_password_failures: 3,
        bbs_path,
        ..Config::default()
    };
    let runtime = bootstrap::build_runtime(
        &config,
        user_repo,
        hasher_shared,
        caller_log,
        conferences_handle,
        mail_stores,
    );

    let listener = TelnetListener::bind("127.0.0.1:0", runtime)
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local_addr");
    let listener = Arc::new(listener);
    let task_listener = listener.clone();
    tokio::spawn(async move { task_listener.run().await });
    addr
}

async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    sign_in(addr, b"sysop", b"sysop").await
}

async fn sign_in(addr: &std::net::SocketAddr, handle: &[u8], password: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, handle).await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, password).await;
    drain_until(&mut stream, b"mins. left): ").await;
    stream
}

async fn end_session(stream: &mut TcpStream) {
    write_line(stream, b"G").await;
    drain_until(stream, b"Goodbye").await;
}

async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

async fn drain_until(stream: &mut TcpStream, needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let n = match tokio::time::timeout(DRAIN_DEADLINE, stream.read(&mut chunk)).await {
            Ok(Ok(n)) => n,
            Ok(Err(_)) | Err(_) => 0,
        };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
        if contains(&out, needle) {
            break;
        }
    }
    assert!(
        contains(&out, needle),
        "needle {:?} not found within {DRAIN_DEADLINE:?}; got {:?}",
        std::str::from_utf8(needle).unwrap_or("<bin>"),
        String::from_utf8_lossy(&out),
    );
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
