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

/// Prompt sent before reading the user's handle. Mirrors the original
/// `AmiExpress` wire format: a CRLF prefix and trailing space around the
/// default `NAME_PROMPT` of `Enter your Name:` (see
/// `amiexpress/express.e:29571` and `:31774`).
pub(crate) const NAME_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's password.
pub(crate) const PASSWORD_PROMPT: &[u8] = b"PassWord: ";

/// Prompt asking a registering user for the handle they want.
/// Mirrors the wire format of [`NAME_PROMPT`] (CRLF prefix, trailing
/// space) — `amiexpress/express.e:30141`.
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
/// [`render_menu_prompt`] — `mins. left): ` (Tier A quickwin A4). The
/// leading BBS name, conference block and minute count vary per
/// session, but this suffix is constant, so it is the marker tests
/// drain on to detect "the menu is awaiting a command". Test-only: the
/// menu loop renders the full prompt via [`render_menu_prompt`] rather
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

/// Sent after a not-found name lookup to invite a retry.
pub(crate) const UNKNOWN_USER_LINE: &[u8] = b"Unknown user.\r\n";

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

/// Sent when the user has burned through all five name retries.
pub(crate) const TOO_MANY_RETRIES_LINE: &[u8] = b"Too many failed login attempts. Goodbye.\r\n";

/// Sent after a successful authentication.
pub(crate) const AUTHENTICATED_LINE: &[u8] = b"Authenticated.\r\n";

/// Sent when the password didn't match.
pub(crate) const WRONG_PASSWORD_LINE: &[u8] = b"Incorrect password.\r\n";

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

/// Sent when the post-auth cluster locks the account.
pub(crate) const ACCOUNT_LOCKED_LINE: &[u8] = b"Account locked. Goodbye.\r\n";

/// Sent when the per-session retry budget is exhausted at the password
/// prompt.
pub(crate) const TOO_MANY_PASSWORD_FAILURES_LINE: &[u8] =
    b"Too many password failures. Goodbye.\r\n";

/// Sent when the post-auth cluster rejects the logon for insufficient
/// access.
pub(crate) const LOGON_REJECTED_LINE: &[u8] = b"Logon rejected. Goodbye.\r\n";

/// Sent when the auto-rejoin / explicit-join flow can't find any
/// conference the user has access to (Slice 30 / Slice 34a). The
/// session terminates with `LogoffReason::NoConferenceAccess`.
pub(crate) const NO_CONFERENCE_ACCESS_LINE: &[u8] = b"\r\nNo accessible conferences. Goodbye.\r\n";

/// Sent when the user requested a conference they don't have access
/// to (or — defensively — one missing from the catalogue). The legacy
/// `'\b\nYou do not have access to the requested conference\b\n\b\n'`
/// (`amiexpress/express.e:25157`, Amiga `\b\n` becomes telnet
/// `\r\n`); the session stays in its current conference
/// (`amiexpress/express.e:25158` returns to the menu).
pub(crate) const NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE: &[u8] =
    b"\r\nYou do not have access to the requested conference\r\n\r\n";

/// Prompt of the `J` command's interactive join-conference flow
/// (`amiexpress/express.e:25144`):
/// `'Conference Number (1-\d): '` with `\d` = the highest conference
/// number (legacy `cmds.numConf`). Written immediately after the
/// echoed command line — no leading CRLF — and ends with the prompt's
/// trailing space, no trailing CRLF.
pub(crate) fn render_conference_number_prompt(highest_conference_number: u32) -> Vec<u8> {
    format!("Conference Number (1-{highest_conference_number}): ").into_bytes()
}

/// Prompt of the interactive join-message-base flow
/// (`amiexpress/express.e:25224`, identical literal at `:25173` in
/// `internalCommandJ`'s message-base prompt):
/// `'Message Base Number (1-\d): '` with `\d` = the current/target
/// conference's message-base count (legacy `getConfMsgBaseCount`).
/// Written immediately after the echoed command line (or the
/// `JoinMsgBase` screen when installed) — no leading CRLF — and ends
/// with the prompt's trailing space, no trailing CRLF.
pub(crate) fn render_msgbase_number_prompt(msgbase_count: u32) -> Vec<u8> {
    format!("Message Base Number (1-{msgbase_count}): ").into_bytes()
}

/// Sent when `JM` (any non-dotted form, argument or not) is used in a
/// conference holding a single message base. The legacy probes the
/// `NMSGBASES` tooltype and fails before any range logic when it is
/// absent — the normal single-base configuration
/// (`amiexpress/express.e:25211-25215`); the literal is
/// `'\b\nThis conference does not contain multiple message bases\b\n\b\n'`
/// (`amiexpress/express.e:25213`, Amiga `\b\n` becomes telnet `\r\n`).
/// `NextExpress` equates "tooltype absent" with the conference holding
/// exactly one base; the legacy nuance of an explicitly-set
/// `NMSGBASES=1` (which prompts `(1-1)` instead) is deliberately not
/// modelled — recorded in `slices/cmds-conf-nav.md`.
pub(crate) const SINGLE_MSGBASE_CONFERENCE_LINE: &[u8] =
    b"\r\nThis conference does not contain multiple message bases\r\n\r\n";

/// Sent when `R <something>` cannot be parsed as a message number.
pub(crate) const INVALID_MESSAGE_NUMBER_LINE: &[u8] = b"\r\nInvalid message number.\r\n";

/// Prompt shown when the `E` command needs the recipient handle.
/// The legacy `enterMSG` uses the bare `To:` line that
/// `msgToHeader` paints (`amiexpress/express.e:10778`).
pub(crate) const POST_TO_PROMPT: &[u8] = b"\r\nTo: ";

/// Prompt for the subject during line-mode mail composition.
/// Simplified from `amiexpress/express.e:10847` (the legacy form
/// adds an ANSI-coloured "(Blank)=abort?" hint).
pub(crate) const POST_SUBJECT_PROMPT: &[u8] = b"Subject: ";

/// Prompt asking whether the new mail should be private.
/// Verbatim text from `amiexpress/express.e:10861`'s `Private` prompt
/// modulo the colour escapes the legacy `yesNo` macro renders.
pub(crate) const POST_PRIVATE_PROMPT: &[u8] = b"Private (y/N)? ";

/// Instructions printed before the body input loop. Slice 42 uses a
/// minimal line-mode editor — a full editor (`/S` save, `/A` abort,
/// numbered line edits) arrives in Phase 8. Still used by the `R`
/// sub-prompt reply (B6); `E` / `C` use the ruler editor below.
pub(crate) const POST_BODY_PROMPT: &[u8] =
    b"Enter your message. End with a single '.' on a line by itself; '/A' aborts.\r\n";

/// The `E` / `C` ruler-editor intro: the "Enter your text" instruction
/// and the 75-column ruler (`amiexpress/express.e:10146-10152`, the
/// repeating `|-------` pattern truncated to `maxLineLen`=75). Input
/// ends on a blank line.
pub(crate) const EDITOR_INTRO: &[u8] =
    b"\r\n   Enter your text. (Enter) alone to end. (75 chars/line)\r\n   (|-------|-------|-------|-------|-------|-------|-------|-------|-------|--)\r\n";

/// The ruler editor's `Msg. Options:` save-menu prompt, shown after a
/// blank line ends input (`amiexpress/express.e:10375-10379`, rendered
/// for the no-file-attach case so `F`/`X` are absent). `S` saves, `A`
/// aborts (with confirm), `C` continues editing, `L` lists, `?` shows
/// the verb help. `D`/`E` (delete / edit lines) are advertised but
/// deferred.
pub(crate) const EDITOR_MSG_OPTIONS_PROMPT: &[u8] =
    b"\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m,\x1b[33mC\x1b[36m,\x1b[33mD\x1b[36m,\x1b[33mE\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mS\x1b[36m,\x1b[33m? \x1b[0m>:";

/// The expanded `Msg. Options:` help list shown after `?`
/// (`amiexpress/express.e:10381-10389`, no-file-attach case). It ends
/// in its own ` >: ` prompt and reads the next verb directly.
pub(crate) const EDITOR_MSG_OPTIONS_HELP: &[u8] = b"\r\n\x1b[33mA\x1b[32m>\x1b[36mbort\x1b[0m\r\n\x1b[33mC\x1b[32m>\x1b[36montinue\x1b[0m\r\n\x1b[33mD\x1b[32m>\x1b[36melete Lines\x1b[0m\r\n\x1b[33mE\x1b[32m>\x1b[36mdit\x1b[0m\r\n\x1b[33mL\x1b[32m>\x1b[36mist\x1b[0m\r\n\x1b[33mS\x1b[32m>\x1b[36mave\x1b[0m\r\n\x1b[0m >: ";

/// The `A`bort confirmation prompt from the save menu
/// (`amiexpress/express.e:10568`). A `y` answer abandons the message.
pub(crate) const EDITOR_ABORT_CONFIRM_PROMPT: &[u8] = b"\r\nAbort message entry (y/n)? ";

/// Renders one ruler-editor line prompt, `<n>> ` with the number
/// left-justified to a 2-character field for lines 1..=99 and a
/// 3-character field beyond (legacy `\d[2]> ` / `\d[3]> ` at
/// `amiexpress/express.e:10180-10184`). Line 1 renders `"1 > "`,
/// line 10 `"10> "`, line 100 `"100> "`.
#[must_use]
pub(crate) fn render_editor_line_prompt(line_number: usize) -> Vec<u8> {
    if line_number <= 99 {
        format!("{line_number:<2}> ").into_bytes()
    } else {
        format!("{line_number:<3}> ").into_bytes()
    }
}

/// Renders the ruler editor's `L`ist output: a leading CRLF, then each
/// stored line as `<n>> <text>` followed by CRLF
/// (`amiexpress/express.e:10496-10504`).
#[must_use]
pub(crate) fn render_editor_listing(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\r\n");
    for (index, line) in lines.iter().enumerate() {
        out.extend_from_slice(&render_editor_line_prompt(index + 1));
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Sent when the typed recipient can't be resolved against the user
/// repository. Mirrors the legacy `User does not exist!!` notice
/// (`amiexpress/express.e:10814`).
pub(crate) const POST_UNKNOWN_USER_LINE: &[u8] = b"\r\nUnknown user.\r\n";

/// Sent when the `C` (comment to sysop) command can't resolve a slot-1
/// sysop user (e.g. a fresh installation that never seeded one). The
/// legacy BBS always has a sysop on disk; this notice surfaces the
/// misconfiguration so the operator can run the seed.
pub(crate) const NO_SYSOP_LINE: &[u8] = b"\r\nNo sysop is configured on this BBS.\r\n";

/// Sent when a reply targets a soft-deleted source (Slice 49a). The
/// underlying domain check fires before any prompts; the rule
/// rejects with `ReplyToMailError::SourceDeleted`.
pub(crate) const SOURCE_DELETED_LINE: &[u8] =
    b"\r\nThat message has been deleted; cannot reply.\r\n";

/// Prompt for the `FW` command's new-addressee handle (Slice 49a).
pub(crate) const FORWARD_TO_PROMPT: &[u8] = b"\r\nForward to: ";

/// Prompt for the optional `--`-separated note the user may append
/// to a forwarded mail (Slice 49a). Empty input means "no note".
pub(crate) const FORWARD_NOTE_PROMPT: &[u8] =
    b"Optional note. End with a single '.' on a line by itself; blank line skips.\r\n";

/// Formats the mail-scan summary line (Slice 40 / 41). Mirrors the
/// legacy `searchNewMail` output's "New Mail" notice and the
/// "No New Mail" fallback at `amiexpress/express.e:26499`.
///
/// ```text
///   No new mail.                                  (unread_count == 0)
///   You have <N> new message(s). First: <num>.    (unread_count > 0)
/// ```
pub(crate) fn render_scan_summary(unread_count: u32, first_unread_number: Option<u32>) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    if unread_count == 0 {
        out.extend_from_slice(b"\r\nNo new mail.\r\n");
        return out;
    }
    out.extend_from_slice(b"\r\nYou have ");
    out.extend_from_slice(unread_count.to_string().as_bytes());
    out.extend_from_slice(if unread_count == 1 {
        b" new message"
    } else {
        b" new messages"
    });
    if let Some(first) = first_unread_number {
        out.extend_from_slice(b". First: ");
        out.extend_from_slice(first.to_string().as_bytes());
    }
    out.extend_from_slice(b".\r\n");
    out
}

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
pub(crate) fn render_menu_prompt(
    bbs_name: &str,
    conference: Option<(u32, &str)>,
    mins_left: u64,
) -> Vec<u8> {
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
pub(crate) fn auto_rejoin_line(
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
pub(crate) fn explicit_join_line(conference_name: &str, msgbase_name: Option<&str>) -> Vec<u8> {
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

    #[test]
    fn conference_number_prompt_matches_the_legacy_capture() {
        // Live capture (AmiExpress 5.6.0, two conferences):
        // `b'Conference Number (1-2): '` — trailing space, no CRLF on
        // either side (`amiexpress/express.e:25144`).
        assert_eq!(
            render_conference_number_prompt(2),
            b"Conference Number (1-2): "
        );
    }

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

    #[test]
    fn render_scan_summary_emits_no_new_mail_for_zero() {
        // Legacy `\tNo New Mail!\b\n` (`amiexpress/express.e:26499`).
        assert_eq!(render_scan_summary(0, None), b"\r\nNo new mail.\r\n");
        // first_unread_number is ignored when count is zero.
        assert_eq!(render_scan_summary(0, Some(5)), b"\r\nNo new mail.\r\n");
    }

    #[test]
    fn render_scan_summary_pluralises_message_for_more_than_one() {
        assert_eq!(
            render_scan_summary(3, Some(5)),
            b"\r\nYou have 3 new messages. First: 5.\r\n",
        );
    }

    #[test]
    fn render_scan_summary_uses_singular_for_one_message() {
        assert_eq!(
            render_scan_summary(1, Some(7)),
            b"\r\nYou have 1 new message. First: 7.\r\n",
        );
    }

    #[test]
    fn render_scan_summary_handles_missing_first_unread_number() {
        // Defensive: a non-zero count without a number would be a
        // bug, but the renderer must not panic on it.
        assert_eq!(
            render_scan_summary(2, None),
            b"\r\nYou have 2 new messages.\r\n",
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

    #[test]
    fn editor_line_prompt_left_justifies_the_number() {
        // Legacy `\d[2]> ` (`amiexpress/express.e:10180`): the number is
        // left-justified to a 2-char field, so line 1 reads `"1 > "`.
        assert_eq!(render_editor_line_prompt(1), b"1 > ");
        assert_eq!(render_editor_line_prompt(9), b"9 > ");
        // Two digits fill the field exactly.
        assert_eq!(render_editor_line_prompt(10), b"10> ");
        assert_eq!(render_editor_line_prompt(99), b"99> ");
        // Beyond 99 the legacy widens to `\d[3]` (`:10182`).
        assert_eq!(render_editor_line_prompt(100), b"100> ");
    }

    #[test]
    fn editor_listing_numbers_each_line() {
        // Legacy `L` (`amiexpress/express.e:10496-10504`): leading CRLF,
        // then `<n>> <text>` + CRLF per line.
        let lines = vec!["first".to_string(), "second".to_string()];
        assert_eq!(
            render_editor_listing(&lines),
            b"\r\n1 > first\r\n2 > second\r\n"
        );
    }

    #[test]
    fn editor_listing_with_no_lines_is_just_a_crlf() {
        assert_eq!(render_editor_listing(&[]), b"\r\n");
    }
}
