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

mod join;
mod post_mail;
mod read_mail;
mod reply_forward;
mod scan_mail;
mod sysop_admin;

use std::time::SystemTime;

use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg};
use crate::app::services::AppServices;
use crate::app::session_presenter::format_menu_prompt;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_stats_screen, render_time_line, EXPERT_MODE_DISABLED_LINE, EXPERT_MODE_ENABLED_LINE,
    GOODBYE_LINE, HELP_UNAVAILABLE_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, JOIN_REQUIRES_NUMBER_LINE, QUIET_MODE_OFF_LINE,
    QUIET_MODE_ON_LINE, READ_REQUIRES_NUMBER_LINE, UNKNOWN_COMMAND_LINE, VERSION_BANNER,
};
use crate::domain::session::typed::{LoggingOffSession, MenuSession};

use self::join::ExplicitJoinResult;

/// Internal control-flow signal returned by
/// [`MenuFlow::dispatch`]: either the loop continues with the supplied
/// live [`MenuSession`], or it terminates with the supplied
/// [`LoggingOffSession`].
enum DispatchOutcome {
    Continue(MenuSession),
    LogoffComplete(LoggingOffSession),
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
                self.services.bbs_name(),
                self.services.conferences(),
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
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
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

    /// Routes one parsed command to the matching handler. Returns
    /// either the live [`MenuSession`] (loop continues) or the
    /// [`LoggingOffSession`] terminal value (loop exits).
    async fn dispatch(
        &mut self,
        mut session: MenuSession,
        command: MenuCommand,
    ) -> Result<DispatchOutcome, T::Error> {
        match command {
            MenuCommand::Logoff => {
                let logging_off = session.user_requests_logoff();
                // SCREEN_LOGOFF (amiexpress/express.e:6554, displayed
                // at :8187): sysop-supplied pre-goodbye splash. The
                // adapter returns empty bytes when the asset is
                // absent, so this is a no-op on a fresh install.
                // Idle-timeout / account-lock / carrier exits use
                // their dedicated goodbye lines and never reach this
                // branch — matching the legacy.
                let logoff_screen = self.services.screens().logoff_screen().await;
                if !logoff_screen.is_empty() {
                    self.terminal.write(&logoff_screen).await?;
                }
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(DispatchOutcome::LogoffComplete(logging_off));
            }
            MenuCommand::Join(arg) => match arg {
                NumberArg::Number(n) => {
                    session = match self.handle_explicit_join(session, n).await? {
                        ExplicitJoinResult::Joined(menu) => menu,
                        ExplicitJoinResult::NoAccess(logging_off) => {
                            return Ok(DispatchOutcome::LogoffComplete(logging_off));
                        }
                    };
                }
                NumberArg::Missing => self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?,
                NumberArg::Invalid => self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?,
            },
            MenuCommand::Read(arg) => match arg {
                NumberArg::Number(n) => self.handle_read_mail(&mut session, n).await?,
                NumberArg::Missing => self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?,
                NumberArg::Invalid => self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?,
            },
            MenuCommand::Scan(scan) => self.handle_scan_mail(&mut session, scan).await?,
            MenuCommand::Post(post) => self.handle_post_mail(&mut session, post).await?,
            MenuCommand::CommentToSysop => self.handle_comment_to_sysop(&mut session).await?,
            MenuCommand::Reply(arg) => self.handle_reply(&mut session, arg).await?,
            MenuCommand::Forward(arg) => self.handle_forward(&mut session, arg).await?,
            MenuCommand::Kill(arg) => self.handle_kill(&mut session, arg).await?,
            MenuCommand::Move(arg) => self.handle_move_mail(&mut session, arg).await?,
            MenuCommand::EditHeader(arg) => self.handle_edit_header(&mut session, arg).await?,
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
            MenuCommand::ShowStats => {
                // Tier A quickwin A3 (`S`): the baseline user-stats
                // screen from `internalCommandS()`
                // (`amiexpress/express.e:25540`), reading the fields
                // already present on the logged-on user.
                let user = session.user();
                let screen = render_stats_screen(
                    user.slot_number(),
                    user.last_call(),
                    user.access_level(),
                    user.times_called(),
                    user.times_called_today(),
                    user.messages_posted(),
                );
                self.write_and_flush(&screen).await?;
            }
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
                let screen = self.services.screens().topic_help(&topic).await;
                if !screen.is_empty() {
                    self.write_and_flush(&screen).await?;
                }
            }
            MenuCommand::Unknown => self.terminal.write(UNKNOWN_COMMAND_LINE).await?,
        }
        Ok(DispatchOutcome::Continue(session))
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        let timeout = self.services.session_policy().input_timeout();
        crate::app::terminal::read_prompted(self.terminal, prompt, echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        crate::app::terminal::write_and_flush(self.terminal, bytes).await
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
                    .screens()
                    .conference_menu(conf, access_level)
                    .await
            }
            None => self.services.screens().default_menu(access_level).await,
        }
    }

    /// Tier A quickwin A5 (`H`): write the on-disk `BBSHelp.txt`
    /// asset if present, or the legacy
    /// `Sorry Help is unavailable at this time.` line when the
    /// adapter returns empty bytes (`amiexpress/express.e:25079-25085`).
    async fn handle_show_help(&mut self) -> Result<(), T::Error> {
        let bytes = self.services.screens().bbs_help_screen().await;
        if bytes.is_empty() {
            self.write_and_flush(HELP_UNAVAILABLE_LINE).await
        } else {
            self.terminal.write(&bytes).await?;
            self.terminal.flush().await
        }
    }
}
