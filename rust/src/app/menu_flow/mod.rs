//! Menu sub-flow: command loop and dispatch.
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
mod pager;
mod post_mail;
mod read_mail;
mod read_subprompt;
mod reply_forward;
mod scan_all_mail;
mod sysop_admin;
#[cfg(test)]
mod tests;

use std::time::SystemTime;

use self::scan_all_mail::ScanFilter;
use crate::app::mail_stores::{MailStoreGuard, MailStores};
use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg};
use crate::app::services::AppServices;
use crate::app::session_presenter::format_menu_prompt;
use crate::app::terminal::{KeyRead, Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_stats_screen, render_time_line, ANSI_COLOR_OFF_LINE, ANSI_COLOR_ON_LINE, CRLF,
    EXPERT_MODE_DISABLED_LINE, EXPERT_MODE_ENABLED_LINE, GOODBYE_LINE, HELP_UNAVAILABLE_LINE,
    IDLE_TIMEOUT_LINE, INVALID_MESSAGE_NUMBER_LINE, LEAVE_FLAGGED_CONFIRM, QUIET_MODE_OFF_LINE,
    QUIET_MODE_ON_LINE, UNKNOWN_COMMAND_LINE, VERSION_BANNER, YESNO_NO_ECHO, YESNO_YES_ECHO,
};
use crate::app::yes_no::{yes_no, YesNo};
use crate::domain::conference::{
    find_msgbase_in, AllowedAddressing, Conference, MessageBase, MessageBaseRef,
};
use crate::domain::session::typed::{LoggingOffSession, MenuSession};
use crate::domain::user::Right;

/// Internal control-flow signal returned by
/// [`MenuFlow::dispatch`]: either the loop continues with the supplied
/// live [`MenuSession`], or it terminates with the supplied
/// [`LoggingOffSession`].
enum DispatchOutcome {
    Continue(MenuSession),
    LogoffComplete(LoggingOffSession),
}

/// Outcome of the plain-`G` flagged-file leave confirm
/// (`amiexpress/express.e:12667` `checkFlagged` + `:2129` `yesNo`).
enum LeaveFlagged {
    /// `N` / default — keep the caller in the menu.
    Stay,
    /// `Y` — proceed to logoff.
    Leave,
    /// The peer dropped mid-confirm (carrier loss).
    Disconnected,
    /// No key arrived before the input timeout.
    TimedOut,
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

    /// Runs the menu loop until the session reaches a logoff state.
    pub(crate) async fn run(
        &mut self,
        mut session: MenuSession,
    ) -> Result<LoggingOffSession, T::Error> {
        loop {
            // Tier A quickwin A6: in expert mode the menu screen is not
            // auto-displayed before the prompt — the user requests it
            // with `?` (legacy `displayMenuPrompt` gate at
            // `amiexpress/express.e:28583`).
            if !session.user().expert_mode() {
                let menu_bytes = self.render_menu_screen(&session).await;
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
            let read = self.read_prompted(&prompt, TerminalEcho::Visible).await?;
            let line = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => return Ok(session.into_active().apply_carrier_loss()),
                TerminalRead::IdleTimedOut => {
                    let logoff = session
                        .into_active()
                        .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(logoff);
                }
            };
            let trimmed = line.trim();
            match self.dispatch(session, parse_menu_command(trimmed)).await? {
                DispatchOutcome::Continue(next) => session = next,
                DispatchOutcome::LogoffComplete(logoff) => return Ok(logoff),
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
    ) -> Result<(), T::Error> {
        if session.quick_logon() {
            return Ok(());
        }
        self.handle_scan_all_mail(session, ScanFilter::MailScanFlagged)
            .await
    }

    /// Renders the baseline user-stats screen — Tier A quickwin A3
    /// (`S`), the `internalCommandS()` layout (`amiexpress/express.e:25540`)
    /// — reading the fields already present on the logged-on user.
    async fn handle_show_stats(&mut self, session: &MenuSession) -> Result<(), T::Error> {
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

    /// `G` / `G Y`: logoff with the legacy flagged-file confirm.
    ///
    /// Plain `G` with a non-empty session flag set runs `checkFlagged()`
    /// (`amiexpress/express.e:25053`, `:12667`): `N`/default keeps the
    /// caller in the menu; `Y`, the `G Y` force form (`auto`), or an
    /// empty flag set fall straight through to logoff. Persisting the
    /// flags (`saveFlagged`/`saveHistory`) is slice D5.
    async fn handle_logoff(
        &mut self,
        mut session: MenuSession,
        auto: bool,
    ) -> Result<DispatchOutcome, T::Error> {
        if !auto && !session.flagged_files_mut().is_empty() {
            match self.confirm_leave_flagged().await? {
                LeaveFlagged::Stay => {
                    // mystat=0 path: one CRLF, back to the menu
                    // (`amiexpress/express.e:25060`).
                    self.write_newline().await?;
                    return Ok(DispatchOutcome::Continue(session));
                }
                LeaveFlagged::Leave => {}
                LeaveFlagged::Disconnected => {
                    return Ok(DispatchOutcome::LogoffComplete(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                LeaveFlagged::TimedOut => {
                    let logoff = session
                        .into_active()
                        .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(DispatchOutcome::LogoffComplete(logoff));
                }
            }
        }
        let logging_off = session.user_requests_logoff();
        // SCREEN_LOGOFF (amiexpress/express.e:6554, displayed at :8187):
        // sysop-supplied pre-goodbye splash. The adapter returns empty
        // bytes when the asset is absent, so this is a no-op on a fresh
        // install. Idle-timeout / account-lock / carrier exits use their
        // dedicated goodbye lines and never reach this branch.
        let logoff_screen = self.services.screens.as_ref().logoff_screen().await;
        if !logoff_screen.is_empty() {
            self.terminal.write(&logoff_screen).await?;
        }
        self.write_and_flush(GOODBYE_LINE).await?;
        Ok(DispatchOutcome::LogoffComplete(logging_off))
    }

    /// Routes one parsed command to the matching handler. Returns
    /// either the live [`MenuSession`] (loop continues) or the
    /// [`LoggingOffSession`] terminal value (loop exits).
    async fn dispatch(
        &mut self,
        mut session: MenuSession,
        command: MenuCommand,
    ) -> Result<DispatchOutcome, T::Error> {
        match command {
            MenuCommand::Logoff { auto } => return self.handle_logoff(session, auto).await,
            MenuCommand::Join(arg) => {
                // Tier C C2: a direct in-range argument joins
                // immediately; everything else opens the legacy
                // interactive `Conference Number (1-N): ` prompt
                // (`amiexpress/express.e:25142-25154`). Both arms
                // return the session — explicit join never logs the
                // caller off.
                session = self.handle_join_command(session, arg).await?;
            }
            MenuCommand::PrevConference => {
                // Tier C C3 (`<`): nearest lower-numbered accessible
                // conference at its primary message base, or the
                // interactive join prompt at the bottom edge
                // (`internalCommandLT`, `amiexpress/express.e:24529-24546`).
                session = self.handle_prev_conference(session).await?;
            }
            MenuCommand::NextConference => {
                // Tier C C3 (`>`): the upward mirror
                // (`internalCommandGT`, `amiexpress/express.e:24548-24564`).
                session = self.handle_next_conference(session).await?;
            }
            MenuCommand::JoinMsgBase(arg) => {
                // Tier C C4a (`JM`): join a message base of the current
                // conference (`internalCommandJM`,
                // `amiexpress/express.e:25185-25237`). Single-base
                // conferences fail with the legacy notice; an in-range
                // argument runs the full join sequence.
                session = self.handle_join_msgbase_command(session, arg).await?;
            }
            MenuCommand::PrevMsgBase => {
                // Tier C C4b (`<<`): step to the previous message base
                // of the current conference, falling into the `JM`
                // no-arg flow past the bottom (`internalCommandLT2`,
                // `amiexpress/express.e:24566-24578`).
                session = self.handle_prev_msgbase(session).await?;
            }
            MenuCommand::NextMsgBase => {
                // Tier C C4b (`>>`): the upward mirror
                // (`internalCommandGT2`, `amiexpress/express.e:24580-24592`).
                session = self.handle_next_msgbase(session).await?;
            }
            MenuCommand::Read(arg) => match arg {
                NumberArg::Number(n) => self.handle_read_mail(&mut session, n).await?,
                // Bare `R` opens the sub-prompt at the read-resume point
                // (legacy `readMSG` no-arg entry, `express.e:11984-11985`).
                NumberArg::Missing => self.handle_read_mail_at_pointer(&mut session).await?,
                NumberArg::Invalid => self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?,
            },
            MenuCommand::ScanAllMail => {
                self.handle_scan_all_mail(&mut session, ScanFilter::AllConferences)
                    .await?;
            }
            MenuCommand::Post(post) => self.handle_post_mail(&mut session, post).await?,
            MenuCommand::CommentToSysop => self.handle_comment_to_sysop(&mut session).await?,
            MenuCommand::ShowTime => {
                self.write_and_flush(&render_time_line(SystemTime::now()))
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
            MenuCommand::ShowStats => self.handle_show_stats(&session).await?,
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
                    let menu_bytes = self.render_menu_screen(&session).await;
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
                    self.handle_conference_flags(&mut session).await?;
                } else {
                    self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
                }
            }
            MenuCommand::FileList(arg) => {
                // Slice D2 (`F`): the NextScan file lister — AquaScan
                // door parity with NextScan branding
                // (`comparison/evidence-tierD/live-observations.md`).
                self.handle_file_list(&mut session, arg).await?;
            }
            // Slice D4 (`Z`): the internal zippy text search — see
            // `file_list::handle_zippy_search` for the parity record.
            MenuCommand::ZippySearch(arg) => self.handle_zippy_search(&mut session, arg).await?,
            MenuCommand::Unknown => self.terminal.write(UNKNOWN_COMMAND_LINE).await?,
        }
        Ok(DispatchOutcome::Continue(session))
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        let timeout = self.services.session_policy.input_timeout();
        crate::app::terminal::read_prompted(self.terminal, prompt, echo, timeout).await
    }

    /// Flushes pending output, then reads one keystroke in hot-key
    /// mode with the session's input timeout (slice D2b — the
    /// `NextScan` pager prompts act per key).
    async fn read_key(&mut self) -> Result<crate::app::terminal::KeyRead, T::Error> {
        let timeout = self.services.session_policy.input_timeout();
        self.terminal.flush().await?;
        self.terminal.read_key(timeout).await
    }

    /// The flagged-file leave confirm: `checkFlagged()`'s prompt
    /// (`amiexpress/express.e:12670`) plus `yesNo(2)`'s single-key read
    /// (`:2129`). The hot-key adapter echoes nothing, so this owns the
    /// `Yes`/`No` echo; CR defaults to `No`, and unrecognised keys loop
    /// (the legacy `LOOP`/`readChar` at `:2140`).
    async fn confirm_leave_flagged(&mut self) -> Result<LeaveFlagged, T::Error> {
        self.write_and_flush(LEAVE_FLAGGED_CONFIRM).await?;
        loop {
            let key = match self.read_key().await? {
                KeyRead::Key(key) => key,
                KeyRead::Eof => return Ok(LeaveFlagged::Disconnected),
                KeyRead::IdleTimedOut => return Ok(LeaveFlagged::TimedOut),
            };
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

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        crate::app::terminal::write_and_flush(self.terminal, bytes).await
    }

    /// Writes a single line terminator ([`CRLF`]) and flushes — the
    /// common "blank line / end the current line" emit, named so the
    /// bare `b"\r\n"` literal does not recur at call sites.
    async fn write_newline(&mut self) -> Result<(), T::Error> {
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
    async fn handle_show_help(&mut self) -> Result<(), T::Error> {
        let bytes = self.services.screens.as_ref().bbs_help_screen().await;
        if bytes.is_empty() {
            self.write_and_flush(HELP_UNAVAILABLE_LINE).await
        } else {
            self.terminal.write(&bytes).await?;
            self.terminal.flush().await
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
