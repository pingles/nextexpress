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

/// Sent when the `MV` sub-command's target cannot be parsed as a
/// conference number.
pub(crate) const INVALID_CONFERENCE_NUMBER_LINE: &[u8] = b"\r\nInvalid conference number.\r\n";

/// Sent when `R <something>` cannot be parsed as a message number.
pub(crate) const INVALID_MESSAGE_NUMBER_LINE: &[u8] = b"\r\nInvalid message number.\r\n";

/// Sent when the requested message number is unknown in this
/// message base. Mirrors the legacy `Msg #X not found.` notice
/// (`amiexpress/express.e:25460`).
pub(crate) const MESSAGE_NOT_FOUND_LINE: &[u8] = b"\r\nMessage not found.\r\n";

/// Sent when the current conference has no mail store configured.
/// In a correctly-configured BBS every conference's `MsgBase/`
/// directory backs a store; this notice surfaces a sysop
/// misconfiguration.
pub(crate) const NO_MAIL_BASE_LINE: &[u8] = b"\r\nNo message base for this conference.\r\n";

/// Sent when the user tries to read a soft-deleted message. Mirrors
/// the legacy `That message has been deleted.` line
/// (`amiexpress/express.e:8890`).
pub(crate) const DELETED_MESSAGE_LINE: &[u8] = b"\r\nThat message has been deleted.\r\n\r\n";

/// Sent when the user has no membership grant for the current
/// conference, or the message's visibility blocks them from reading
/// it.
pub(crate) const READ_DENIED_LINE: &[u8] = b"\r\nYou are not permitted to read this message.\r\n";

/// Sent when the underlying mail store rejects the request (I/O
/// failure, corrupted payload, etc.). The detailed error is logged
/// to stderr; the wire surface is intentionally generic so a bad
/// disk doesn't leak file paths to the user.
pub(crate) const MAIL_STORE_ERROR_LINE: &[u8] = b"\r\nMessage base error. Notify the sysop.\r\n";

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

/// Sent when the user aborts message composition (empty subject,
/// `/A` body command, or `~` shortcut). The session returns to the
/// menu prompt.
pub(crate) const POST_ABORTED_LINE: &[u8] = b"\r\nMessage aborted.\r\n";

/// Sent when the typed recipient can't be resolved against the user
/// repository. Mirrors the legacy `User does not exist!!` notice
/// (`amiexpress/express.e:10814`).
pub(crate) const POST_UNKNOWN_USER_LINE: &[u8] = b"\r\nUnknown user.\r\n";

/// Sent when the resolved recipient has no granted membership for
/// the current conference. Mirrors `amiexpress/express.e:10838`.
pub(crate) const POST_RECIPIENT_NO_ACCESS_LINE: &[u8] =
    b"\r\nUser does not have access to this conference.\r\n";

/// Sent when the user lacks `has_access(EnterMessage)`. The
/// pending-validation tier denies this right (Slice 21), so this
/// notice fires for not-yet-validated accounts.
pub(crate) const POST_ACCESS_DENIED_LINE: &[u8] =
    b"\r\nYou do not have permission to post messages.\r\n";

/// Sent when the user addresses ALL or EALL but the current message
/// base's [`AllowedAddressing`] policy refuses that broadcast kind
/// (Slice 43).
///
/// Mirrors the spirit of the legacy `enterMSG` "Echo To All" gate at
/// `amiexpress/express.e:10802` which checks the tooltype-driven
/// `ALLOW_ALL` flag.
pub(crate) const POST_ADDRESSING_NOT_ALLOWED_LINE: &[u8] =
    b"\r\nThis message base does not accept that addressee.\r\n";

/// Sent when the `C` (comment to sysop) command can't resolve a slot-1
/// sysop user (e.g. a fresh installation that never seeded one). The
/// legacy BBS always has a sysop on disk; this notice surfaces the
/// misconfiguration so the operator can run the seed.
pub(crate) const NO_SYSOP_LINE: &[u8] = b"\r\nNo sysop is configured on this BBS.\r\n";

/// Sent when a reply / forward / kill / move / edit-header command
/// references a message number that does not exist in the current
/// message base (Slice 49a / 49b).
pub(crate) const SOURCE_NOT_FOUND_LINE: &[u8] = b"\r\nNo such message in this base.\r\n";

/// Sent when a reply targets a soft-deleted source (Slice 49a). The
/// underlying domain check fires before any prompts; the rule
/// rejects with `ReplyToMailError::SourceDeleted`.
pub(crate) const SOURCE_DELETED_LINE: &[u8] =
    b"\r\nThat message has been deleted; cannot reply.\r\n";

/// Sent when a `FW` command's typed addressee cannot be resolved
/// (Slice 49a). Mirrors the `E` command's unknown-user surface so
/// the user can re-type without further explanation.
pub(crate) const FORWARD_UNKNOWN_USER_LINE: &[u8] = b"\r\nUnknown forward recipient.\r\n";

/// Prompt asking the user to confirm a destructive operation
/// (Slice 49b's `K` delete). Defaults to `N` so an idle CR is safe.
#[allow(dead_code, reason = "Wired up in Slice 49b")]
pub(crate) const CONFIRM_DELETE_PROMPT: &[u8] = b"Delete message (y/N)? ";

/// Prompt for the `FW` command's new-addressee handle (Slice 49a).
pub(crate) const FORWARD_TO_PROMPT: &[u8] = b"\r\nForward to: ";

/// Prompt for the optional `--`-separated note the user may append
/// to a forwarded mail (Slice 49a). Empty input means "no note".
pub(crate) const FORWARD_NOTE_PROMPT: &[u8] =
    b"Optional note. End with a single '.' on a line by itself; blank line skips.\r\n";

/// Sent when a sysop-only command (Slice 49b) is invoked by a user
/// without the required access level / right.
pub(crate) const SYSOP_ONLY_LINE: &[u8] =
    b"\r\nYou do not have permission to perform that operation.\r\n";

/// Prompt for the target conference number of an `MV <num>` move
/// (Slice 49b).
pub(crate) const MOVE_TARGET_CONFERENCE_PROMPT: &[u8] = b"\r\nTarget conference number: ";

/// Prompt for the target msgbase number of an `MV <num>` move
/// (Slice 49b).
pub(crate) const MOVE_TARGET_MSGBASE_PROMPT: &[u8] = b"Target msgbase number: ";

/// Sent when an `MV <num>` references a target msgbase that's not
/// registered with the running BBS.
pub(crate) const MOVE_UNKNOWN_TARGET_LINE: &[u8] = b"\r\nNo such target message base.\r\n";

/// Confirmation line printed after a successful `K <num>` delete.
pub(crate) const DELETE_DONE_LINE: &[u8] = b"\r\nMessage deleted.\r\n";

/// Confirmation line printed after a successful `MV <num>` move.
/// Includes the new number so the user can navigate to it.
pub(crate) const MOVE_DONE_PREFIX: &[u8] = b"\r\nMessage moved. New number ";

/// Confirmation line printed after a successful `EH <num>` edit.
pub(crate) const EDIT_HEADER_DONE_LINE: &[u8] = b"\r\nHeader updated.\r\n";

/// Prompt for the new subject during an `EH <num>` header edit.
/// Empty input keeps the current subject.
pub(crate) const EDIT_HEADER_SUBJECT_PROMPT: &[u8] = b"New subject (blank = unchanged): ";

/// Prompt for the new addressee during an `EH <num>` header edit.
/// Empty input keeps the current addressee.
pub(crate) const EDIT_HEADER_TO_PROMPT: &[u8] = b"New To (blank = unchanged): ";

/// Renders a [`Mail`]'s header block for the menu's `R` command.
///
/// Mirrors the legacy `displayMessage` output at
/// `amiexpress/express.e:8900-8938`:
///
/// ```text
///   Date   : <date>                Number: <n>
///   To     : <to>                  Recv'd: <date | No | N/A>
///   From   : <from>                Status: Public Message | Private Message
///   Subject: <subject>
/// ```
///
/// ANSI colour escapes match the legacy output: `[32m` green for
/// the labels' left half, `[33m` yellow for the separating colon,
/// `[0m` reset for the value, all on a single colour budget that
/// the existing telnet adapter passes through verbatim.
///
/// Timestamps are rendered as RFC 3339 UTC for now; the legacy's
/// human-friendly `formatLongDateTime` format lands with the
/// locale-aware formatter slice in Phase 13.
///
/// [`Mail`]: crate::domain::messaging::mail::Mail
pub(crate) fn render_mail_header(
    mail: &crate::domain::messaging::mail::Mail,
    conference_name: &str,
) -> Vec<u8> {
    use crate::domain::messaging::mail::{BroadcastTo, MailVisibility};
    use time::OffsetDateTime;
    let mut out = Vec::with_capacity(256);
    let posted = OffsetDateTime::from(mail.posted_at());
    let posted_str = posted
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    out.extend_from_slice(b"\r\n\x1b[32mDate   \x1b[33m:\x1b[0m ");
    out.extend_from_slice(posted_str.as_bytes());
    out.extend_from_slice(b"  \x1b[32mNumber\x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.number().to_string().as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mTo     \x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.to_name().as_bytes());
    out.extend_from_slice(b"  \x1b[32mRecv'd\x1b[33m:\x1b[0m ");
    match (mail.received_at(), mail.broadcast_to()) {
        (Some(t), _) => {
            let recvd = OffsetDateTime::from(t)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "unknown".to_string());
            out.extend_from_slice(recvd.as_bytes());
        }
        (None, BroadcastTo::All | BroadcastTo::Eall) => out.extend_from_slice(b"N/A"),
        (None, BroadcastTo::None) => out.extend_from_slice(b"No"),
    }
    out.extend_from_slice(b"\r\n\x1b[32mFrom   \x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.from_name().as_bytes());
    out.extend_from_slice(b"  \x1b[32mStatus\x1b[33m:\x1b[0m ");
    let status = match mail.visibility() {
        MailVisibility::Public => "Public Message",
        MailVisibility::Private => "Private Message",
        // PrivateToSysop and Deleted are filtered upstream — the `R`
        // dispatch rejects deleted mail and the read-permission gate
        // blocks PrivateToSysop for non-author/non-sysop readers. We
        // render them defensively for the sysop-reads-anything case
        // (PrivateToSysop) and the never-reachable Deleted branch.
        MailVisibility::PrivateToSysop => "Private to Sysop",
        MailVisibility::Deleted => "Deleted",
    };
    out.extend_from_slice(status.as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mSubject\x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.subject().as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mConf   \x1b[33m:\x1b[0m [");
    out.extend_from_slice(mail.msgbase().conference_number().to_string().as_bytes());
    out.extend_from_slice(b"] ");
    out.extend_from_slice(conference_name.as_bytes());
    out.extend_from_slice(b"\r\n\r\n");
    out
}

/// Renders the legacy `readMSG` read sub-prompt, assembled piecewise at
/// `amiexpress/express.e:12016-12021`. `show_delete` inserts the `D`
/// (delete) option after `A` (legacy `ACS_DELETE_MESSAGE`, `:12017`) and
/// `show_move` inserts `M` (move) after it (legacy `ACS_SYSOP_READ`,
/// `:12018`); the caller passes the gate results for the current message
/// and user.
///
/// The `( <range> )` slot carries the precomputed runtime range string,
/// either `<next>+<highest>` (`:12010`, where `next` is the next message
/// to read and `highest` the highest existing number,
/// `mailStat.highMsgNum - 1`) or the literal `QUIT` when `next` is out of
/// range (`:12012`). The caller computes the collapse. When neither `D`
/// nor `M` is shown the two `\x1b[36m` colour codes around the skipped
/// slot collapse into the doubled-`\x1b[36m` seam the legacy leaves in
/// that case.
pub(crate) fn render_read_subprompt(range: &[u8], show_delete: bool, show_move: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(112);
    out.extend_from_slice(b"\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m");
    if show_delete {
        out.extend_from_slice(b",\x1b[33mD");
    }
    if show_move {
        out.extend_from_slice(b",\x1b[33mM");
    }
    out.extend_from_slice(
        b"\x1b[36m,\x1b[33mF\x1b[36m,\x1b[33mR\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mQ\x1b[36m,\x1b[33m?\x1b[36m,\x1b[33m??\x1b[36m,\x1b[32m<\x1b[33mCR\x1b[32m> \x1b[32m(\x1b[0m ",
    );
    out.extend_from_slice(range);
    out.extend_from_slice(b" \x1b[32m )\x1b[0m>: ");
    out
}

/// Renders the `readMSG` sub-prompt help list shown when the caller
/// types `?` (short, `long = false`, `express.e:12023-12032`) or `??`
/// (long, `long = true`, `:12034-12060`). The list is gated like the
/// skeleton: `D`elete / `M`ove appear per `show_delete` / `show_move`,
/// and the long list adds `EH` (edit header) per `show_edit`. The long
/// list spells `Move`/`List` out more fully (`Move Message` /
/// `List all messages`). It ends with the legacy
/// `<CR>=Next ( <range> )? ` tail (`:12031` / `:12058`).
///
/// `NextExpress` deliberately omits the legacy `NS` / translate / `Keep`
/// / `E` / `EM` / account-edit entries (see slice B5 Out of Scope), so
/// the long list is a faithful subset rather than the full legacy menu.
// The four flags map one-to-one to the legacy `helplist` / `checkSecurity`
// switches; grouping them into a struct would obscure that correspondence
// for no real call-site benefit (there is a single caller).
#[allow(clippy::fn_params_excessive_bools)]
pub(crate) fn render_read_subprompt_help(
    long: bool,
    range: &[u8],
    show_delete: bool,
    show_move: bool,
    show_edit: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    // `A`gain has no leading newline — the caller's echoed `?` / `??`
    // CRLF already put the cursor on a fresh line (legacy `:12024`).
    out.extend_from_slice(b"\x1b[33mA\x1b[32m>\x1b[36mgain\x1b[0m");
    if show_delete {
        out.extend_from_slice(b"\r\n\x1b[33mD\x1b[32m>\x1b[36melete Message\x1b[0m");
    }
    if show_move {
        out.extend_from_slice(if long {
            b"\r\n\x1b[33mM\x1b[32m>\x1b[36move Message\x1b[0m".as_slice()
        } else {
            b"\r\n\x1b[33mM\x1b[32m>\x1b[36move\x1b[0m".as_slice()
        });
    }
    out.extend_from_slice(b"\r\n\x1b[33mF\x1b[32m>\x1b[36morward\x1b[0m");
    out.extend_from_slice(b"\r\n\x1b[33mR\x1b[32m>\x1b[36meply\x1b[0m");
    if long {
        out.extend_from_slice(b"\r\n\x1b[33mL\x1b[32m>\x1b[36mist all messages\x1b[0m");
        if show_edit {
            out.extend_from_slice(b"\r\n\x1b[33mEH\x1b[32m>\x1b[36m Edit Message Header\x1b[0m");
        }
    } else {
        out.extend_from_slice(b"\r\n\x1b[33mL\x1b[32m>\x1b[36mist\x1b[0m");
    }
    out.extend_from_slice(b"\r\n\x1b[33mQ\x1b[32m>\x1b[36muit\x1b[0m");
    out.extend_from_slice(
        b"\r\n\x1b[32m<\x1b[33mCR\x1b[32m>\x1b[0m=\x1b[33mNext \x1b[32m(\x1b[0m ",
    );
    out.extend_from_slice(range);
    out.extend_from_slice(b" \x1b[32m )\x1b[0m? ");
    out
}

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

/// The `MS` command's opening banner (`amiexpress/express.e:25258`,
/// `'\b\nScanning conferences for mail...\b\n\b\n'`, Amiga `\b\n`
/// translated to telnet `\r\n`).
pub(crate) const MAIL_SCAN_ALL_HEADER: &[u8] = b"\r\nScanning conferences for mail...\r\n\r\n";

/// Printed in place of a listing when a conference has no new mail
/// since the user's last scan (`amiexpress/express.e:11689`, the
/// `currentConf=0` branch).
pub(crate) const MAIL_SCAN_NO_MAIL_TODAY: &[u8] = b"No mail today!\r\n";

/// The read-it-now prompt the multi-conference scan shows after a base
/// whose listing matched unread mail (`amiexpress/express.e:11739`'s
/// `'\b\nWould you like to read it now '` followed by `yesNo(1)`'s
/// default-Yes `(Y/n)?` render at `:2136`). On a `y`/CR the caller is
/// dropped into the read/reply sub-prompt for the found message.
pub(crate) const MAIL_SCAN_READ_IT_NOW_PROMPT: &[u8] =
    b"\r\nWould you like to read it now \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[32m?\x1b[0m ";

/// Renders the per-conference banner the multi-conference scan prints
/// before each conference's listing (`amiexpress/express.e:11670`).
/// No trailing newline — the message-base sub-line or the listing
/// table that follows supplies the break.
pub(crate) fn render_scan_conference_banner(conference_name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(48 + conference_name.len());
    out.extend_from_slice(b"\x1b[32mScanning Conference\x1b[33m: \x1b[0m");
    out.extend_from_slice(conference_name.as_bytes());
    out.extend_from_slice(b" - ");
    out
}

/// Renders the message-base sub-line of the scan banner
/// (`amiexpress/express.e:11676-11677`). Empty when the base is
/// unnamed (mirrors `IF StrLen(msgBaseName)>0`). A leading CRLF is
/// emitted only for a conference's first base (`IF msgBaseNum=1`).
pub(crate) fn render_scan_msgbase_banner(msgbase_name: &str, first_base: bool) -> Vec<u8> {
    if msgbase_name.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(48 + msgbase_name.len());
    if first_base {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b" \x1b[32mMessage Base\x1b[33m: \x1b[0m");
    out.extend_from_slice(msgbase_name.as_bytes());
    out.extend_from_slice(b" - ");
    out
}

/// Renders the legacy mail-scan listing table for one base
/// (`amiexpress/express.e:11712-11721`): two leading CRLFs, three
/// header writes (green column titles, yellow dash separator, a
/// standalone colour reset), then one row per unread message. Returns
/// an empty buffer for an empty `rows` slice — the legacy only paints
/// the header once it has matched a first message (the `mailFlag`
/// gate at `:11710`).
///
/// Each row is `status(7)  from(29)  subject(21)  <reset>msg(6)` with
/// the over-wide `from` / `subject` fields truncated to their columns,
/// matching `AmigaE` `StringF`'s `\s[n]` behaviour (`:11720`).
pub(crate) fn render_scan_listing_table(
    rows: &[crate::domain::messaging::scan_mail::MailScanRow],
) -> Vec<u8> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(160 + rows.len() * 72);
    out.extend_from_slice(b"\r\n\r\n");
    out.extend_from_slice(
        b"\x1b[32mType     From                           Subject                Msg    \r\n",
    );
    out.extend_from_slice(
        b"\x1b[33m-------  -----------------------------  ---------------------  -------\r\n",
    );
    out.extend_from_slice(b"\x1b[0m");
    for entry in rows {
        out.extend_from_slice(scan_row_status(entry.visibility).as_bytes());
        out.extend_from_slice(b"  ");
        out.extend_from_slice(left_field(&entry.from_name, 29).as_bytes());
        out.extend_from_slice(b"  ");
        out.extend_from_slice(left_field(&entry.subject, 21).as_bytes());
        out.extend_from_slice(b"  \x1b[0m");
        out.extend_from_slice(format!("{:06}", entry.number).as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Renders the `L`ist (`listMSGs`) table header
/// (`amiexpress/express.e:8856-8859`): two leading CRLFs, a green
/// `Msg / Type / From / Subject` column line (message number **first**,
/// unlike the scan table), a yellow dashes row, then a colour reset.
/// The legacy counts this block as four lines towards the pager.
pub(crate) fn render_list_header() -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    out.extend_from_slice(b"\r\n\r\n\x1b[32m");
    out.extend_from_slice(left_field("Msg", 6).as_bytes());
    out.push(b' ');
    out.extend_from_slice(left_field("Type", 7).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice(left_field("From", 29).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice(left_field("Subject", 21).as_bytes());
    out.extend_from_slice(b"\r\n\x1b[33m");
    out.extend_from_slice("-".repeat(6).as_bytes());
    out.push(b' ');
    out.extend_from_slice("-".repeat(7).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice("-".repeat(29).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice("-".repeat(21).as_bytes());
    out.extend_from_slice(b"\r\n\x1b[0m");
    out
}

/// Renders one `L`ist row (`amiexpress/express.e:8864`):
/// `\z\r\d[6] \s  \l\s[29]  \l\s[21]  [0m\b\n` — the zero-padded message
/// number first, then the 7-char status, the 29-wide From and 21-wide
/// Subject columns, and a colour reset.
pub(crate) fn render_list_row(row: &crate::domain::messaging::scan_mail::MailScanRow) -> Vec<u8> {
    let mut out = Vec::with_capacity(80);
    out.extend_from_slice(format!("{:06}", row.number).as_bytes());
    out.push(b' ');
    out.extend_from_slice(scan_row_status(row.visibility).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice(left_field(&row.from_name, 29).as_bytes());
    out.extend_from_slice(b"  ");
    out.extend_from_slice(left_field(&row.subject, 21).as_bytes());
    out.extend_from_slice(b"  \x1b[0m\r\n");
    out
}

/// The 7-character status column for a listing row
/// (`amiexpress/express.e:11719`): only `Public` mail renders as
/// `"Public "`; every other (non-deleted) visibility is `"Private"`.
fn scan_row_status(visibility: crate::domain::messaging::mail::MailVisibility) -> &'static str {
    use crate::domain::messaging::mail::MailVisibility;
    match visibility {
        MailVisibility::Public => "Public ",
        _ => "Private",
    }
}

/// Left-justifies `value` within `width` columns, truncating it to
/// `width` characters first so the listing columns stay aligned even
/// for over-long handles or subjects (`AmigaE` `StringF` `\l\s[n]`).
fn left_field(value: &str, width: usize) -> String {
    let truncated: String = value.chars().take(width).collect();
    format!("{truncated:<width$}")
}

/// Translates a mail body's LF line endings to telnet CRLF, ensuring
/// the trailing line ends with `\r\n`. The on-disk body uses Unix
/// line endings; rendering for telnet has to normalise them or the
/// receiving terminal stair-steps. Mirrors the legacy `displayFile`'s
/// per-line `aePuts` behaviour.
pub(crate) fn render_mail_body(body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 16);
    let mut last_was_cr = false;
    for ch in body.chars() {
        if ch == '\n' && !last_was_cr {
            out.extend_from_slice(b"\r\n");
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
        last_was_cr = ch == '\r';
    }
    if !body.ends_with('\n') {
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Formats the post-success line shown after `messaging.allium:PostMail`
/// (Slice 42). Mirrors the legacy `enterMSG` "Saving..." sequence at
/// `amiexpress/express.e:10972-10976`, simplified to a single line so
/// the menu loop can resume cleanly.
///
/// ```text
///   Message #<n> saved.
/// ```
pub(crate) fn render_post_success(number: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(b"\r\nMessage #");
    out.extend_from_slice(number.to_string().as_bytes());
    out.extend_from_slice(b" saved.\r\n");
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

/// Clears the screen for the `CF` listing: the legacy `sendCLS()` (a
/// single form-feed byte, `amiexpress/express.e:5224`) then the leading
/// CRLF at `:24690`.
const CONF_FLAGS_CLEAR: &[u8] = b"\x0c\r\n";

/// The `CF` listing column header (`amiexpress/express.e:24691`).
const CONF_FLAGS_HEADER: &[u8] =
    b"\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n";

/// The `CF` listing ruler row, with its trailing blank line
/// (`amiexpress/express.e:24692`, whose literal ends `[0m\b\n\b\n`).
const CONF_FLAGS_RULER: &[u8] =
    b"\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n";

/// The `CF` mask-selection prompt, with its two leading blank lines
/// (`amiexpress/express.e:24749`).
pub(crate) const CONF_FLAGS_MASK_PROMPT: &[u8] =
    b"\r\n\r\nEdit which flags [M]ailScan, [A]ll Messages, [F]ileScan, [Z]oom >: ";

/// The `CF` conference-selection prompt (`amiexpress/express.e:24769`).
pub(crate) const CONF_FLAGS_EXPR_PROMPT: &[u8] =
    b"Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on >: ";

/// Renders the full `CF` (conference flags) listing screen — the legacy
/// `internalCommandCF` redraw (`amiexpress/express.e:24689-24747`): a
/// clear-screen, the M/A/F/Z column header and ruler, then one
/// `[ n] M A F Z <name>` cell per accessible conference, two per line.
///
/// The caller emits [`CONF_FLAGS_MASK_PROMPT`] after this. A set flag
/// renders as `*`, a clear flag as a space; the cell order is
/// `M A F Z` = mail / all-messages / file / zoom (`:24732`/`:24735`).
pub(crate) fn render_conf_flags_listing(
    rows: &[crate::domain::conference_flags::ConfFlagRow],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        CONF_FLAGS_CLEAR.len() + CONF_FLAGS_HEADER.len() + CONF_FLAGS_RULER.len() + rows.len() * 80,
    );
    out.extend_from_slice(CONF_FLAGS_CLEAR);
    out.extend_from_slice(CONF_FLAGS_HEADER);
    out.extend_from_slice(CONF_FLAGS_RULER);
    for (index, row) in rows.iter().enumerate() {
        out.extend_from_slice(b"\x1b[34m[\x1b[0m");
        out.extend_from_slice(format!("{:5}", row.conference_number).as_bytes());
        out.extend_from_slice(b"\x1b[34m] \x1b[36m");
        out.push(conf_flag_glyph(row.mail_scan));
        out.push(b' ');
        out.push(conf_flag_glyph(row.mailscan_all));
        out.push(b' ');
        out.push(conf_flag_glyph(row.file_scan));
        out.push(b' ');
        out.push(conf_flag_glyph(row.zoom_scan));
        out.push(b' ');
        out.extend_from_slice(b"\x1b[0m");
        out.extend_from_slice(left_field(&row.conference_name, 23).as_bytes());
        // Two conferences per line: even cells get a separating space,
        // odd cells close the line (legacy `IF n AND 1`, `:24739`).
        if index % 2 == 1 {
            out.extend_from_slice(b"\r\n");
        } else {
            out.push(b' ');
        }
    }
    out
}

/// `*` for a set scan flag, a space for a clear one (the legacy `c1..c4`
/// glyphs at `amiexpress/express.e:24704-24726`, restricted to the
/// `*`/space cases — the `F`/`D` tooltype overrides are out of scope).
fn conf_flag_glyph(on: bool) -> u8 {
    if on {
        b'*'
    } else {
        b' '
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::fn_params_excessive_bools)] // one bool per M/A/F/Z cell
    fn cf_row(
        number: u32,
        name: &str,
        mail_scan: bool,
        mailscan_all: bool,
        file_scan: bool,
        zoom_scan: bool,
    ) -> crate::domain::conference_flags::ConfFlagRow {
        crate::domain::conference_flags::ConfFlagRow {
            conference_number: number,
            conference_name: name.to_string(),
            mail_scan,
            mailscan_all,
            file_scan,
            zoom_scan,
        }
    }

    #[test]
    fn render_conf_flags_listing_matches_the_legacy_all_off_capture() {
        // Byte-for-byte against the FS-UAE reference capture (two
        // single-base conferences, every flag clear).
        let rows = vec![
            cf_row(1, "New Users", false, false, false, false),
            cf_row(2, "Amiga", false, false, false, false),
        ];
        let expected: &[u8] = b"\x0c\r\n\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n\x1b[34m[\x1b[0m    1\x1b[34m] \x1b[36m        \x1b[0mNew Users               \x1b[34m[\x1b[0m    2\x1b[34m] \x1b[36m        \x1b[0mAmiga                  \r\n";
        assert_eq!(render_conf_flags_listing(&rows), expected);
    }

    #[test]
    fn render_conf_flags_listing_marks_a_set_flag_with_a_star() {
        // Conference 2's mail-scan set: the M cell becomes `*` (the
        // legacy post-toggle capture). The two-per-line layout also
        // pins the even-cell separating space and the odd-cell CRLF.
        let rows = vec![
            cf_row(1, "New Users", false, false, false, false),
            cf_row(2, "Amiga", true, false, false, false),
        ];
        let expected: &[u8] = b"\x0c\r\n\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n\x1b[34m[\x1b[0m    1\x1b[34m] \x1b[36m        \x1b[0mNew Users               \x1b[34m[\x1b[0m    2\x1b[34m] \x1b[36m*       \x1b[0mAmiga                  \r\n";
        assert_eq!(render_conf_flags_listing(&rows), expected);
    }

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
    fn cf_prompts_match_the_legacy_captures() {
        assert_eq!(
            CONF_FLAGS_MASK_PROMPT,
            b"\r\n\r\nEdit which flags [M]ailScan, [A]ll Messages, [F]ileScan, [Z]oom >: "
        );
        assert_eq!(
            CONF_FLAGS_EXPR_PROMPT,
            b"Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on >: "
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
    fn render_mail_body_translates_lone_lf_into_crlf() {
        // Bodies arrive from disk with Unix `\n` line endings;
        // emitting them as-is would stair-step the receiving
        // terminal. Telnet requires `\r\n`.
        let body = "First line.\nSecond line.\n";
        assert_eq!(render_mail_body(body), b"First line.\r\nSecond line.\r\n");
    }

    #[test]
    fn render_mail_body_preserves_existing_crlf_pairs() {
        // A body that already carries `\r\n` (e.g. authored on
        // Windows or by a migration tool) must not turn each pair
        // into `\r\r\n`.
        let body = "Line 1.\r\nLine 2.\r\n";
        assert_eq!(render_mail_body(body), b"Line 1.\r\nLine 2.\r\n");
    }

    #[test]
    fn render_mail_body_appends_terminator_when_body_has_no_trailing_newline() {
        // A body without a trailing LF must still end with `\r\n`
        // so the menu prompt that follows starts on a fresh line.
        let body = "No trailing newline";
        assert_eq!(render_mail_body(body), b"No trailing newline\r\n");
    }

    #[test]
    fn render_mail_body_handles_empty_input() {
        let body = "";
        // Empty body still emits the terminator so the menu prompt
        // is not jammed against the header block.
        assert_eq!(render_mail_body(body), b"\r\n");
    }

    #[test]
    fn render_mail_header_emits_legacy_label_block() {
        // Pin the legacy label block. The escape sequences and the
        // ordering of labels must match `amiexpress/express.e:8900-8938`.
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        let mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 7,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Welcome".to_string(),
            posted_at: std::time::SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: Some(2),
            body: "Hello".to_string(),
        });
        let rendered = render_mail_header(&mail, "Programming");
        let text = std::str::from_utf8(&rendered).expect("utf8");
        // Each label and its value appears once.
        assert!(text.contains("Date   "), "missing Date label: {text:?}");
        assert!(text.contains("Number"), "missing Number label: {text:?}");
        assert!(text.contains("Number\x1b[33m:\x1b[0m 7"), "wrong number");
        assert!(
            text.contains("To     \x1b[33m:\x1b[0m alice"),
            "wrong To: {text:?}",
        );
        assert!(
            text.contains("From   \x1b[33m:\x1b[0m Sysop"),
            "wrong From: {text:?}",
        );
        assert!(
            text.contains("Status\x1b[33m:\x1b[0m Public Message"),
            "wrong Status: {text:?}",
        );
        assert!(
            text.contains("Subject\x1b[33m:\x1b[0m Welcome"),
            "wrong Subject: {text:?}",
        );
        assert!(
            text.contains("Conf   \x1b[33m:\x1b[0m [2] Programming"),
            "wrong Conf: {text:?}",
        );
        // An unread mail addressed to a named user renders Recv'd: No.
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m No"),
            "expected Recv'd: No for unread, got: {text:?}",
        );
    }

    #[test]
    fn render_mail_header_marks_broadcast_recipients_as_not_applicable() {
        // `amiexpress/express.e:8923` — broadcast mail has
        // `Recv'd: N/A` because no single addressee owns it.
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        let mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            subject: "Notice".to_string(),
            posted_at: std::time::SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: None,
            body: "Notice body".to_string(),
        });
        let rendered = render_mail_header(&mail, "Conf");
        let text = std::str::from_utf8(&rendered).expect("utf8");
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m N/A"),
            "broadcast mail must render N/A, got: {text:?}",
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
    fn render_post_success_emits_message_number_and_terminator() {
        // Pin the legacy-aligned save confirmation.
        assert_eq!(render_post_success(7), b"\r\nMessage #7 saved.\r\n");
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
    fn render_mail_header_renders_received_at_when_set() {
        // A read mail (with `received_at = Some`) shows the timestamp
        // rather than the literal "No".
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        use std::time::{Duration, SystemTime};
        let mut mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Hi".to_string(),
            posted_at: SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: Some(2),
            body: String::new(),
        });
        mail.mark_received(SystemTime::UNIX_EPOCH + Duration::from_secs(100))
            .unwrap();
        let text = String::from_utf8(render_mail_header(&mail, "Conf")).unwrap();
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m 1970-01-01T00:01:40Z"),
            "expected RFC 3339 received_at, got: {text:?}",
        );
    }

    // ---- Tier B (MS multi-conference mail scan) wire rendering ----

    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::mail::{BroadcastTo, MailVisibility};
    use crate::domain::messaging::scan_mail::MailScanRow;

    fn row(number: u32, visibility: MailVisibility, from: &str, subject: &str) -> MailScanRow {
        MailScanRow {
            msgbase: MessageBaseRef::new(2, 1),
            number,
            visibility,
            from_name: from.to_string(),
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: subject.to_string(),
        }
    }

    #[test]
    fn scan_all_header_is_the_legacy_scanning_conferences_banner() {
        // amiexpress/express.e:25258 — '\b\nScanning conferences for mail...\b\n\b\n'.
        assert_eq!(
            MAIL_SCAN_ALL_HEADER,
            b"\r\nScanning conferences for mail...\r\n\r\n"
        );
    }

    #[test]
    fn renders_conference_banner_with_legacy_ansi() {
        // amiexpress/express.e:11670.
        assert_eq!(
            render_scan_conference_banner("General"),
            b"\x1b[32mScanning Conference\x1b[33m: \x1b[0mGeneral - ".to_vec()
        );
    }

    #[test]
    fn renders_msgbase_banner_for_named_first_base() {
        // amiexpress/express.e:11676-11677 — leading CRLF only on the
        // conference's first base.
        assert_eq!(
            render_scan_msgbase_banner("main", true),
            b"\r\n \x1b[32mMessage Base\x1b[33m: \x1b[0mmain - ".to_vec()
        );
    }

    #[test]
    fn renders_msgbase_banner_without_leading_newline_for_later_bases() {
        assert_eq!(
            render_scan_msgbase_banner("tech", false),
            b" \x1b[32mMessage Base\x1b[33m: \x1b[0mtech - ".to_vec()
        );
    }

    #[test]
    fn msgbase_banner_is_empty_for_unnamed_base() {
        // Mirrors `IF StrLen(msgBaseName)>0` — no sub-line for an unnamed base.
        assert!(render_scan_msgbase_banner("", true).is_empty());
        assert!(render_scan_msgbase_banner("", false).is_empty());
    }

    #[test]
    fn no_mail_today_line_is_verbatim() {
        // amiexpress/express.e:11689 (currentConf=0 branch).
        assert_eq!(MAIL_SCAN_NO_MAIL_TODAY, b"No mail today!\r\n");
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

    #[test]
    fn renders_listing_table_header_and_rows() {
        // amiexpress/express.e:11712-11721: leading CRLFx2, three header
        // writes (green columns / yellow dashes / standalone reset),
        // then one row per unread mail.
        let rows = vec![
            row(1, MailVisibility::Public, "alice", "hello"),
            row(42, MailVisibility::Private, "carol", "re: hello"),
        ];
        let expected = b"\r\n\r\n\x1b[32mType     From                           Subject                Msg    \r\n\x1b[33m-------  -----------------------------  ---------------------  -------\r\n\x1b[0mPublic   alice                          hello                  \x1b[0m000001\r\nPrivate  carol                          re: hello              \x1b[0m000042\r\n".to_vec();
        assert_eq!(render_scan_listing_table(&rows), expected);
    }

    #[test]
    fn listing_table_truncates_overlong_fields_to_their_columns() {
        // AmigaE StringF `\s[n]` truncates to the field width; the
        // legacy table relies on this to keep columns aligned.
        let long_from = "abcdefghijklmnopqrstuvwxyz0123456789"; // 36 chars > 29
        let long_subject = "this subject is far too long to fit"; // > 21
        let rows = vec![row(7, MailVisibility::Public, long_from, long_subject)];
        let out = render_scan_listing_table(&rows);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("abcdefghijklmnopqrstuvwxyz012  ")); // 29 chars then 2 spaces
        assert!(!text.contains("3456789")); // tail dropped
        assert!(text.contains("this subject is far t  ")); // 21 chars then 2 spaces
        assert!(text.ends_with("\x1b[0m000007\r\n"));
    }

    #[test]
    fn listing_table_is_empty_when_there_are_no_rows() {
        // The legacy only emits the header once the first match is found
        // (`mailFlag` gate); zero rows print nothing.
        assert!(render_scan_listing_table(&[]).is_empty());
    }

    #[test]
    fn listing_row_status_is_private_for_private_to_sysop() {
        let rows = vec![row(1, MailVisibility::PrivateToSysop, "x", "y")];
        let text = String::from_utf8(render_scan_listing_table(&rows)).unwrap();
        assert!(text.contains("\x1b[0mPrivate  x"));
    }
}
