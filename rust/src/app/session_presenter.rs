//! Rendering helpers shared by the workflow sub-flows.
//!
//! The login, registration and menu flows each own their own state
//! machine, but several wire-format decisions show up in more than
//! one place — the conference-join announcement is produced by the
//! auto-rejoin path (after sign-in) and the explicit-join path (from
//! the menu); the name-type promotion screen is shown after both.
//! Keeping the rendering in one module means changes to the wire
//! shape land in a single file and the sub-flows stay focused on
//! their own state transitions.

use crate::app::screens::ScreenRepository;
use crate::app::terminal::Terminal;
use crate::domain::conference::{Conference, NameType};

/// Resolves `(conference_name, msgbase_name)` for the wire-format
/// helpers. The `msgbase_name` is `Some(_)` only when the
/// conference holds more than one message base, mirroring the
/// `getConfMsgBaseCount(conf)>1` branch in legacy `joinConf` (the
/// [`Conference::disambiguating_msgbase_name`] rule).
#[must_use]
pub(crate) fn resolve_conference_strings(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> (&str, Option<&str>) {
    let Some(conference) = conferences.iter().find(|c| c.number() == conference_number) else {
        return ("?", None);
    };
    (
        conference.name(),
        conference.disambiguating_msgbase_name(msgbase_number),
    )
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline auto-rejoin announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5071-5073`). Returns just the
/// conference-name segment when the lookup fails, which is
/// defensive — `auto_rejoin_conference` only reports
/// `conference_number`s that came from the catalogue.
#[must_use]
pub(crate) fn format_auto_rejoin_line(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    auto_rejoin_line(conference_number, conference_name, msgbase_name)
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline explicit-join announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5079-5083`).
#[must_use]
pub(crate) fn format_explicit_join_line(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    explicit_join_line(conference_name, msgbase_name)
}

/// Resolves the conference label and renders the menu prompt
/// (Tier A quickwin A4, the default branch of `displayMenuPrompt` at
/// `amiexpress/express.e:28413-28421`).
///
/// For a multi-msgbase conference the label is `"<name> - <msgbase>"`,
/// matching the legacy `StringF(tempstr,'\s - \s',...)` at
/// `:28416`; otherwise it is just the conference name. `current` is the
/// open visit's `(conference_number, msgbase_number)`, or `None` for a
/// menu session with no open conference — which renders the prompt
/// without the `[<num>:<label>]` segment.
///
/// `time_remaining` is the session's per-call budget; the displayed
/// minute count is `time_remaining.as_secs() / 60` (whole minutes,
/// truncated), mirroring the legacy `Div((timeTotal - timeUsed), 60)`
/// at `amiexpress/express.e:28417`.
#[must_use]
pub(crate) fn format_menu_prompt(
    bbs_name: &str,
    conferences: &[Conference],
    current: Option<(u32, u32)>,
    time_remaining: std::time::Duration,
) -> Vec<u8> {
    let mins_left = time_remaining.as_secs() / 60;
    let label = current.map(|(conference_number, msgbase_number)| {
        let (name, msgbase_name) =
            resolve_conference_strings(conferences, conference_number, msgbase_number);
        let label = match msgbase_name {
            Some(msgbase) => format!("{name} - {msgbase}"),
            None => name.to_string(),
        };
        (conference_number, label)
    });
    render_menu_prompt(
        bbs_name,
        label
            .as_ref()
            .map(|(number, label)| (*number, label.as_str())),
        mins_left,
    )
}

/// Renders `SCREEN_REALNAMES` / `SCREEN_INTERNETNAMES` when a join
/// promoted the session's `display_name_type` (Slice 34).
pub(crate) async fn render_name_type_promotion<T, S>(
    terminal: &mut T,
    screens: &S,
    promoted: Option<NameType>,
) -> Result<(), T::Error>
where
    T: Terminal + ?Sized,
    S: ScreenRepository + ?Sized,
{
    let bytes = match promoted {
        Some(NameType::RealName) => screens.realnames_screen().await,
        Some(NameType::InternetName) => screens.internetnames_screen().await,
        Some(NameType::Handle) | None => return Ok(()),
    };
    terminal.write(&bytes).await?;
    terminal.flush().await
}

/// `formatLongDateTime`'s `DD-MMM-YYYY HH:MM:SS` layout
/// (`amiexpress/ACP.e:549-566`): a `FORMAT_DOS` day-month-year date with
/// the century prepended — e.g. `09-Sep-2001 01:46:40`.
const STATS_DATE_FORMAT: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[day]-[month repr:short]-[year] [hour]:[minute]:[second]");

/// Emits one `[32m<label>[33m:[0m <value>` stats line, terminated with
/// a telnet CRLF. The label is written verbatim — callers pad it to the
/// legacy's fixed 11-character column width.
fn write_stat_line(out: &mut Vec<u8>, label: &[u8], value: &str) {
    out.extend_from_slice(b"\x1b[32m");
    out.extend_from_slice(label);
    out.extend_from_slice(b"\x1b[33m:\x1b[0m ");
    out.extend_from_slice(value.as_bytes());
    out.extend_from_slice(b"\r\n");
}

/// Formats the `S` user-stats screen (Tier A quickwin A3).
///
/// Renders the baseline lines of `internalCommandS()`
/// (`amiexpress/express.e:25540-25578`) — the subset whose fields exist
/// on `User` today — in the legacy order, each label padded to 11
/// characters and wrapped in the `[32m…[33m:[0m ` ANSI prefixes. The
/// block is bracketed by a leading and trailing CRLF, matching the
/// legacy's opening `aePuts('\b\n')` and closing blank line.
///
/// The config-gated `Area Name` / `Caller Num.` lines and the
/// Tier-I-only rows (`Online Baud`, CPS rates, credit, sysop
/// availability, file ratios) are deferred to slice A11.
///
/// # Parameters
/// - `slot_number`: the user's account slot (legacy `User Number`).
/// - `last_call`: the most recent completed logon, or `None` for a
///   user who has never called — rendered as the Unix epoch, mirroring
///   the legacy's `timeLastOn = 0` "never" sentinel.
/// - `security_level`: the user's access level (legacy `Security Lv`).
/// - `times_called`: lifetime completed logons (`# Times On`).
/// - `times_called_today`: logons in the current accounting day.
/// - `messages_posted`: lifetime messages posted.
///
/// # Returns
/// The wire bytes of the stats screen, ready to write to the terminal.
pub(crate) fn render_stats_screen(
    slot_number: u32,
    last_call: Option<std::time::SystemTime>,
    security_level: u8,
    times_called: u32,
    times_called_today: u32,
    messages_posted: u32,
) -> Vec<u8> {
    let last_on =
        time::OffsetDateTime::from(last_call.unwrap_or(std::time::SystemTime::UNIX_EPOCH))
            .format(STATS_DATE_FORMAT)
            .expect("STATS_DATE_FORMAT is total over OffsetDateTime");
    let mut out = Vec::with_capacity(220);
    out.extend_from_slice(b"\r\n");
    write_stat_line(&mut out, b"User Number", &slot_number.to_string());
    write_stat_line(&mut out, b"Lst Date On", &last_on);
    write_stat_line(&mut out, b"Security Lv", &security_level.to_string());
    write_stat_line(&mut out, b"# Times On ", &times_called.to_string());
    write_stat_line(&mut out, b"Times Today", &times_called_today.to_string());
    write_stat_line(&mut out, b"Msgs Posted", &messages_posted.to_string());
    out.extend_from_slice(b"\r\n");
    out
}

/// Renders the menu prompt (Tier A quickwin A4), mirroring the default
/// branch of `displayMenuPrompt()` (`amiexpress/express.e:28413-28421`):
///
/// ```text
///   <bbsName> [<confNum>:<confLabel>] Menu (<mins> mins. left):
/// ```
///
/// with the legacy ANSI colour run (`[35m` magenta name, `[36m` cyan
/// number / label, `[34m` blue separator, `[33m` yellow minutes). The
/// prompt has a trailing space and no CRLF — the user types their
/// command on the same line.
///
/// The sysop-supplied custom-prompt (MCI) branch
/// (`amiexpress/express.e:28409-28412`) is deferred.
///
/// # Parameters
/// - `bbs_name`: the configured BBS name (legacy `cmds.bbsName`).
/// - `conference`: the `(number, label)` of the open conference, where
///   `label` is the conference name (optionally `"<name> - <msgbase>"`
///   for multi-msgbase conferences). `None` for the defensive case
///   where a menu session has no open conference, which renders the
///   prompt without the `[<num>:<label>]` segment.
/// - `mins_left`: per-call minutes remaining, `(timeTotal -
///   timeUsed) / 60`.
///
/// # Returns
/// The wire bytes of the prompt, ready to write to the terminal.
fn render_menu_prompt(bbs_name: &str, conference: Option<(u32, &str)>, mins_left: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(bbs_name.len() + 64);
    out.extend_from_slice(b"\x1b[0m\x1b[35m");
    out.extend_from_slice(bbs_name.as_bytes());
    out.extend_from_slice(b" \x1b[0m");
    if let Some((number, label)) = conference {
        out.extend_from_slice(b"[\x1b[36m");
        out.extend_from_slice(number.to_string().as_bytes());
        out.extend_from_slice(b"\x1b[34m:\x1b[36m");
        out.extend_from_slice(label.as_bytes());
        out.extend_from_slice(b"\x1b[0m] ");
    }
    out.extend_from_slice(b"Menu (\x1b[33m");
    out.extend_from_slice(mins_left.to_string().as_bytes());
    out.extend_from_slice(b"\x1b[0m mins. left): ");
    out
}

/// Formats the auto-rejoin announcement (Slice 30 / Slice 34a).
/// Mirrors the legacy `joinConf` output at
/// `amiexpress/express.e:5071-5073`:
///
/// ```text
///   Conference <n>: <name> Auto-ReJoined          (single msgbase)
///   Conference <n>: <name> [<msgbase>] Auto-ReJoined (multiple msgbases)
/// ```
///
/// `\b\n` in the legacy source becomes telnet `\r\n` on the wire
/// (lines 5065 and 5088 wrap the announcement with one CRLF on
/// either side). `msgbase_name` is `Some(_)` only when the
/// conference holds more than one message base, mirroring the
/// `getConfMsgBaseCount(conf)>1` branch in the legacy.
fn auto_rejoin_line(
    conference_number: u32,
    conference_name: &str,
    msgbase_name: Option<&str>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(conference_name.len() + 32);
    out.extend_from_slice(b"\r\nConference ");
    out.extend_from_slice(conference_number.to_string().as_bytes());
    out.extend_from_slice(b": ");
    out.extend_from_slice(conference_name.as_bytes());
    if let Some(mb) = msgbase_name {
        out.extend_from_slice(b" [");
        out.extend_from_slice(mb.as_bytes());
        out.push(b']');
    }
    out.extend_from_slice(b" Auto-ReJoined\r\n");
    out
}

/// Formats the explicit-join announcement (Slice 32 / Slice 34a).
/// Mirrors the legacy `joinConf` output at
/// `amiexpress/express.e:5079-5083`:
///
/// ```text
///   <ESC>[32mJoining Conference<ESC>[33m:<ESC>[0m <name>
///   <ESC>[32mJoining Conference<ESC>[33m:<ESC>[0m <name> [<msgbase>]
/// ```
///
/// The ANSI colour escapes are emitted verbatim — the legacy
/// listener (and ours, by way of `aePuts`) writes them to the wire
/// when `ansiColour` is true; clients without colour rendering
/// still receive the readable text in between.
fn explicit_join_line(conference_name: &str, msgbase_name: Option<&str>) -> Vec<u8> {
    let mut out = Vec::with_capacity(conference_name.len() + 48);
    out.extend_from_slice(b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m ");
    out.extend_from_slice(conference_name.as_bytes());
    if let Some(mb) = msgbase_name {
        out.extend_from_slice(b" [");
        out.extend_from_slice(mb.as_bytes());
        out.push(b']');
    }
    out.extend_from_slice(b"\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::conference::{Conference, MessageBase};

    #[test]
    fn resolve_conference_strings_returns_name_only_for_single_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = false` branch in
        // legacy `joinConf` (`amiexpress/express.e:5072`): the
        // announcement omits the `[<msgbase>]` segment.
        let confs = vec![Conference::new(
            7,
            "Solo".to_string(),
            vec![MessageBase::new(7, 1, "main".to_string())],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 7, 1);
        assert_eq!(name, "Solo");
        assert!(
            mb.is_none(),
            "single-msgbase conferences should not include a msgbase name"
        );
    }

    #[test]
    fn resolve_conference_strings_emits_msgbase_for_multi_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = true` branch in
        // legacy `joinConf` (`amiexpress/express.e:5070`): the
        // announcement carries `[<msgbase>]`.
        let confs = vec![Conference::new(
            3,
            "Tech-and-misc".to_string(),
            vec![
                MessageBase::new(3, 1, "main".to_string()),
                MessageBase::new(3, 2, "tech".to_string()),
            ],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 3, 2);
        assert_eq!(name, "Tech-and-misc");
        assert_eq!(mb, Some("tech"));
    }

    #[test]
    fn format_menu_prompt_uses_bare_conference_name_for_single_msgbase() {
        // Tier A quickwin A4: a single-msgbase conference renders just
        // its name in the `[<num>:<label>]` segment.
        let confs = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        // 58m45s of budget displays as a truncated `58` minutes,
        // pinning `as_secs() / 60` against the `%` / `*` mutants.
        assert_eq!(
            format_menu_prompt(
                "NextExpress",
                &confs,
                Some((1, 1)),
                std::time::Duration::from_secs(58 * 60 + 45)
            ),
            &b"\x1b[0m\x1b[35mNextExpress \x1b[0m[\x1b[36m1\x1b[34m:\x1b[36mMain\x1b[0m] Menu (\x1b[33m58\x1b[0m mins. left): "[..],
        );
    }

    #[test]
    fn format_menu_prompt_appends_msgbase_for_multi_msgbase_conference() {
        // Mirrors the legacy `StringF(tempstr,'\s - \s',confName,
        // msgBaseName)` label at `amiexpress/express.e:28416`.
        let confs = vec![Conference::new(
            3,
            "Programming".to_string(),
            vec![
                MessageBase::new(3, 1, "main".to_string()),
                MessageBase::new(3, 2, "tech".to_string()),
            ],
        )
        .expect("valid")];
        assert_eq!(
            format_menu_prompt(
                "NextExpress",
                &confs,
                Some((3, 2)),
                std::time::Duration::from_secs(42 * 60 + 10)
            ),
            &b"\x1b[0m\x1b[35mNextExpress \x1b[0m[\x1b[36m3\x1b[34m:\x1b[36mProgramming - tech\x1b[0m] Menu (\x1b[33m42\x1b[0m mins. left): "[..],
        );
    }

    #[test]
    fn format_menu_prompt_without_conference_omits_the_bracket() {
        // Defensive: a menu session with no open conference renders the
        // prompt without the `[<num>:<label>]` segment.
        assert_eq!(
            format_menu_prompt("NextExpress", &[], None, std::time::Duration::ZERO),
            &b"\x1b[0m\x1b[35mNextExpress \x1b[0mMenu (\x1b[33m0\x1b[0m mins. left): "[..],
        );
    }

    #[test]
    fn resolve_conference_strings_returns_question_mark_for_unknown_conference() {
        // Defensive fallback: a conference number that's not in the
        // catalogue produces "?". Today this is unreachable (the
        // resolver only reports numbers that came from the
        // catalogue) but the helper has to be total.
        let (name, mb) = resolve_conference_strings(&[], 99, 1);
        assert_eq!(name, "?");
        assert!(mb.is_none());
    }

    #[test]
    fn auto_rejoin_line_single_msgbase_matches_legacy_format() {
        // `amiexpress/express.e:5073` —
        // `Conference \d: \s Auto-ReJoined` wrapped with CRLFs.
        assert_eq!(
            auto_rejoin_line(1, "Main", None),
            b"\r\nConference 1: Main Auto-ReJoined\r\n",
        );
    }

    #[test]
    fn auto_rejoin_line_includes_msgbase_when_supplied() {
        // `amiexpress/express.e:5071` — `\d: \s [\s] Auto-ReJoined`.
        assert_eq!(
            auto_rejoin_line(2, "Programming", Some("tech")),
            b"\r\nConference 2: Programming [tech] Auto-ReJoined\r\n",
        );
    }

    #[test]
    fn explicit_join_line_single_msgbase_matches_legacy_ansi_format() {
        // `amiexpress/express.e:5083` — ESC sequences carry colour;
        // text is `Joining Conference: <name>`.
        assert_eq!(
            explicit_join_line("Main", None),
            b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Main\r\n",
        );
    }

    #[test]
    fn explicit_join_line_includes_msgbase_when_supplied() {
        // `amiexpress/express.e:5079` — multi-msgbase variant.
        assert_eq!(
            explicit_join_line("Programming", Some("tech")),
            b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Programming [tech]\r\n",
        );
    }

    #[test]
    fn render_stats_screen_emits_legacy_baseline_lines() {
        // Tier A quickwin A3 (`S`): the baseline stats block from
        // `internalCommandS()` (`amiexpress/express.e:25540-25578`),
        // restricted to the lines whose fields exist on `User` today.
        // Each label is padded to 11 chars and wrapped in the legacy
        // `[32m…[33m:[0m ` ANSI prefixes; `Lst Date On` uses
        // `formatLongDateTime`'s `DD-MMM-YYYY HH:MM:SS` layout.
        //
        // `1_000_000_000` seconds past the Unix epoch is 2001-09-09
        // 01:46:40 UTC — a fixed, well-known instant.
        let last_call =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        assert_eq!(
            render_stats_screen(1, Some(last_call), 255, 42, 3, 7),
            &b"\r\n\
\x1b[32mUser Number\x1b[33m:\x1b[0m 1\r\n\
\x1b[32mLst Date On\x1b[33m:\x1b[0m 09-Sep-2001 01:46:40\r\n\
\x1b[32mSecurity Lv\x1b[33m:\x1b[0m 255\r\n\
\x1b[32m# Times On \x1b[33m:\x1b[0m 42\r\n\
\x1b[32mTimes Today\x1b[33m:\x1b[0m 3\r\n\
\x1b[32mMsgs Posted\x1b[33m:\x1b[0m 7\r\n\
\r\n"[..],
        );
    }

    #[test]
    fn render_stats_screen_renders_never_called_user_as_unix_epoch() {
        // A user who has never completed a logon has `last_call =
        // None` (the seeded sysop's state). The legacy stores
        // `timeLastOn = 0` for "never" and formats it as the epoch;
        // we mirror that by rendering the Unix epoch.
        let screen = render_stats_screen(1, None, 255, 0, 0, 0);
        let text = std::str::from_utf8(&screen).expect("utf8");
        assert!(
            text.contains("Lst Date On\x1b[33m:\x1b[0m 01-Jan-1970 00:00:00\r\n"),
            "expected epoch date for never-called user, got: {text:?}",
        );
    }

    #[test]
    fn render_menu_prompt_matches_legacy_default_format() {
        // Tier A quickwin A4 (menu-prompt parity): the default
        // `displayMenuPrompt` format at `amiexpress/express.e:28419` —
        // `<bbsName> [<confNum>:<confName>] Menu (<mins> mins. left): `
        // with the legacy ANSI colour run. `<mins>` is
        // `(timeTotal - timeUsed) / 60`.
        assert_eq!(
            render_menu_prompt("NextExpress", Some((1, "Main")), 58),
            &b"\x1b[0m\x1b[35mNextExpress \x1b[0m[\x1b[36m1\x1b[34m:\x1b[36mMain\x1b[0m] Menu (\x1b[33m58\x1b[0m mins. left): "[..],
        );
    }

    #[test]
    fn render_menu_prompt_without_conference_omits_the_bracket_segment() {
        // Defensive case: a menu session with no open conference
        // (e.g. a user with no conference access). The legacy always
        // has a current conference, so this is a NextExpress fallback —
        // the `[<num>:<label>]` segment is dropped, the rest is intact.
        assert_eq!(
            render_menu_prompt("NextExpress", None, 0),
            &b"\x1b[0m\x1b[35mNextExpress \x1b[0mMenu (\x1b[33m0\x1b[0m mins. left): "[..],
        );
    }
}
