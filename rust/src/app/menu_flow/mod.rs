//! Menu sub-flow: command loop and dispatch.
//!
//! The connection driver owns the phase-typed [`MenuSession`]. This flow and
//! every command handler borrow it for same-phase work, returning only control
//! metadata. EOF and idle timeout propagate from any nested prompt as a
//! connection-level [`MenuExit`]; only an explicit blank submission can be a
//! command-local cancellation.
//!
//! Runs once the session is onboarded and joined to a conference.
//! Reads command lines, dispatches the supported ones (`G` for logoff,
//! `J <num>` for explicit conference join, `R <num>` for reading a
//! message, `M`/`N` for scanning, `E`/`E <to>` for posting, `C` for
//! comment-to-sysop) and reports back to the driver when the loop
//! terminates.
//!
//! Per-command handlers live in sibling files (`read_mail`, `scan_mail`,
//! `post_mail`, `join`) as `impl<'a, T: Terminal> MenuFlow<'a, T>`
//! blocks, so this file stays focused on the dispatch loop plus the
//! shared terminal-I/O helpers.

mod conf_flags;
mod file_list;
mod join;
mod list_messages;
pub(crate) mod mail_text;
mod pager;
mod post_mail;
mod read_mail;
mod read_subprompt;
mod reply_forward;
mod scan_all_mail;
mod sysop_admin;
pub(crate) mod table;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use self::scan_all_mail::ScanFilter;
use crate::app::mail_stores::{MailStoreGuard, MailStores};
use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg};
use crate::app::services::AppServices;
use crate::app::session_presenter::{format_menu_prompt, render_stats_screen};
use crate::app::terminal::{KeyEvent, KeyRead, Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{CRLF, INVALID_MESSAGE_NUMBER_LINE};
use crate::app::yes_no::{yes_no, YesNo};
use crate::domain::conference::{
    find_msgbase_in, AllowedAddressing, Conference, MessageBase, MessageBaseRef,
};
use crate::domain::session::typed::MenuSession;
use crate::domain::user::Right;

/// The invariant tail of the menu prompt rendered by
/// `render_menu_prompt` — `mins. left): ` (Tier A quickwin A4). The
/// leading BBS name, conference block and minute count vary per
/// session, but this suffix is constant, so it is the marker tests
/// drain on to detect "the menu is awaiting a command". Test-only: the
/// menu loop renders the full prompt via `render_menu_prompt` rather
/// than referencing this constant.
#[cfg(test)]
pub(crate) const MENU_PROMPT_SUFFIX: &[u8] = b"mins. left): ";

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
const VERSION_BANNER: &[u8] = concat!(
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
const HELP_UNAVAILABLE_LINE: &[u8] = b"\r\n\r\nSorry Help is unavailable at this time.\r\n\r\n";

/// Sent when the `Q` command (Tier A quickwin A9) flips the session
/// into quiet mode. Verbatim from `amiexpress/express.e:25509`'s
/// `\b\nQuiet Mode On\b\n` (Amiga `\b\n` → telnet `\r\n`).
const QUIET_MODE_ON_LINE: &[u8] = b"\r\nQuiet Mode On\r\n";

/// Sent when the `Q` command (Tier A quickwin A9) flips the session
/// back out of quiet mode. Verbatim from
/// `amiexpress/express.e:25511`'s `\b\nQuiet Mode Off\b\n`.
const QUIET_MODE_OFF_LINE: &[u8] = b"\r\nQuiet Mode Off\r\n";

/// Sent when the `X` command (Tier A quickwin A6) turns expert mode on.
/// Verbatim from `amiexpress/express.e:26118`'s
/// `\b\nExpert mode enabled\b\n` (Amiga `\b\n` → telnet `\r\n`).
const EXPERT_MODE_ENABLED_LINE: &[u8] = b"\r\nExpert mode enabled\r\n";

/// Sent when the `X` command (Tier A quickwin A6) turns expert mode
/// off. Verbatim from `amiexpress/express.e:26115`'s
/// `\b\nExpert mode disabled\b\n`.
const EXPERT_MODE_DISABLED_LINE: &[u8] = b"\r\nExpert mode disabled\r\n";

/// Sent when the `M` command (Tier A quickwin A8) turns ANSI colour on.
/// Verbatim from `amiexpress/express.e:25247`'s
/// `\b\nAnsi Color On\b\n` (Amiga `\b\n` → telnet `\r\n`).
const ANSI_COLOR_ON_LINE: &[u8] = b"\r\nAnsi Color On\r\n";

/// Sent when the `M` command (Tier A quickwin A8) turns ANSI colour
/// off. Verbatim from `amiexpress/express.e:25243`'s
/// `\b\nAnsi Color Off\b\n`.
const ANSI_COLOR_OFF_LINE: &[u8] = b"\r\nAnsi Color Off\r\n";

/// Sent for unrecognised menu commands.
const UNKNOWN_COMMAND_LINE: &[u8] = b"Unknown command. Type G to log off.\r\n";

/// `higherAccess()` (`amiexpress/express.e:3038`,
/// `'\b\nCommand requires higher access.\b\n'`) — the dispatcher's generic
/// denial, printed on `RESULT_NOT_ALLOWED` (`express.e:28400`). Legacy `\b`
/// renders `\r` on the wire — byte-pinned in `ae_tierd_fs.txt`. Generic by
/// design: any future ACS-gated command reuses this line, so it lives with
/// the dispatcher, not in `file_list/wire.rs`.
const HIGHER_ACCESS_LINE: &[u8] = b"\r\nCommand requires higher access.\r\n";

/// Sent immediately before the connection closes on a normal logoff.
const GOODBYE_LINE: &[u8] = b"Goodbye!\r\n";

/// `loadFlagged`'s restore notice (`amiexpress/express.e:2792-2793`),
/// emitted on logon when a non-empty flag set is restored: blank line,
/// the banner, then the `sendBELL` BEL and a trailing CRLF —
/// structurally identical to [`AUTOSAVING_FILE_FLAGS`]. Live-captured at
/// login in `comparison/transcripts/ae_tierd_alterflags.txt:77-81`.
const FLAGGED_FILES_EXIST: &[u8] = b"\r\n** Flagged File(s) Exist **\r\n\x07\r\n";

/// `saveFlagged`'s autosave announcement (`amiexpress/express.e:2803`),
/// emitted on every `G` logoff (the banner precedes saveFlagged's own
/// flag-count gate, so it shows even with nothing flagged): a blank
/// line, the banner, then the `sendBELL` BEL (`\x07`, valid UTF-8) and a
/// trailing CRLF. Live-captured with files flagged
/// (`comparison/transcripts/ae_tierd_g_confirm.txt:177`) and empty
/// (`comparison/transcripts/ae_tierd_g_empty.txt`). Persisting the flags
/// themselves (the per-slot `flagged` file) is slice D5-persist.
const AUTOSAVING_FILE_FLAGS: &[u8] = b"\r\n** AutoSaving File Flags **\r\n\x07\r\n";

/// `showFlags`'s empty-set line (`amiexpress/express.e:12488`), printed
/// by `A` (slice D6a) when nothing is flagged. Live-captured in
/// `comparison/transcripts/ae_tierd_alterflags.txt`.
const NO_FILE_FLAGS: &str = "No file flags";

/// `flagFiles`'s main prompt (`amiexpress/express.e:12601`): the
/// `Filename(s) to flag: (F)rom, (C)lear, (Enter)=none? ` line the `A`
/// loop renders after each listing (slice D6b). The legacy `[..m`
/// embedded-ESC colour codes are emitted verbatim as `\x1b[..m` (valid
/// UTF-8). Live-captured in
/// `comparison/transcripts/ae_tierd_alterflags.txt:114`.
const FLAG_PROMPT: &[u8] =
    b"\x1b[36mFilename(s) to flag: \x1b[32m(\x1b[33mF\x1b[32m)\x1b[36mrom, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none\x1b[0m? ";

/// `flagFiles`'s clear sub-prompt (`amiexpress/express.e:12614`): the
/// `Filename(s) to Clear: (*)All, (Enter)=none? ` line shown when the
/// caller types bare `C` at [`FLAG_PROMPT`] (slice D6b). Live-captured
/// in `comparison/transcripts/ae_tierd_alterflags.txt:122`.
const CLEAR_PROMPT: &[u8] =
    b"\x1b[36mFilename(s) to Clear: \x1b[32m(\x1b[33m*\x1b[32m)\x1b[36mAll, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none\x1b[0m? ";

/// The `checkFlagged()` leave-confirm prompt
/// (`amiexpress/express.e:12670`) followed by `yesNo(2)`'s own ANSI
/// `(y/N)? ` suffix (`:2134`). Server bytes, live-captured
/// (`comparison/transcripts/ae_tierd_g_confirm.txt:146`); the legacy
/// `\b\n` line breaks are re-encoded to telnet `\r\n` (AGENTS.md wire
/// policy).
const LEAVE_FLAGGED_CONFIRM: &[u8] =
    b"\r\nYou have flagged files still not downloaded.\r\nDo you leave without them? \x1b[32m(\x1b[33my\x1b[32m/\x1b[33mN\x1b[32m)\x1b[32m?\x1b[0m ";

/// `yesNo`'s single-key echo on a `Y` answer (`amiexpress/express.e:2148`).
const YESNO_YES_ECHO: &[u8] = b"Yes\r\n";

/// `yesNo`'s single-key echo on an `N` / default answer
/// (`amiexpress/express.e:2152`).
const YESNO_NO_ECHO: &[u8] = b"No\r\n";

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
fn render_time_line(at: std::time::SystemTime) -> Vec<u8> {
    let formatted = time::OffsetDateTime::from(at)
        .format(TIME_FORMAT)
        .expect("TIME_FORMAT is total over OffsetDateTime");
    format!("\r\nIt is {formatted}\r\n").into_bytes()
}

/// Renders one `showFlags` listing (`amiexpress/express.e:12486`) for the
/// `A` loop: the space-joined upper-cased flagged names — or
/// [`NO_FILE_FLAGS`] when empty — closed by a blank line. The leading
/// blank line is alterFlags's, emitted once before the loop, so this
/// renders only `<body>\r\n`.
fn render_flag_listing(session: &MenuSession) -> Vec<u8> {
    let names: Vec<&str> = session.flagged_files().names().collect();
    let body = if names.is_empty() {
        NO_FILE_FLAGS.to_string()
    } else {
        names.join(" ")
    };
    format!("{body}\r\n").into_bytes()
}

/// Whether `token` is the legacy `flagFiles` clear family
/// (`amiexpress/express.e:12609`): a `C`/`c` first character followed by
/// nothing or a space. The caller has already trimmed, so a bare `C` is
/// length 1 and an inline clear is `C <arg>`.
fn is_clear_family(token: &str) -> bool {
    let mut chars = token.chars();
    matches!(chars.next(), Some('C' | 'c')) && matches!(chars.next(), None | Some(' '))
}

/// Internal control-flow signal returned by [`MenuFlow::dispatch`].
///
/// The session remains borrowed in both cases. A confirmed logoff is an intent
/// for the driver, which alone consumes [`MenuSession`] into the logging-off
/// phase.
enum DispatchOutcome {
    Continue,
    UserRequestedLogoff,
}

/// A lifecycle exit observed anywhere inside the menu command tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuExit {
    /// The caller confirmed a normal `G` / `G Y` logoff.
    UserRequestedLogoff,
    /// The terminal reached EOF while the menu or a nested prompt owned input.
    CarrierLost,
    /// No input arrived before the session input timeout.
    IdleTimedOut,
}

/// Internal error propagated through every nested menu prompt.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MenuFlowError<E> {
    /// A lifecycle exit, distinct from command-level cancellation.
    Exit(MenuExit),
    /// The terminal adapter failed while the menu still owned the session.
    Terminal(E),
}

impl<E> From<E> for MenuFlowError<E> {
    fn from(source: E) -> Self {
        Self::Terminal(source)
    }
}

/// Result type shared by all menu handlers so EOF/idle cannot be collapsed
/// into a local command abort.
pub(crate) type MenuFlowResult<T, E> = Result<T, MenuFlowError<E>>;

/// What a blank submission means at a single-line prompt — one of the
/// two axes the old hand-rolled readers differed on (SYSTEM.md
/// item 10; the other is [`AbortNotice`]).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyMeaning {
    /// Blank aborts the current command locally (the composer's
    /// `read_required_line` semantics). EOF and idle timeout instead exit the
    /// connection through [`MenuFlowError::Exit`].
    Abort,
    /// Blank means "keep the current value" (`EH`'s keep branch).
    Keep,
    /// Blank is a legitimate answer, returned as an empty entry (the
    /// `To:` reroute-to-ALL and the `A` flag loop's `<CR>`=none rely
    /// on blank being distinguishable from EOF / idle).
    Verbatim,
}

/// Whether the aborted path of [`MenuFlow::prompt_line`] writes a
/// notice. The legacy is split: the `E`/`C` composer prints
/// `Message aborted.`; `editHeader` and the `R` sub-prompt family
/// abort silently (`express.e:11602`, B6).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbortNotice {
    /// Write nothing on abort.
    Silent,
    /// Write [`mail_text::POST_ABORTED_LINE`] on abort.
    MessageAborted,
}

/// Outcome of [`MenuFlow::prompt_line`].
enum PromptLine {
    /// A trimmed submission — empty only under
    /// [`EmptyMeaning::Verbatim`].
    Entered(String),
    /// Blank under [`EmptyMeaning::Keep`].
    Kept,
    /// Blank under [`EmptyMeaning::Abort`]. EOF and idle timeout propagate as
    /// [`MenuFlowError::Exit`] instead.
    Aborted,
}

/// Outcome of the plain-`G` flagged-file leave confirm
/// (`amiexpress/express.e:12667` `checkFlagged` + `:2129` `yesNo`).
enum LeaveFlagged {
    /// `N` / default — keep the caller in the menu.
    Stay,
    /// `Y` — proceed to logoff.
    Leave,
}

/// Control signal returned by one [`MenuFlow::flag_files_once`] pass to
/// the `alterFlags` REPEAT loop (`amiexpress/express.e:12659-12662`),
/// mapping the legacy `flagFiles` return code (`stat`).
enum FlagLoop {
    /// `stat < 0` — `alterFlags` returns at once with no trailing blank
    /// line because a new file was flagged (`RESULT_FAILURE`, `:12642`).
    /// Connection exits bypass this command-local outcome.
    Exit,
    /// `stat = 0` (`RESULT_SUCCESS`) — the REPEAT loop ends; `alterFlags`
    /// emits its trailing blank line (`<CR>`=none, `:12646`/`:12618`).
    Done,
    /// `stat = 1` — keep looping: a clear happened (`:12623`), so the
    /// next pass re-shows the (now changed) listing.
    Again,
}

/// Menu sub-flow.
pub(crate) struct MenuFlow<'a, T>
where
    T: Terminal,
{
    terminal: &'a mut T,
    services: &'a AppServices,
}

impl<'a, T> MenuFlow<'a, T>
where
    T: Terminal,
{
    /// Constructs a flow that drives `terminal` against the supplied
    /// driven adapters.
    pub(crate) fn new(terminal: &'a mut T, services: &'a AppServices) -> Self {
        Self { terminal, services }
    }

    /// Runs the menu loop until a lifecycle exit is requested or observed.
    pub(crate) async fn run(
        &mut self,
        session: &mut MenuSession,
    ) -> MenuFlowResult<MenuExit, T::Error> {
        loop {
            // Accrue wall-clock time spent since the last iteration
            // against the per-call budget so the prompt's "mins. left"
            // decrements (item 27a). The exhausted flag drives the
            // expiry logoff in item 27b; here we only refresh the
            // displayed budget, so it is deliberately unused.
            let _budget_exhausted = session.accrue_time(self.services.clock.now());
            // Tier A quickwin A6: in expert mode the menu screen is not
            // auto-displayed before the prompt — the user requests it
            // with `?` (legacy `displayMenuPrompt` gate at
            // `amiexpress/express.e:28583`).
            if !session.user().expert_mode() {
                let menu_bytes = self.render_menu_screen(session).await;
                self.terminal.write(&menu_bytes).await?;
            }
            // Tier A quickwin A4: the legacy `displayMenuPrompt`
            // (`amiexpress/express.e:28404`) renders the BBS name, the
            // current conference and the per-call minutes remaining.
            let prompt = format_menu_prompt(
                self.services.bbs_name.as_ref(),
                self.services.conferences.as_ref(),
                session.current_msgbase(),
                session.time_remaining(),
            );
            let line = self.read_prompted(&prompt, TerminalEcho::Visible).await?;
            session.record_input(self.services.clock.now());
            let trimmed = line.trim();
            match self.dispatch(session, parse_menu_command(trimmed)).await? {
                DispatchOutcome::Continue => {}
                DispatchOutcome::UserRequestedLogoff => {
                    return Ok(MenuExit::UserRequestedLogoff);
                }
            }
        }
    }

    /// Runs the logon conference scan (legacy `confScan`,
    /// `amiexpress/express.e:28066`, driven before the menu opens at
    /// `:28564`): the same multi-conference [`Self::handle_scan_all_mail`]
    /// walk the `MS` command renders — header, per-conference banner,
    /// listing table and the read-it-now offer — but restricted to
    /// conferences whose membership has `mail_scan` set
    /// ([`ScanFilter::MailScanFlagged`], the legacy `checkMailConfScan`
    /// gate). Skipped on a quick logon, mirroring the spec
    /// `messaging.allium:ScanConferencesOnLogon`'s `not quick_logon`
    /// guard. The walk scans by coordinate and never opens or moves the
    /// session's visit, so the caller's home conference (resolved by the
    /// auto-rejoin) is preserved.
    pub(crate) async fn run_logon_conference_scan(
        &mut self,
        session: &mut MenuSession,
    ) -> MenuFlowResult<(), T::Error> {
        if session.quick_logon() {
            return Ok(());
        }
        self.handle_scan_all_mail(session, ScanFilter::MailScanFlagged)
            .await
    }

    /// Restores the user's saved flag set on logon (legacy `loadFlagged`,
    /// `amiexpress/express.e:2757`) and, when the restored set is
    /// non-empty, emits the `** Flagged File(s) Exist **` banner
    /// (`:2791-2794`) — the logon analogue of the logoff autosave banner.
    /// A load error logs and leaves the set empty; the caller still
    /// reaches the menu (slice D5-persist).
    pub(crate) async fn restore_flags_and_announce(
        &mut self,
        session: &mut MenuSession,
    ) -> MenuFlowResult<(), T::Error> {
        let slot = session.user().slot_number();
        match self.services.flagged_store.load(slot) {
            Ok(restored) => *session.flagged_files_mut() = restored,
            Err(error) => {
                eprintln!("loadFlagged: could not restore flags for slot {slot}: {error}");
            }
        }
        if !session.flagged_files_mut().is_empty() {
            self.write_and_flush(FLAGGED_FILES_EXIST).await?;
        }
        Ok(())
    }

    /// `A` — the legacy `alterFlags(NIL)` (`amiexpress/express.e:12648`,
    /// reached via `internalCommandA` `:24601`). Emits a leading blank
    /// line, then loops `flagFiles(NIL)` (`:12659-12662`): each pass shows
    /// the flag set (slice D6a `showFlags`) and prompts. A bare `<CR>`
    /// (=none) or a no-op token ends the loop with a trailing blank line;
    /// flagging a new file exits immediately (`IF(stat<0) THEN RETURN`,
    /// `:12661`); a dropped/idle caller exits the connection directly;
    /// `C` -> `*` clears and loops
    /// (slice D6b). `F`-from and clear-by-name are deferred.
    async fn handle_alter_flags(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        // alterFlags's leading `aePuts('\b\n')` (express.e:12651).
        self.write_and_flush(CRLF).await?;
        loop {
            match self.flag_files_once(session).await? {
                // stat < 0: a new file was flagged — alterFlags returns
                // with no trailing blank line. Connection exits propagate
                // without becoming a FlagLoop value.
                FlagLoop::Exit => return Ok(()),
                // stat = 0 (RESULT_SUCCESS): the REPEAT loop ends.
                FlagLoop::Done => break,
                // stat = 1: a clear happened — re-show and re-prompt.
                FlagLoop::Again => {}
            }
        }
        // alterFlags's trailing `aePuts('\b\n')` (express.e:12664).
        self.write_and_flush(CRLF).await
    }

    /// One pass of the legacy `flagFiles(NIL)` (`amiexpress/express.e:12594`):
    /// renders the current flag set (`showFlags`), prompts with
    /// [`FLAG_PROMPT`], reads one line, and acts on it. Returns the
    /// control signal for the [`Self::handle_alter_flags`] REPEAT loop.
    async fn flag_files_once(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<FlagLoop, T::Error> {
        // showFlags() (express.e:12598): `<names>` space-joined (or
        // `No file flags`) closed by a blank line — alterFlags's leading
        // `\b\n` already opened the frame.
        let listing = render_flag_listing(session);
        self.write_and_flush(&listing).await?;
        let answer = match self
            .prompt_line(
                session,
                FLAG_PROMPT,
                EmptyMeaning::Verbatim,
                AbortNotice::Silent,
            )
            .await?
        {
            PromptLine::Entered(answer) => answer,
            // Retained for the exhaustive PromptLine match. Verbatim blank
            // input is `Entered("")`; EOF/idle propagate before this point.
            PromptLine::Aborted => return Ok(FlagLoop::Exit),
            PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
        };

        // `<CR>` (=none): StrLen 0 falls through to RESULT_SUCCESS,
        // ending the loop (express.e:12646).
        if answer.is_empty() {
            return Ok(FlagLoop::Done);
        }
        // `C` / `c` (bare, or `C ` which trims to `C`): the clear family
        // (express.e:12609). Inline `C <arg>` and the sub-prompt resolve
        // a target; `*` clears all, a name (removeFlagFromList) is
        // deferred for slice D6b.
        if is_clear_family(&answer) {
            return self.flag_clear(session, &answer).await;
        }
        // (`F`-from, express.e:12625, is deferred for slice D6b.)
        //
        // A filename token: addFlagToList (express.e:12638), owned by
        // `MenuSession::flag_file`. A newly flagged file returns
        // RESULT_FAILURE (stat=2 -> -1), exiting the loop with no
        // trailing line; a no-op (too short, or already flagged) falls
        // through to RESULT_SUCCESS.
        if session.flag_file(&answer) {
            Ok(FlagLoop::Exit)
        } else {
            Ok(FlagLoop::Done)
        }
    }

    /// The `C`lear path of `flagFiles` (`amiexpress/express.e:12609-12623`).
    /// `token` is the user's clear-family line (`C`, `C ` or `C <arg>`).
    /// Bare `C` renders [`CLEAR_PROMPT`] and reads the target; an inline
    /// `C <arg>` uses the argument directly (no sub-prompt, no blank
    /// line). `*` clears every flag; an empty sub-prompt answer ends the
    /// loop (`RESULT_SUCCESS`); clearing-by-name is deferred. Always loops
    /// (`RETURN 1`) once a clear is resolved.
    async fn flag_clear(
        &mut self,
        session: &mut MenuSession,
        token: &str,
    ) -> crate::app::menu_flow::MenuFlowResult<FlagLoop, T::Error> {
        // Inline `C <arg>` (express.e:12610): strip the `C ` prefix.
        let target = if token.len() > 1 {
            token[1..].trim().to_ascii_uppercase()
        } else {
            // Bare `C`: the clear sub-prompt + lineInput (express.e:12614).
            let answer = match self
                .prompt_line(
                    session,
                    CLEAR_PROMPT,
                    EmptyMeaning::Verbatim,
                    AbortNotice::Silent,
                )
                .await?
            {
                PromptLine::Entered(answer) => answer,
                PromptLine::Aborted => return Ok(FlagLoop::Exit),
                PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
            };
            // Empty -> RESULT_SUCCESS, no clear, end the loop (:12618).
            if answer.is_empty() {
                return Ok(FlagLoop::Done);
            }
            // The post-input `aePuts('\b\n')` (:12619), only on the bare-C
            // path with a non-empty answer.
            self.write_and_flush(CRLF).await?;
            answer.to_ascii_uppercase()
        };
        // `*` -> clearFlagItems (:12622). A name -> removeFlagFromList,
        // deferred for slice D6b (treated as a no-op so the listing
        // re-renders unchanged).
        if target.starts_with('*') {
            session.flagged_files_mut().clear();
        }
        Ok(FlagLoop::Again)
    }

    /// Renders the baseline user-stats screen — Tier A quickwin A3
    /// (`S`), the `internalCommandS()` layout (`amiexpress/express.e:25540`)
    /// — reading the fields already present on the logged-on user.
    async fn handle_show_stats(
        &mut self,
        session: &MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        let user = session.user();
        let screen = render_stats_screen(
            user.slot_number(),
            user.last_call(),
            user.access_level(),
            user.times_called(),
            user.times_called_today(),
            user.messages_posted(),
        );
        self.write_and_flush(&screen).await
    }

    /// `G` / `G Y`: prepare a normal logoff with the legacy flagged-file
    /// confirm.
    ///
    /// Plain `G` with a non-empty session flag set runs `checkFlagged()`
    /// (`amiexpress/express.e:25053`, `:12667`): `N`/default keeps the
    /// caller in the menu; `Y`, the `G Y` force form (`auto`), or an
    /// empty flag set return a logoff intent. This handler borrows the menu
    /// session for confirmation and flag persistence; the driver performs the
    /// consuming `Menu -> LoggingOff` transition.
    async fn handle_logoff(
        &mut self,
        session: &mut MenuSession,
        auto: bool,
    ) -> MenuFlowResult<DispatchOutcome, T::Error> {
        if !auto && !session.flagged_files_mut().is_empty() {
            match self.confirm_leave_flagged().await? {
                LeaveFlagged::Stay => {
                    // mystat=0 path: one CRLF, back to the menu
                    // (`amiexpress/express.e:25060`).
                    self.write_newline().await?;
                    return Ok(DispatchOutcome::Continue);
                }
                LeaveFlagged::Leave => {}
            }
        }
        // saveFlagged (express.e:25064 -> :2803) runs on every `G`
        // logoff and prints the autosave banner + BEL unconditionally —
        // even with nothing flagged (the banner sits before saveFlagged's
        // own count gate). Live-confirmed for the empty case in
        // `comparison/transcripts/ae_tierd_g_empty.txt`. Only the Stay
        // branch above returns early and skips it. Persisting the flags
        // is slice D5-persist.
        self.write_and_flush(AUTOSAVING_FILE_FLAGS).await?;
        // D5-persist: saveFlagged writes the set to the durable store
        // (express.e:2806). A store error is logged, never fatal.
        let slot = session.user().slot_number();
        if let Err(error) = self
            .services
            .flagged_store
            .save(slot, session.flagged_files())
        {
            eprintln!("saveFlagged: could not persist flags for slot {slot}: {error}");
        }
        Ok(DispatchOutcome::UserRequestedLogoff)
    }

    /// Writes the normal-logoff screen and goodbye after the driver has
    /// consumed the menu phase into `LoggingOff`.
    ///
    /// The driver retains that `LoggingOffSession` around this operation, so a
    /// failed screen or goodbye write still reaches the single finalisation
    /// boundary with the original logoff reason.
    ///
    /// # Errors
    /// Returns the terminal adapter's error when the logoff screen, goodbye,
    /// or final flush cannot be written.
    pub(crate) async fn write_logoff_tail(&mut self) -> Result<(), T::Error> {
        // SCREEN_LOGOFF (amiexpress/express.e:6554, displayed at :8187):
        // sysop-supplied pre-goodbye splash. The adapter returns empty
        // bytes when the asset is absent, so this is a no-op on a fresh
        // install. Idle-timeout / account-lock / carrier exits use their
        // dedicated goodbye lines and never reach this branch.
        let logoff_screen = self.services.screens.as_ref().logoff_screen().await;
        if !logoff_screen.is_empty() {
            self.terminal.write(&logoff_screen).await?;
        }
        crate::app::terminal::write_and_flush(self.terminal, GOODBYE_LINE).await
    }

    /// Routes one parsed command to the matching handler while borrowing the
    /// driver's live [`MenuSession`]. Returns only whether the loop continues
    /// or the caller requested normal logoff; lifecycle transitions remain
    /// driver-owned.
    async fn dispatch(
        &mut self,
        session: &mut MenuSession,
        command: MenuCommand,
    ) -> MenuFlowResult<DispatchOutcome, T::Error> {
        match command {
            MenuCommand::Logoff { auto } => return self.handle_logoff(session, auto).await,
            MenuCommand::Join(arg) => {
                // Tier C C2: a direct in-range argument joins
                // immediately; everything else opens the legacy
                // interactive `Conference Number (1-N): ` prompt
                // (`amiexpress/express.e:25142-25154`). Both arms mutate the
                // borrowed session in place — explicit join never changes
                // its lifecycle phase.
                self.handle_join_command(session, arg).await?;
            }
            MenuCommand::PrevConference => {
                // Tier C C3 (`<`): nearest lower-numbered accessible
                // conference at its primary message base, or the
                // interactive join prompt at the bottom edge
                // (`internalCommandLT`, `amiexpress/express.e:24529-24546`).
                self.handle_prev_conference(session).await?;
            }
            MenuCommand::NextConference => {
                // Tier C C3 (`>`): the upward mirror
                // (`internalCommandGT`, `amiexpress/express.e:24548-24564`).
                self.handle_next_conference(session).await?;
            }
            MenuCommand::JoinMsgBase(arg) => {
                // Tier C C4a (`JM`): join a message base of the current
                // conference (`internalCommandJM`,
                // `amiexpress/express.e:25185-25237`). Single-base
                // conferences fail with the legacy notice; an in-range
                // argument runs the full join sequence.
                self.handle_join_msgbase_command(session, arg).await?;
            }
            MenuCommand::PrevMsgBase => {
                // Tier C C4b (`<<`): step to the previous message base
                // of the current conference, falling into the `JM`
                // no-arg flow past the bottom (`internalCommandLT2`,
                // `amiexpress/express.e:24566-24578`).
                self.handle_prev_msgbase(session).await?;
            }
            MenuCommand::NextMsgBase => {
                // Tier C C4b (`>>`): the upward mirror
                // (`internalCommandGT2`, `amiexpress/express.e:24580-24592`).
                self.handle_next_msgbase(session).await?;
            }
            MenuCommand::Read(arg) => match arg {
                NumberArg::Number(n) => self.handle_read_mail(session, n).await?,
                // Bare `R` opens the sub-prompt at the read-resume point
                // (legacy `readMSG` no-arg entry, `express.e:11984-11985`).
                NumberArg::Missing => self.handle_read_mail_at_pointer(session).await?,
                NumberArg::Invalid => self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?,
            },
            MenuCommand::ScanAllMail => {
                self.handle_scan_all_mail(session, ScanFilter::AllConferences)
                    .await?;
            }
            MenuCommand::Post(post) => self.handle_post_mail(session, post).await?,
            MenuCommand::CommentToSysop => self.handle_comment_to_sysop(session).await?,
            MenuCommand::ShowTime => {
                self.write_and_flush(&render_time_line(self.services.clock.now()))
                    .await?;
            }
            MenuCommand::ShowVersion => self.write_and_flush(VERSION_BANNER).await?,
            MenuCommand::ShowHelp => self.handle_show_help().await?,
            MenuCommand::QuietToggle => {
                // Tier A quickwin A9 (`Q`): toggle `Session.quiet_mode`
                // and emit the legacy on/off literal at
                // `amiexpress/express.e:25506-25512`. The flag's effect
                // on OLM/join broadcasts lands with cmds-comm.md.
                let line = if session.toggle_quiet_mode() {
                    QUIET_MODE_ON_LINE
                } else {
                    QUIET_MODE_OFF_LINE
                };
                self.write_and_flush(line).await?;
            }
            MenuCommand::ShowStats => self.handle_show_stats(session).await?,
            MenuCommand::ExpertToggle => {
                // Tier A quickwin A6 (`X`): flip the user's expert flag
                // and emit the legacy on/off literal at
                // `amiexpress/express.e:26115-26118`. The flip is
                // persisted with the user record on logoff; in expert
                // mode the menu loop stops auto-displaying the menu.
                let line = if session.toggle_expert_mode() {
                    EXPERT_MODE_ENABLED_LINE
                } else {
                    EXPERT_MODE_DISABLED_LINE
                };
                self.write_and_flush(line).await?;
            }
            MenuCommand::ShowMenu => {
                // Tier A quickwin A7 (`?`): re-display the conference
                // menu, but only in expert mode — outside it the loop
                // has just displayed the menu anyway
                // (`amiexpress/express.e:24595`).
                if session.user().expert_mode() {
                    let menu_bytes = self.render_menu_screen(session).await;
                    self.write_and_flush(&menu_bytes).await?;
                }
            }
            MenuCommand::TopicHelp(topic) => {
                // Tier A quickwin A10 (`^`): display the topic help
                // screen, truncating the topic until a screen matches
                // (`amiexpress/express.e:25089`). An empty topic or a
                // topic with no matching screen is a silent no-op.
                let screen = self.services.screens.as_ref().topic_help(&topic).await;
                if !screen.is_empty() {
                    self.write_and_flush(&screen).await?;
                }
            }
            MenuCommand::AnsiToggle => {
                // Tier A quickwin A8 (`M`): flip the live ANSI colour
                // mode on the terminal and emit the legacy on/off
                // literal (`amiexpress/express.e:25243-25247`). While
                // colour is off, the ColourTerminal decorator strips
                // ANSI SGR escapes from every subsequent write.
                let enabled = !self.terminal.ansi_colour();
                self.terminal.set_ansi_colour(enabled);
                let line = if enabled {
                    ANSI_COLOR_ON_LINE
                } else {
                    ANSI_COLOR_OFF_LINE
                };
                self.write_and_flush(line).await?;
            }
            MenuCommand::ConferenceFlags => {
                // Tier C C5 (`CF`): edit the caller's own per-conference
                // scan flags, gated on the legacy `ACS_CONFFLAGS` right
                // (`amiexpress/express.e:24686`). A user without it — an
                // awaiting-validation new user — sees the unknown-command
                // notice, since `CF` is not part of their menu.
                if session.user().has_access(Right::EditConferenceFlags) {
                    self.handle_conference_flags(session).await?;
                } else {
                    self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
                }
            }
            MenuCommand::FileList(arg) => {
                // Slice D2 (`F`): the NextScan file lister — AquaScan
                // door parity with NextScan branding
                // (`comparison/evidence-tierD/live-observations.md`).
                self.handle_file_list(session, arg).await?;
            }
            // Slice D4 (`Z`): the internal zippy text search — see
            // `file_list::handle_zippy_search` for the parity record.
            MenuCommand::ZippySearch(arg) => self.handle_zippy_search(session, arg).await?,
            // Slice D9 (`N`): the NextScan new-files scan — the AquaScan
            // door surface (`comparison/transcripts/ae_tierd_newfiles.txt`).
            MenuCommand::NewFilesScan(arg) => self.handle_new_files(session, arg).await?,
            // Slices D6a/D6b (`A`): the `alterFlags` flag listing +
            // `Filename(s) to flag:` prompt loop (`amiexpress/express.e:12648`).
            MenuCommand::AlterFlags => self.handle_alter_flags(session).await?,
            MenuCommand::FileStatus => {
                // Slice D8 (`FS`): internalCommandFS (`express.e:24871-24874`)
                // gates on ACS_CONFERENCE_ACCOUNTING and returns
                // RESULT_NOT_ALLOWED emitting zero bytes; the dispatcher tail
                // (`:28400`) prints higherAccess() (`:3038`). On the shipped
                // board no account holds the right (captured every probe,
                // `ae_tierd_fs.txt`), so FS denies unconditionally here — the
                // deny is reachable without a domain gate (§10.4; see design
                // §7). The granted branch — fileStatus(0) (`:24141`) — is
                // A11's surface; when a slice first makes it reachable it
                // introduces the ACS gate with BOTH branches live (deferred;
                // COMMAND_PARITY Q1, Option G).
                self.terminal.write(HIGHER_ACCESS_LINE).await?;
            }
            MenuCommand::Unknown => self.terminal.write(UNKNOWN_COMMAND_LINE).await?,
        }
        Ok(DispatchOutcome::Continue)
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> MenuFlowResult<String, T::Error> {
        let timeout = self.services.session_policy.input_timeout();
        match crate::app::terminal::read_prompted(self.terminal, prompt, echo, timeout).await? {
            TerminalRead::Line(line) => Ok(line),
            TerminalRead::Eof => Err(MenuFlowError::Exit(MenuExit::CarrierLost)),
            TerminalRead::IdleTimedOut => Err(MenuFlowError::Exit(MenuExit::IdleTimedOut)),
        }
    }

    /// The single line-prompt reader (SYSTEM.md item 10): writes
    /// `prompt`, reads one visible-echo line under the session input
    /// timeout, and — the invariant the hand-rolled copies kept
    /// fraying — stamps the idle clock on every accepted line, so a
    /// new prompt cannot forget `record_input`. Blank-line meaning and
    /// abort-notice policy are the two axes the old readers differed
    /// on; everything else is shared. EOF and idle timeout do not become a
    /// [`PromptLine`]: they propagate immediately as a lifecycle exit.
    async fn prompt_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
        empty: EmptyMeaning,
        notice: AbortNotice,
    ) -> MenuFlowResult<PromptLine, T::Error> {
        let line = self.read_prompted(prompt, TerminalEcho::Visible).await?;
        session.record_input(self.services.clock.now());
        let trimmed = line.trim();
        if trimmed.is_empty() {
            match empty {
                EmptyMeaning::Abort => {
                    self.abort_notice(notice).await?;
                    Ok(PromptLine::Aborted)
                }
                EmptyMeaning::Keep => Ok(PromptLine::Kept),
                EmptyMeaning::Verbatim => Ok(PromptLine::Entered(String::new())),
            }
        } else {
            Ok(PromptLine::Entered(trimmed.to_string()))
        }
    }

    /// Writes the abort notice [`AbortNotice`] selects, if any.
    async fn abort_notice(&mut self, notice: AbortNotice) -> MenuFlowResult<(), T::Error> {
        match notice {
            AbortNotice::Silent => Ok(()),
            AbortNotice::MessageAborted => self.write_and_flush(mail_text::POST_ABORTED_LINE).await,
        }
    }

    /// Flushes pending output, then reads one keystroke in hot-key
    /// mode with the session's input timeout (slice D2b — the
    /// `NextScan` pager prompts act per key). EOF and idle timeout propagate
    /// immediately as a lifecycle exit from any nested hot-key prompt.
    async fn read_key(&mut self) -> MenuFlowResult<KeyEvent, T::Error> {
        let timeout = self.services.session_policy.input_timeout();
        self.terminal.flush().await?;
        match self.terminal.read_key(timeout).await? {
            KeyRead::Key(key) => Ok(key),
            KeyRead::Eof => Err(MenuFlowError::Exit(MenuExit::CarrierLost)),
            KeyRead::IdleTimedOut => Err(MenuFlowError::Exit(MenuExit::IdleTimedOut)),
        }
    }

    /// The flagged-file leave confirm: `checkFlagged()`'s prompt
    /// (`amiexpress/express.e:12670`) plus `yesNo(2)`'s single-key read
    /// (`:2129`). The hot-key adapter echoes nothing, so this owns the
    /// `Yes`/`No` echo; CR defaults to `No`, and unrecognised keys loop
    /// (the legacy `LOOP`/`readChar` at `:2140`).
    async fn confirm_leave_flagged(&mut self) -> MenuFlowResult<LeaveFlagged, T::Error> {
        self.write_and_flush(LEAVE_FLAGGED_CONFIRM).await?;
        loop {
            let key = self.read_key().await?;
            // yesNo(2): CR defaults to No (`amiexpress/express.e:2145`).
            match yes_no(key, YesNo::No) {
                Some(YesNo::Yes) => {
                    self.write_and_flush(YESNO_YES_ECHO).await?;
                    return Ok(LeaveFlagged::Leave);
                }
                Some(YesNo::No) => {
                    self.write_and_flush(YESNO_NO_ECHO).await?;
                    return Ok(LeaveFlagged::Stay);
                }
                // Any other key is ignored, like yesNo's `LOOP`.
                None => {}
            }
        }
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> MenuFlowResult<(), T::Error> {
        Ok(crate::app::terminal::write_and_flush(self.terminal, bytes).await?)
    }

    /// Writes a single line terminator ([`CRLF`]) and flushes — the
    /// common "blank line / end the current line" emit, named so the
    /// bare `b"\r\n"` literal does not recur at call sites.
    async fn write_newline(&mut self) -> MenuFlowResult<(), T::Error> {
        self.write_and_flush(CRLF).await
    }

    /// Renders the conference menu screen for the session's current
    /// conference, preferring the per-conference asset and falling back
    /// to the system-wide default (`ScreenRepository::conference_menu`
    /// / `default_menu`). Shared by the menu loop's auto-display
    /// (Tier A quickwin A6) and the `?` command (A7).
    async fn render_menu_screen(&self, session: &MenuSession) -> Vec<u8> {
        let access_level = session.user().access_level();
        match session.current_conference_number() {
            Some(conf) => {
                self.services
                    .screens
                    .as_ref()
                    .conference_menu(conf, access_level)
                    .await
            }
            None => {
                self.services
                    .screens
                    .as_ref()
                    .default_menu(access_level)
                    .await
            }
        }
    }

    /// Tier A quickwin A5 (`H`): write the on-disk `BBSHelp.txt`
    /// asset if present, or the legacy
    /// `Sorry Help is unavailable at this time.` line when the
    /// adapter returns empty bytes (`amiexpress/express.e:25079-25085`).
    async fn handle_show_help(&mut self) -> MenuFlowResult<(), T::Error> {
        let bytes = self.services.screens.as_ref().bbs_help_screen().await;
        if bytes.is_empty() {
            self.write_and_flush(HELP_UNAVAILABLE_LINE).await
        } else {
            self.terminal.write(&bytes).await?;
            Ok(self.terminal.flush().await?)
        }
    }
}

/// The session's open message-base coordinate, as the
/// [`MessageBaseRef`] the stores and messaging rules consume. `None`
/// when no visit is open.
fn current_base(session: &MenuSession) -> Option<MessageBaseRef> {
    session
        .current_msgbase()
        .map(|(conference, msgbase)| MessageBaseRef::new(conference, msgbase))
}

/// Locks the mail store for the session's current message base —
/// the resolution preamble every mail command shares. `None` when the
/// session has no open visit or no store is registered for the
/// coordinate.
async fn lock_current_base<M>(
    session: &MenuSession,
    mail_stores: &M,
) -> Option<(MessageBaseRef, MailStoreGuard)>
where
    M: MailStores + ?Sized,
{
    let base = current_base(session)?;
    let guard = mail_stores.lock(base).await?;
    Some((base, guard))
}

/// The `allowed_addressing` policy for `base` within the loaded
/// catalogue (Slice 43), or `None` when the coordinate is unknown.
fn allowed_addressing_for(
    conferences: &[Conference],
    base: MessageBaseRef,
) -> Option<AllowedAddressing> {
    find_msgbase_in(conferences, base).map(MessageBase::allowed_addressing)
}
