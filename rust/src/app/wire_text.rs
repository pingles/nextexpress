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

/// Prompt asking whether the user wants ANSI graphics. Simplified from
/// `amiexpress/express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?`
/// — RIP rendering lands in a future toggles slice.
pub(crate) const ANSI_PROMPT: &[u8] = b"Use ANSI graphics? (Y/n) ";

/// Prompt for the sysop-set new-user password gate. Verbatim from
/// `amiexpress/express.e:30018`.
pub(crate) const NEW_USER_PASSWORD_PROMPT: &[u8] = b"Enter New User Password: ";

/// Prompt printed after each menu screen, awaiting a command.
pub(crate) const MENU_PROMPT: &[u8] = b"Command: ";

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

/// Sent when the user types `J <num>` for a conference they don't
/// have access to; the session falls through to the first accessible
/// conference but the listener surfaces this notice first
/// (`amiexpress/express.e:25157`).
pub(crate) const NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE: &[u8] =
    b"\r\nYou do not have access to the requested conference\r\n\r\n";

/// Sent when the user types `J` without a target conference number.
/// The simplified Phase-4 wiring rejects the no-arg form rather than
/// running the `JoinConf` prompt sub-flow; future slices may refine
/// this when the `JoinConf` prompt arrives.
pub(crate) const JOIN_REQUIRES_NUMBER_LINE: &[u8] = b"\r\nUsage: J <conference-number>\r\n";

/// Sent when `J <something>` cannot be parsed as a conference
/// number.
pub(crate) const INVALID_CONFERENCE_NUMBER_LINE: &[u8] = b"\r\nInvalid conference number.\r\n";

/// Sent when the user types `R` without a target message number.
/// The Phase-6 wiring rejects the no-arg form rather than running
/// the legacy `readMSG` prompt sub-flow; future slices may refine
/// this when the prompt arrives.
pub(crate) const READ_REQUIRES_NUMBER_LINE: &[u8] = b"\r\nUsage: R <message-number>\r\n";

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
/// numbered line edits) arrives in Phase 8.
pub(crate) const POST_BODY_PROMPT: &[u8] =
    b"Enter your message. End with a single '.' on a line by itself; '/A' aborts.\r\n";

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
    use time::OffsetDateTime;
    let now = OffsetDateTime::from(at);
    let mut out = Vec::with_capacity(40);
    out.extend_from_slice(b"\r\nIt is ");
    // amiexpress/express.e:25636-25640 — `It is <date> <time>`.
    write_two_digits(&mut out, now.month() as u8);
    out.push(b'-');
    write_two_digits(&mut out, now.day());
    out.push(b'-');
    // year().rem_euclid(100) is in 0..=99, fits in u8.
    let yy = u8::try_from(now.year().rem_euclid(100)).expect("year mod 100 in 0..=99");
    write_two_digits(&mut out, yy);
    out.push(b' ');
    write_two_digits(&mut out, now.hour());
    out.push(b':');
    write_two_digits(&mut out, now.minute());
    out.push(b':');
    write_two_digits(&mut out, now.second());
    out.extend_from_slice(b"\r\n");
    out
}

fn write_two_digits(out: &mut Vec<u8>, value: u8) {
    out.push(b'0' + (value / 10) % 10);
    out.push(b'0' + value % 10);
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
}
