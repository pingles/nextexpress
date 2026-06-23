//! Wire-format byte constants the BBS workflow writes to terminals.
//!
//! Lifting these out of `session_driver` so:
//!   1. the workflow file shrinks to the orchestration that earns its
//!      keep, and
//!   2. future I18N / theming / ANSI-rendering work has a single place
//!      to plug.
//!
//! Each constant's doc comment cross-references the legacy `AmiExpress`
//! source so spec-driven changes can be traced back to the original.

/// The telnet line terminator (`\r\n`) — the one newline primitive the
/// whole wire is built from. Standalone newline writes go through
/// [`MenuFlow::write_newline`](crate::app::menu_flow); this constant is
/// for composing it into larger byte sequences.
pub(crate) const CRLF: &[u8] = b"\r\n";

/// Prompt asking a registering user for the handle they want.
/// Mirrors the wire format of [`NAME_PROMPT`] (CRLF prefix, trailing
/// space) — `amiexpress/express.e:30141`.
///
/// [`NAME_PROMPT`]: crate::app::login_flow::NAME_PROMPT
pub(crate) const REGISTRATION_HANDLE_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's location during registration. Verbatim from
/// `amiexpress/express.e:30194`.
pub(crate) const LOCATION_PROMPT: &[u8] = b"City, State: ";

/// Prompt for the user's phone number during registration. Verbatim
/// from `amiexpress/express.e:30204`.
pub(crate) const PHONE_PROMPT: &[u8] = b"Phone Number: ";

/// Prompt for the user's email address during registration. Verbatim
/// from `amiexpress/express.e:30215`.
pub(crate) const EMAIL_PROMPT: &[u8] = b"E-Mail Address: ";

/// First password prompt during registration. Verbatim from
/// `amiexpress/express.e:30227`.
pub(crate) const REGISTRATION_PASSWORD_PROMPT: &[u8] = b"Enter a PassWord: ";

/// Confirmation password prompt during registration. Verbatim from
/// `amiexpress/express.e:30233`.
pub(crate) const REGISTRATION_PASSWORD_CONFIRM_PROMPT: &[u8] = b"Reenter the PassWord: ";

/// Prompt asking the user for their preferred line length. Simplified
/// from `amiexpress/express.e:11307` (which streams a 70..2 ladder
/// before asking).
pub(crate) const LINE_LENGTH_PROMPT: &[u8] = b"Enter line length (or 0 for Auto): ";

/// Prompt asking whether the user wants ANSI graphics, asked at connect
/// before the name prompt. Simplified from
/// `amiexpress/express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?` —
/// RIP is dropped, so the choice collapses to ANSI (default) vs. ASCII.
/// An answer beginning `n`/`N` selects ASCII and turns the terminal's
/// live colour mode off, so subsequent screens render with ANSI SGR
/// stripped.
pub(crate) const ANSI_PROMPT: &[u8] = b"ANSI Graphics (Y/n)? ";

/// Prompt for the sysop-set new-user password gate. Verbatim from
/// `amiexpress/express.e:30018`.
pub(crate) const NEW_USER_PASSWORD_PROMPT: &[u8] = b"Enter New User Password: ";

/// Notice shown when a user must rotate their password before menu
/// entry. Verbatim from `amiexpress/express.e:29805`.
pub(crate) const PASSWORD_RESET_REQUIRED_LINE: &[u8] =
    b"\r\nYour account requires your password to be changed.\r\n\r\n";

/// Prompt for the first forced-reset password entry. Verbatim from
/// `amiexpress/express.e:29808`.
pub(crate) const PASSWORD_RESET_PROMPT: &[u8] = b"Enter New Password: ";

/// Prompt for confirming the forced-reset password. Verbatim from
/// `amiexpress/express.e:29810`.
pub(crate) const PASSWORD_RESET_CONFIRM_PROMPT: &[u8] = b"Reenter New Password: ";

/// The invariant tail of the menu prompt rendered by
/// `render_menu_prompt` — `mins. left): ` (Tier A quickwin A4). The
/// leading BBS name, conference block and minute count vary per
/// session, but this suffix is constant, so it is the marker tests
/// drain on to detect "the menu is awaiting a command". Test-only: the
/// menu loop renders the full prompt via `render_menu_prompt` rather
/// than referencing this constant.
#[cfg(test)]
pub(crate) const MENU_PROMPT_SUFFIX: &[u8] = b"mins. left): ";

/// Two-line copyright block printed on every accepted connection,
/// directly after the BBS title banner. The `NextExpress` line sits
/// above the `AmiExpress` line to make the lineage obvious; the
/// `AmiExpress` line mirrors the original BBS's banner verbatim
/// (`amiexpress/express.e:25690`, modulo the legacy file's mojibake of
/// the © glyph).
///
/// The `NextExpress` version slot carries the short git SHA the
/// `build.rs` script captures into `NEXTEXPRESS_GIT_SHA` — pinning the
/// running binary to a specific source commit beats `Cargo.toml`'s
/// long-lived `0.1.0` placeholder for a project that ships continuously.
pub(crate) const COPYRIGHT_LINES: &[u8] = concat!(
    "NextExpress (",
    env!("NEXTEXPRESS_GIT_SHA"),
    ") Copyright \u{00A9}2026\r\n",
    "AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n",
)
.as_bytes();

/// Full response to the `VER` menu command (Tier A quickwin A2),
/// mirroring `internalCommandVER()` at
/// `amiexpress/express.e:25688-25698`.
///
/// The legacy emits an `AmiExpress <ver> (<date>) Copyright ©2018-2023 Darren
/// Coles` header, an `Original Version:` label, the two original-author lines
/// (Thomas, Hodge), and a `Registered to <key>.` line.
///
/// `NextExpress` doesn't carry an `AmiExpress` build at runtime, so the
/// banner leads with `NextExpress <version> (<sha>) Copyright ©2026 Paul
/// Ingles`, followed by the stable `AmiExpress 5` lineage. The `Registered to`
/// line is deliberately elided — see `slices/cmds-quickwins.md` (A2 Out of
/// Scope).
pub(crate) const VERSION_BANNER: &[u8] = concat!(
    "\r\n",
    "NextExpress ",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("NEXTEXPRESS_GIT_SHA"),
    ") Copyright \u{00A9}2026 Paul Ingles\r\n",
    "\r\n",
    "Based on Versions:\r\n",
    "  AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n",
    "  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n",
    "  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n",
    "\r\n",
)
.as_bytes();

/// Sent when the `H` command (Tier A quickwin A5) can't find a
/// `BBSHelp.txt` asset on disk. Verbatim from
/// `amiexpress/express.e:25083`'s `\b\n\b\nSorry Help is unavailable
/// at this time.\b\n\b\n` (Amiga `\b\n` → telnet `\r\n`).
pub(crate) const HELP_UNAVAILABLE_LINE: &[u8] =
    b"\r\n\r\nSorry Help is unavailable at this time.\r\n\r\n";

/// Sent when the `Q` command (Tier A quickwin A9) flips the session
/// into quiet mode. Verbatim from `amiexpress/express.e:25509`'s
/// `\b\nQuiet Mode On\b\n` (Amiga `\b\n` → telnet `\r\n`).
pub(crate) const QUIET_MODE_ON_LINE: &[u8] = b"\r\nQuiet Mode On\r\n";

/// Sent when the `Q` command (Tier A quickwin A9) flips the session
/// back out of quiet mode. Verbatim from
/// `amiexpress/express.e:25511`'s `\b\nQuiet Mode Off\b\n`.
pub(crate) const QUIET_MODE_OFF_LINE: &[u8] = b"\r\nQuiet Mode Off\r\n";

/// Sent when the `X` command (Tier A quickwin A6) turns expert mode on.
/// Verbatim from `amiexpress/express.e:26118`'s
/// `\b\nExpert mode enabled\b\n` (Amiga `\b\n` → telnet `\r\n`).
pub(crate) const EXPERT_MODE_ENABLED_LINE: &[u8] = b"\r\nExpert mode enabled\r\n";

/// Sent when the `X` command (Tier A quickwin A6) turns expert mode
/// off. Verbatim from `amiexpress/express.e:26115`'s
/// `\b\nExpert mode disabled\b\n`.
pub(crate) const EXPERT_MODE_DISABLED_LINE: &[u8] = b"\r\nExpert mode disabled\r\n";

/// Sent when the `M` command (Tier A quickwin A8) turns ANSI colour on.
/// Verbatim from `amiexpress/express.e:25247`'s
/// `\b\nAnsi Color On\b\n` (Amiga `\b\n` → telnet `\r\n`).
pub(crate) const ANSI_COLOR_ON_LINE: &[u8] = b"\r\nAnsi Color On\r\n";

/// Sent when the `M` command (Tier A quickwin A8) turns ANSI colour
/// off. Verbatim from `amiexpress/express.e:25243`'s
/// `\b\nAnsi Color Off\b\n`.
pub(crate) const ANSI_COLOR_OFF_LINE: &[u8] = b"\r\nAnsi Color Off\r\n";

/// Sent when the typed handle is `NEW` (reserved) or already taken
/// during registration. Followed by a fresh handle prompt.
pub(crate) const HANDLE_TAKEN_LINE: &[u8] = b"That name is taken. Try another.\r\n";

/// Sent when the user has burned through five handle retries during
/// registration.
pub(crate) const REGISTRATION_RETRIES_EXHAUSTED_LINE: &[u8] =
    b"Too many failed registration attempts. Goodbye.\r\n";

/// Sent when the two registration passwords don't match. Verbatim from
/// `amiexpress/express.e:30237`.
pub(crate) const PASSWORDS_DO_NOT_MATCH_LINE: &[u8] =
    b"\r\nPasswords do not match, try again..\r\n";

/// Sent when the two forced-reset password entries don't match.
/// Verbatim from `amiexpress/express.e:29835`.
pub(crate) const PASSWORD_RESET_MISMATCH_LINE: &[u8] =
    b"\r\nPasswords do not match, please try again.\r\n\r\n";

/// Sent when the forced-reset candidate matches the current password.
/// Verbatim from `amiexpress/express.e:29813`.
pub(crate) const PASSWORD_RESET_SAME_AS_CURRENT_LINE: &[u8] =
    b"\r\nYour new password must be different from your old password...\r\n\r\n";

/// Sent when the forced-reset candidate fails the configured password
/// strength policy. The legacy distinguishes length vs category
/// failures, but the app-layer rule currently reports a single weak
/// password error.
pub(crate) const PASSWORD_RESET_WEAK_LINE: &[u8] = b"\r\nInvalid PassWord\r\n";

/// Sent when the user exhausts forced-reset attempts without changing
/// their password. Verbatim from `amiexpress/express.e:29841`.
pub(crate) const PASSWORD_RESET_EXHAUSTED_LINE: &[u8] =
    b"\r\nYou have not updated your password so you will now be disconnected...\r\n\r\n";

/// Sent when the line-length input doesn't parse as a number in
/// `0..=255`.
pub(crate) const INVALID_LINE_LENGTH_LINE: &[u8] = b"Invalid line length.\r\n";

/// Sent after the registration succeeds; immediately followed by the
/// menu sequence inherited by every authenticated session.
pub(crate) const REGISTRATION_COMPLETE_LINE: &[u8] = b"\r\nWelcome aboard!\r\n";

/// Sent after each failed new-user password attempt. Verbatim from
/// `amiexpress/express.e:30036`.
pub(crate) const NEW_USER_INVALID_PASSWORD_LINE: &[u8] = b"Invalid PassWord\r\n";

/// Sent when the gate's retry budget is exhausted. Verbatim from
/// `amiexpress/express.e:30039`. Followed by a goodbye line.
pub(crate) const NEW_USER_EXCESSIVE_FAILURES_LINE: &[u8] =
    b"\r\nExcessive Password Failure\r\nGoodbye.\r\n";

/// Sent on a successful gate match. Verbatim from
/// `amiexpress/express.e:30046`.
pub(crate) const NEW_USER_PASSWORD_OK_LINE: &[u8] = b"Correct\r\n";

/// Sent for unrecognised menu commands.
pub(crate) const UNKNOWN_COMMAND_LINE: &[u8] = b"Unknown command. Type G to log off.\r\n";

/// Sent immediately before the connection closes on a normal logoff.
pub(crate) const GOODBYE_LINE: &[u8] = b"Goodbye!\r\n";

/// The `checkFlagged()` leave-confirm prompt
/// (`amiexpress/express.e:12670`) followed by `yesNo(2)`'s own ANSI
/// `(y/N)? ` suffix (`:2134`). Server bytes, live-captured
/// (`comparison/transcripts/ae_tierd_g_confirm.txt:146`); the legacy
/// `\b\n` line breaks are re-encoded to telnet `\r\n` (AGENTS.md wire
/// policy).
pub(crate) const LEAVE_FLAGGED_CONFIRM: &[u8] =
    b"\r\nYou have flagged files still not downloaded.\r\nDo you leave without them? \x1b[32m(\x1b[33my\x1b[32m/\x1b[33mN\x1b[32m)\x1b[32m?\x1b[0m ";

/// `yesNo`'s single-key echo on a `Y` answer (`amiexpress/express.e:2148`).
pub(crate) const YESNO_YES_ECHO: &[u8] = b"Yes\r\n";

/// `yesNo`'s single-key echo on an `N` / default answer
/// (`amiexpress/express.e:2152`).
pub(crate) const YESNO_NO_ECHO: &[u8] = b"No\r\n";

/// Sent immediately before the connection closes on idle timeout.
pub(crate) const IDLE_TIMEOUT_LINE: &[u8] = b"Idle timeout. Goodbye.\r\n";

/// Sent when the post-auth cluster rejects the logon for insufficient
/// access.
pub(crate) const LOGON_REJECTED_LINE: &[u8] = b"Logon rejected. Goodbye.\r\n";

/// Sent when the auto-rejoin / explicit-join flow can't find any
/// conference the user has access to (Slice 30 / Slice 34a). The
/// session terminates with `LogoffReason::NoConferenceAccess`.
pub(crate) const NO_CONFERENCE_ACCESS_LINE: &[u8] = b"\r\nNo accessible conferences. Goodbye.\r\n";

/// Sent when `R <something>` cannot be parsed as a message number.
pub(crate) const INVALID_MESSAGE_NUMBER_LINE: &[u8] = b"\r\nInvalid message number.\r\n";

/// `time::macros::format_description!` builds a const `FormatItem`
/// slice describing the legacy `FORMAT_USA` date-time layout —
/// `MM-DD-YY HH:MM:SS` (`amiexpress/express.e:25636-25640`).
const TIME_FORMAT: &[time::format_description::FormatItem<'_>] = time::macros::format_description!(
    "[month]-[day]-[year repr:last_two] [hour]:[minute]:[second]"
);

/// Formats the response to the `T` menu command (Tier A — quickwin):
/// the legacy "It is " prefix followed by date and time, wrapped in
/// CRLFs. Mirrors `internalCommandT()` at
/// `amiexpress/express.e:25622-25644`.
///
/// The legacy uses `AmigaOS`'s `DateToStr` with `FORMAT_USA`, which
/// produces a two-digit-year `MM-DD-YY` date and `HH:MM:SS` time.
/// Time is rendered in UTC; the legacy used the Amiga's local
/// `DateStamp()`, but `NextExpress` doesn't yet have a per-deployment
/// timezone setting — landing local-offset support is a future
/// refinement, not a parity break in the visible literal.
pub(crate) fn render_time_line(at: std::time::SystemTime) -> Vec<u8> {
    let formatted = time::OffsetDateTime::from(at)
        .format(TIME_FORMAT)
        .expect("TIME_FORMAT is total over OffsetDateTime");
    format!("\r\nIt is {formatted}\r\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyright_lines_wrap_build_git_sha_in_parens() {
        // The banner shown on connect must reflect the source commit
        // the binary was built from. `build.rs` captures
        // `git rev-parse --short HEAD` into `NEXTEXPRESS_GIT_SHA`; the
        // wire format wraps it in parentheses (`NextExpress (sha)
        // Copyright ©…`) so the build identifier is visually distinct
        // from the product name.
        let sha = env!("NEXTEXPRESS_GIT_SHA");
        assert!(
            !sha.is_empty(),
            "build script must capture a non-empty git SHA",
        );
        let copyright = std::str::from_utf8(COPYRIGHT_LINES).expect("utf8 copyright");
        let needle = format!("NextExpress ({sha}) Copyright");
        assert!(
            copyright.contains(&needle),
            "expected `{needle}` in copyright lines: {copyright:?}",
        );
    }

    #[test]
    fn version_banner_carries_lineage_lines_verbatim() {
        // Pin the lineage block so a future edit can't quietly drift
        // the wording. Each line is checked individually so a swap or
        // reorder fails the test.
        let banner = std::str::from_utf8(VERSION_BANNER).expect("utf8 banner");
        assert!(
            banner.contains("Based on Versions:\r\n"),
            "missing lineage label: {banner:?}",
        );
        assert!(
            banner.contains("AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n"),
            "missing AmiExpress copyright line: {banner:?}",
        );
        assert!(
            banner.contains("  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n"),
            "missing Thomas author line: {banner:?}",
        );
        assert!(
            banner.contains("  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n"),
            "missing Hodge author line: {banner:?}",
        );
    }

    #[test]
    fn version_banner_carries_nextexpress_version_and_sha() {
        // Slice A2: the leading line pins the running Rust port to
        // its `Cargo.toml` version + `build.rs` SHA so the operator
        // can correlate a running session with a specific build.
        let banner = std::str::from_utf8(VERSION_BANNER).expect("utf8 banner");
        let version = env!("CARGO_PKG_VERSION");
        let sha = env!("NEXTEXPRESS_GIT_SHA");
        let needle =
            format!("NextExpress {version} ({sha}) Copyright \u{00A9}2026 Paul Ingles\r\n");
        assert!(
            banner.contains(&needle),
            "expected `{needle}` in banner: {banner:?}",
        );
    }

    #[test]
    fn version_banner_starts_with_crlf_and_omits_registration_key_line() {
        // Slice A2 (Out of Scope): the legacy `Registered to <key>.`
        // line (`amiexpress/express.e:25696`) is deliberately elided.
        let banner = std::str::from_utf8(VERSION_BANNER).expect("utf8 banner");
        assert!(
            banner.starts_with("\r\n"),
            "banner missing CRLF prefix: {banner:?}"
        );
        assert!(
            !banner.contains("Registered to"),
            "banner must elide the legacy `Registered to` line: {banner:?}",
        );
    }

    #[test]
    fn render_time_line_emits_legacy_it_is_prefix_and_us_format() {
        // Pin the legacy `It is <MM-DD-YY> <HH:MM:SS>` wire format
        // (`amiexpress/express.e:25636-25640`, FORMAT_USA). 1970-01-02
        // 03:04:05 UTC is the chosen fixed instant so all fields are
        // distinct two-digit numbers — any swap of fields shows up
        // immediately in the assertion.
        use std::time::{Duration, UNIX_EPOCH};
        let at = UNIX_EPOCH + Duration::from_secs(86_400 + 3 * 3600 + 4 * 60 + 5);
        assert_eq!(render_time_line(at), b"\r\nIt is 01-02-70 03:04:05\r\n");
    }

    #[test]
    fn render_time_line_zero_pads_single_digit_fields() {
        // FORMAT_USA pads every numeric field to two digits; a leading
        // zero is required for `09` and the like.
        use std::time::{Duration, UNIX_EPOCH};
        // 1970-01-01 00:00:00 UTC — every field is `00`.
        let at = UNIX_EPOCH + Duration::from_secs(0);
        assert_eq!(render_time_line(at), b"\r\nIt is 01-01-70 00:00:00\r\n");
    }

    #[test]
    fn render_time_line_uses_two_digit_year_wrap_after_2000() {
        // FORMAT_USA on AmigaOS produces a two-digit year; 2001 must
        // render as `01`, not `2001`. The Unix billennium (1e9 seconds
        // past the epoch) is 2001-09-09 01:46:40 UTC — a widely-known
        // reference instant.
        use std::time::{Duration, UNIX_EPOCH};
        let at = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert_eq!(render_time_line(at), b"\r\nIt is 09-09-01 01:46:40\r\n");
    }
}
