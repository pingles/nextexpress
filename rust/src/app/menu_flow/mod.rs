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
mod scan_mail;

use std::time::SystemTime;

use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg};
use crate::app::services::AppServices;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    GOODBYE_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE, INVALID_MESSAGE_NUMBER_LINE,
    JOIN_REQUIRES_NUMBER_LINE, MENU_PROMPT, READ_REQUIRES_NUMBER_LINE, UNKNOWN_COMMAND_LINE,
};
use crate::domain::session::typed::{LoggingOffSession, MenuSession};

use self::join::ExplicitJoinResult;

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
            let access_level = session.user().access_level();
            let menu_bytes = match session.current_conference_number() {
                Some(conf) => {
                    self.services
                        .screens()
                        .conference_menu(conf, access_level)
                        .await
                }
                None => self.services.screens().default_menu(access_level).await,
            };
            self.terminal.write(&menu_bytes).await?;
            let read = self
                .read_prompted(MENU_PROMPT, TerminalEcho::Visible)
                .await?;
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
            match parse_menu_command(trimmed) {
                MenuCommand::Logoff => {
                    let logging_off = session.user_requests_logoff();
                    self.write_and_flush(GOODBYE_LINE).await?;
                    return Ok(logging_off);
                }
                MenuCommand::Join(arg) => match arg {
                    NumberArg::Number(n) => {
                        session = match self.handle_explicit_join(session, n).await? {
                            ExplicitJoinResult::Joined(menu) => menu,
                            ExplicitJoinResult::NoAccess(logging_off) => {
                                return Ok(logging_off);
                            }
                        };
                    }
                    NumberArg::Missing => {
                        self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?;
                    }
                    NumberArg::Invalid => {
                        self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
                    }
                },
                MenuCommand::Read(arg) => match arg {
                    NumberArg::Number(n) => {
                        self.handle_read_mail(&mut session, n).await?;
                    }
                    NumberArg::Missing => {
                        self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?;
                    }
                    NumberArg::Invalid => {
                        self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
                    }
                },
                MenuCommand::Scan(scan) => {
                    self.handle_scan_mail(&mut session, scan).await?;
                }
                MenuCommand::Post(post) => {
                    self.handle_post_mail(&mut session, post).await?;
                }
                MenuCommand::CommentToSysop => {
                    self.handle_comment_to_sysop(&mut session).await?;
                }
                MenuCommand::Unknown => {
                    self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
                }
            }
        }
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
}
