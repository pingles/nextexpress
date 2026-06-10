//! Tier A "quick wins" in-process integration tests.
//!
//! Each scenario boots a [`TelnetListener`] in-process on a tokio
//! task, opens a real telnet client to the bound address, drives one
//! Tier A quickwin, and asserts the verbatim `AmiExpress` wire text.
//! Going in-process (rather than spawning the `nextexpress` binary)
//! cuts the per-test cost from a full process startup to a single
//! `Runtime` build, while still exercising the same composition root
//! and the same telnet adapter the binary uses.

use std::sync::Arc;
use std::time::Duration;

use nextexpress::adapters::in_memory_caller_log::InMemoryCallerLog;
use nextexpress::adapters::in_memory_file_repository::InMemoryFileRepository;
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
use nextexpress::domain::conference::{Conference, MessageBase};
use nextexpress::domain::password::PasswordHasher;
use nextexpress::domain::user_repository::UserRepository;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Per-`drain_until` deadline. A real BBS prompt arrives in
/// milliseconds; two seconds is generous enough to forgive a slow CI
/// runner without making genuine failures wait forever.
const DRAIN_DEADLINE: Duration = Duration::from_secs(2);

#[tokio::test]
async fn t_command_renders_legacy_it_is_format() {
    // Slice A1 — `T` (current date/time). Mirrors
    // `internalCommandT()` at `amiexpress/express.e:25622-25644`.
    // The wall-clock fields are wall-clock-dependent so the smoke
    // pins the surrounding literal: `It is ` prefix, CRLF terminator,
    // and a `MM-DD-YY HH:MM:SS` structure.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"T").await;
    let post_t = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&post_t, b"It is "),
        "expected legacy `It is ` prefix after T, got {:?}",
        String::from_utf8_lossy(&post_t)
    );
    assert_time_line_shape(&post_t);

    end_session(&mut stream).await;
}

#[tokio::test]
async fn ver_command_renders_legacy_version_banner() {
    // Slice A2 — `VER` (version banner). Mirrors
    // `internalCommandVER()` at `amiexpress/express.e:25688-25698`.
    // Each author line is pinned so a future wording drift fails
    // here rather than silently in production.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"VER").await;
    let post_ver = drain_until(&mut stream, b"mins. left): ").await;
    let needles: &[&[u8]] = &[
        "NextExpress ".as_bytes(),
        " Copyright \u{00A9}2026 Paul Ingles\r\n".as_bytes(),
        b"Based on Versions:\r\n",
        "  AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n".as_bytes(),
        b"  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n",
        b"  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n",
    ];
    for needle in needles {
        assert!(
            contains(&post_ver, needle),
            "expected `{}` in VER response, got {:?}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&post_ver)
        );
    }
    // Slice A2 (Out of Scope): no `Registered to` line.
    assert!(
        !contains(&post_ver, b"Registered to"),
        "VER must elide the legacy `Registered to` line, got {:?}",
        String::from_utf8_lossy(&post_ver)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn q_command_toggles_quiet_mode_on_then_off() {
    // Slice A9 — `Q` (quiet-mode toggle). Mirrors
    // `internalCommandQ()` at `amiexpress/express.e:25504-25516`.
    // The first press emits `Quiet Mode On` (legacy `\b\n` → telnet
    // `\r\n`); the second press emits `Quiet Mode Off`. Both
    // responses are pinned verbatim so a future wording drift fails
    // here rather than silently in production.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"Q").await;
    let after_first = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_first, b"\r\nQuiet Mode On\r\n"),
        "expected `Quiet Mode On` after first Q, got {:?}",
        String::from_utf8_lossy(&after_first)
    );
    assert!(
        !contains(&after_first, b"Quiet Mode Off"),
        "first Q must not emit the Off variant, got {:?}",
        String::from_utf8_lossy(&after_first)
    );

    write_line(&mut stream, b"Q").await;
    let after_second = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_second, b"\r\nQuiet Mode Off\r\n"),
        "expected `Quiet Mode Off` after second Q, got {:?}",
        String::from_utf8_lossy(&after_second)
    );
    assert!(
        !contains(&after_second, b"Quiet Mode On"),
        "second Q must not re-emit the On variant, got {:?}",
        String::from_utf8_lossy(&after_second)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn s_command_renders_user_stats_screen() {
    // Slice A3 — `S` (user stats). Mirrors `internalCommandS()` at
    // `amiexpress/express.e:25540-25608`. The counter and date fields
    // are session-state-dependent, so the smoke pins only the stable
    // fields — slot `1` and access level `255` for the seeded sysop —
    // plus the presence of every baseline label with its `[32m…[33m:`
    // ANSI prefix, proving the screen is reachable end-to-end.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"S").await;
    let post_s = drain_until(&mut stream, b"mins. left): ").await;

    let needles: &[&[u8]] = &[
        b"\x1b[32mUser Number\x1b[33m:\x1b[0m 1\r\n",
        b"\x1b[32mSecurity Lv\x1b[33m:\x1b[0m 255\r\n",
        b"\x1b[32mLst Date On\x1b[33m:\x1b[0m ",
        b"\x1b[32m# Times On \x1b[33m:\x1b[0m ",
        b"\x1b[32mTimes Today\x1b[33m:\x1b[0m ",
        b"\x1b[32mMsgs Posted\x1b[33m:\x1b[0m ",
    ];
    for needle in needles {
        assert!(
            contains(&post_s, needle),
            "expected `{}` in S response, got {:?}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&post_s)
        );
    }

    end_session(&mut stream).await;
}

#[tokio::test]
async fn m_command_toggles_ansi_colour_and_strips_escapes() {
    // Slice A8 — `M` (ANSI toggle). Mirrors `internalCommandM()` at
    // `amiexpress/express.e:25239`: the first press emits `Ansi Color
    // Off` and the `ColourTerminal` decorator then strips ANSI SGR
    // escapes from output; the second press emits `Ansi Color On` and
    // colour returns. Colour is on by default for a fresh connection.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Colour on by default: the menu prompt carries ANSI escapes.
    write_line(&mut stream, b"T").await;
    let colour_on = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&colour_on, b"\x1b["),
        "with colour on the prompt must carry ANSI escapes, got {:?}",
        String::from_utf8_lossy(&colour_on)
    );

    // `M` turns colour off; the following menu + prompt are stripped.
    write_line(&mut stream, b"M").await;
    let after_off = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_off, b"\r\nAnsi Color Off\r\n"),
        "expected `Ansi Color Off` after first M, got {:?}",
        String::from_utf8_lossy(&after_off)
    );
    assert!(
        !contains(&after_off, b"\x1b["),
        "with colour off all ANSI escapes must be stripped, got {:?}",
        String::from_utf8_lossy(&after_off)
    );

    // `M` again restores colour; the prompt's ANSI escapes return.
    write_line(&mut stream, b"M").await;
    let after_on = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_on, b"\r\nAnsi Color On\r\n"),
        "expected `Ansi Color On` after second M, got {:?}",
        String::from_utf8_lossy(&after_on)
    );
    assert!(
        contains(&after_on, b"\x1b["),
        "with colour restored the prompt's ANSI escapes return, got {:?}",
        String::from_utf8_lossy(&after_on)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn caret_command_displays_topic_help_and_is_silent_on_no_match() {
    // Slice A10 — `^<topic>` (topic help). Mirrors
    // `internalCommandUpHat()` at `amiexpress/express.e:25089`: reads
    // `<bbs-loc>/help/<topic>.txt` and displays it; a topic with no
    // matching screen is a silent no-op.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("help")).expect("mkdir help");
    std::fs::write(
        dir.path().join("help").join("FILES.txt"),
        b"FILE AREA HELP\x08\nUse F to list.\x08\n",
    )
    .expect("write help/FILES.txt");
    let addr = spawn_listener_at_bbs_path(dir.path().to_path_buf()).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"^FILES").await;
    let post_help = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&post_help, b"FILE AREA HELP\r\nUse F to list.\r\n"),
        "expected disk topic-help asset (CRLF-normalised), got {:?}",
        String::from_utf8_lossy(&post_help)
    );

    // A topic with no matching screen displays nothing.
    write_line(&mut stream, b"^NOPE").await;
    let post_miss = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&post_miss, b"FILE AREA HELP"),
        "an unmatched topic must be a silent no-op, got {:?}",
        String::from_utf8_lossy(&post_miss)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn question_mark_redisplays_menu_only_in_expert_mode() {
    // Slice A7 — `?` (display menu). Mirrors
    // `internalCommandQuestionMark()` at
    // `amiexpress/express.e:24594-24599`: a no-op outside expert mode
    // (the loop already auto-displays the menu), and a re-display of
    // the conference menu inside it. The harness has no `Menu.txt`
    // asset, so the menu surfaces as the built-in `[ Default menu …`
    // fallback.
    let needle: &[u8] = b"[ Default menu";
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Normal mode: `?` is a no-op, so the only menu in the cycle is the
    // loop's single auto-display — exactly one, not a duplicate.
    write_line(&mut stream, b"?").await;
    let normal = drain_until(&mut stream, b"mins. left): ").await;
    let menu_count = normal
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count();
    assert_eq!(
        menu_count,
        1,
        "in normal mode `?` must not add a second menu, got {:?}",
        String::from_utf8_lossy(&normal)
    );

    // Enable expert mode: the loop stops auto-displaying the menu.
    write_line(&mut stream, b"X").await;
    drain_until(&mut stream, b"mins. left): ").await;

    // In expert mode, `?` brings the menu back.
    write_line(&mut stream, b"?").await;
    let expert = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&expert, needle),
        "in expert mode `?` must re-display the menu, got {:?}",
        String::from_utf8_lossy(&expert)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn x_command_toggles_expert_mode_and_gates_the_menu() {
    // Slice A6 — `X` (expert-mode toggle). Mirrors `internalCommandX()`
    // at `amiexpress/express.e:26113-26121`: the first press emits
    // `Expert mode enabled` and stops the menu auto-displaying before
    // the prompt; the second press emits `Expert mode disabled` and
    // restores it (legacy `displayMenuPrompt` gate at :28583). The
    // test harness has no `Menu.txt` asset, so the menu surfaces as the
    // built-in `[ Default menu …` fallback.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"X").await;
    let after_enable = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_enable, b"\r\nExpert mode enabled\r\n"),
        "expected `Expert mode enabled` after first X, got {:?}",
        String::from_utf8_lossy(&after_enable)
    );

    // In expert mode the menu is suppressed before the next prompt.
    write_line(&mut stream, b"T").await;
    let expert_cycle = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        !contains(&expert_cycle, b"[ Default menu"),
        "menu must be suppressed in expert mode, got {:?}",
        String::from_utf8_lossy(&expert_cycle)
    );

    write_line(&mut stream, b"X").await;
    let after_disable = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&after_disable, b"\r\nExpert mode disabled\r\n"),
        "expected `Expert mode disabled` after second X, got {:?}",
        String::from_utf8_lossy(&after_disable)
    );

    // Back in normal mode the menu auto-displays again.
    write_line(&mut stream, b"T").await;
    let normal_cycle = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&normal_cycle, b"[ Default menu"),
        "menu must reappear in normal mode, got {:?}",
        String::from_utf8_lossy(&normal_cycle)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn menu_prompt_renders_bbs_name_conference_and_mins_left() {
    // Slice A4 (menu-prompt parity). Mirrors the default branch of
    // `displayMenuPrompt()` at `amiexpress/express.e:28419`:
    // `<bbsName> [<confNum>:<confName>] Menu (<mins> mins. left): `
    // with the legacy ANSI colour run. The seeded sysop auto-rejoins
    // conference 1 ("Main") and the default config BBS name is
    // "NextExpress". The `<mins>` value is session-dependent, so the
    // assertion pins the stable prefix (up to the yellow minutes) and
    // the suffix, not the digit count.
    let addr = spawn_listener_with_seeded_sysop().await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    // Issue any command and capture the prompt that follows it.
    write_line(&mut stream, b"T").await;
    let buf = drain_until(&mut stream, b"mins. left): ").await;

    assert!(
        contains(
            &buf,
            b"\x1b[0m\x1b[35mNextExpress \x1b[0m[\x1b[36m1\x1b[34m:\x1b[36mMain\x1b[0m] Menu (\x1b[33m"
        ),
        "expected legacy menu-prompt prefix (bbs name + [1:Main] + Menu), got {:?}",
        String::from_utf8_lossy(&buf)
    );
    assert!(
        contains(&buf, b"\x1b[0m mins. left): "),
        "expected legacy `mins. left): ` suffix, got {:?}",
        String::from_utf8_lossy(&buf)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn h_command_falls_back_to_help_unavailable_when_no_asset() {
    // Slice A5 — `H` (BBS help). When `<bbs-loc>/BBSHelp.txt` is
    // absent the listener emits the verbatim legacy line at
    // `amiexpress/express.e:25083`, then re-prompts.
    let dir = tempfile::tempdir().expect("tempdir");
    let addr = spawn_listener_at_bbs_path(dir.path().to_path_buf()).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"H").await;
    let post_h = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(
            &post_h,
            b"\r\n\r\nSorry Help is unavailable at this time.\r\n\r\n"
        ),
        "expected legacy help-unavailable line after H, got {:?}",
        String::from_utf8_lossy(&post_h)
    );

    end_session(&mut stream).await;
}

#[tokio::test]
async fn h_command_renders_bbs_help_asset_when_present() {
    // Slice A5 — `H` (BBS help). When `<bbs-loc>/BBSHelp.txt` exists
    // the listener writes it through, with Amiga `\b\n` line endings
    // translated to telnet `\r\n` by the screen adapter.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("BBSHelp.txt"),
        b"== Help Screen ==\x08\nType G to log off.\x08\n",
    )
    .expect("write BBSHelp.txt");
    let addr = spawn_listener_at_bbs_path(dir.path().to_path_buf()).await;
    let mut stream = sign_in_seeded_sysop(&addr).await;

    write_line(&mut stream, b"H").await;
    let post_h = drain_until(&mut stream, b"mins. left): ").await;
    assert!(
        contains(&post_h, b"== Help Screen ==\r\nType G to log off.\r\n"),
        "expected disk help asset (CRLF-normalised), got {:?}",
        String::from_utf8_lossy(&post_h)
    );
    // The fallback line must NOT appear when an asset is present.
    assert!(
        !contains(&post_h, b"Sorry Help is unavailable"),
        "fallback must not fire when BBSHelp.txt is on disk, got {:?}",
        String::from_utf8_lossy(&post_h)
    );

    end_session(&mut stream).await;
}

/// Builds a `Runtime` with an in-memory user repo, the seeded sysop,
/// a single `Main` conference, an empty mail store, and an in-memory
/// caller log, then binds a [`TelnetListener`] on an ephemeral port
/// and spawns its accept loop. Returns the address the listener is
/// bound to. The BBS root defaults to the current working directory
/// (cargo's manifest dir at test time), which has no asset overrides;
/// use [`spawn_listener_at_bbs_path`] when a scenario needs assets on
/// disk.
async fn spawn_listener_with_seeded_sysop() -> std::net::SocketAddr {
    spawn_listener_at_bbs_path(std::env::current_dir().expect("cwd")).await
}

/// Variant of [`spawn_listener_with_seeded_sysop`] that roots the
/// `Runtime`'s BBS path at a caller-supplied directory — used by
/// scenarios that need to drop screen assets (e.g. `BBSHelp.txt`)
/// where the [`FileScreenRepository`] will find them.
async fn spawn_listener_at_bbs_path(bbs_path: std::path::PathBuf) -> std::net::SocketAddr {
    let hasher = Arc::new(Pbkdf2PasswordHasher::new());
    let conferences = vec![Conference::new(
        1,
        "Main".to_string(),
        vec![MessageBase::new(1, 1, "main".to_string())],
    )
    .expect("valid conference")];

    let mut sysop = seed::default_sysop(hasher.as_ref()).expect("seed sysop");
    seed::grant_all_memberships(&mut sysop, &conferences);
    let user_repo: SharedUserRepo =
        Arc::new(InMemoryUserRepository::new(vec![sysop])) as Arc<dyn UserRepository + Send + Sync>;
    let hasher_shared: SharedHasher = hasher as Arc<dyn PasswordHasher + Send + Sync>;
    let caller_log: SharedCallerLog =
        Arc::new(InMemoryCallerLog::new()) as Arc<dyn CallerLogAppender + Send + Sync>;
    let mail_stores: SharedMailStores =
        Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>;
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
        Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
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

/// Connects to `addr`, walks the standard auth handshake as the
/// seeded `sysop` / `sysop`, and returns the open stream sitting at
/// the menu prompt (whose stable tail is `mins. left): `).
async fn sign_in_seeded_sysop(addr: &std::net::SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    drain_until(&mut stream, b"ANSI Graphics (Y/n)? ").await;
    write_line(&mut stream, b"Y").await;
    drain_until(&mut stream, b"Enter your Name: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"PassWord: ").await;
    write_line(&mut stream, b"sysop").await;
    drain_until(&mut stream, b"mins. left): ").await;
    stream
}

/// Sends `G` to log off and drains until the listener emits the
/// `Goodbye` line, mirroring the close-of-session pattern shared by
/// every quickwin scenario.
async fn end_session(stream: &mut TcpStream) {
    write_line(stream, b"G").await;
    drain_until(stream, b"Goodbye").await;
}

/// Structural check on the rendered `T` line: between `It is ` and
/// the CRLF the format is `MM-DD-YY HH:MM:SS` — three hyphen-separated
/// date parts and three colon-separated time parts. Anything else
/// (e.g. a stub literal or a swapped separator) fails the parse.
fn assert_time_line_shape(post_t: &[u8]) {
    let it_is_idx = find(post_t, b"It is ").expect("`It is ` prefix not found");
    let tail = &post_t[it_is_idx + b"It is ".len()..];
    let line_end = tail
        .windows(2)
        .position(|w| w == b"\r\n")
        .expect("missing CRLF terminator after time line");
    let line = std::str::from_utf8(&tail[..line_end]).expect("non-utf8 time line");
    let (date, clock) = line.split_once(' ').unwrap_or_else(|| {
        panic!("expected `<date> <time>`, got {line:?}");
    });
    let date_parts: Vec<&str> = date.split('-').collect();
    let clock_parts: Vec<&str> = clock.split(':').collect();
    assert!(
        date_parts.len() == 3 && clock_parts.len() == 3,
        "expected `MM-DD-YY HH:MM:SS` after `It is `, got {line:?}",
    );
}

async fn write_line(stream: &mut TcpStream, body: &[u8]) {
    stream.write_all(body).await.expect("write body");
    stream.write_all(b"\r\n").await.expect("write CRLF");
    stream.flush().await.expect("flush");
}

/// Reads bytes from `stream` until `needle` appears in the
/// accumulated buffer, EOF arrives, or [`DRAIN_DEADLINE`] elapses.
/// The deadline matters: a broken server would otherwise leave us
/// blocked on `read` forever.
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
