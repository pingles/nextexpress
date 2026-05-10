//! Menu sub-flow: command loop and dispatch.
//!
//! Runs once the session is onboarded and joined to a conference.
//! Reads command lines, dispatches the supported ones (Phase 4
//! supports `G` for logoff and `J <num>` for explicit conference
//! join) and reports back to the driver when the loop terminates.

use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::typed_session::{ExplicitJoinTransition, LoggingOffSession, MenuSession};
use crate::app::wire_text::{
    GOODBYE_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE, JOIN_REQUIRES_NUMBER_LINE,
    MENU_PROMPT, NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, NO_CONFERENCE_ACCESS_LINE,
    UNKNOWN_COMMAND_LINE,
};

/// Parsed shape of a `J <number>` command. Returned by
/// [`parse_join_command`].
enum JoinArg {
    /// `J <n>` where `<n>` parsed as a `u32`.
    Number(u32),
    /// `J` (or `J ` / `J\t`) with no number.
    Missing,
    /// `J <token>` where `<token>` could not be parsed as a `u32`.
    Invalid,
}

/// Outcome of [`MenuFlow::handle_explicit_join`]. The success branch
/// returns the still-Menu-state session so the menu loop continues;
/// failure terminates with `LogoffReason::NoConferenceAccess`.
enum ExplicitJoinResult {
    /// The user is now attached to a (possibly fallback) conference.
    Joined(MenuSession),
    /// The user lost their last membership; the session is closing.
    NoAccess(LoggingOffSession),
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
            if trimmed.eq_ignore_ascii_case("G") {
                let logging_off = session.user_requests_logoff();
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(logging_off);
            }
            if let Some(arg) = parse_join_command(trimmed) {
                match arg {
                    JoinArg::Number(n) => {
                        session = match self.handle_explicit_join(session, n).await? {
                            ExplicitJoinResult::Joined(menu) => menu,
                            ExplicitJoinResult::NoAccess(logging_off) => return Ok(logging_off),
                        };
                    }
                    JoinArg::Missing => {
                        self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?;
                    }
                    JoinArg::Invalid => {
                        self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
                    }
                }
                continue;
            }
            self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
        }
    }

    /// Handles a `J <num>` command from the menu (Slice 32). Writes
    /// the legacy "no access" notice when the resolver fell through,
    /// the inline `Joining Conference: <name>` announcement on
    /// success, and any name-type promotion screen (Slice 34).
    async fn handle_explicit_join(
        &mut self,
        session: MenuSession,
        target_conference_number: u32,
    ) -> Result<ExplicitJoinResult, T::Error> {
        let conferences = self.services.conferences();
        let outcome = session.explicit_join_conference(
            target_conference_number,
            conferences,
            SystemTime::now(),
        );
        match outcome {
            ExplicitJoinTransition::Joined {
                session,
                conference_number,
                msgbase_number,
                matched_request,
                name_type_promoted_to,
                ..
            } => {
                // Compute the announcement bytes up-front so the
                // immutable borrow on `self.services.conferences()`
                // doesn't overlap the mutable borrows below.
                let line =
                    format_explicit_join_line(conferences, conference_number, msgbase_number);
                if !matched_request {
                    self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                        .await?;
                }
                self.write_and_flush(&line).await?;
                render_name_type_promotion(
                    self.terminal,
                    self.services.screens(),
                    name_type_promoted_to,
                )
                .await?;
                Ok(ExplicitJoinResult::Joined(session))
            }
            ExplicitJoinTransition::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(ExplicitJoinResult::NoAccess(logging_off))
            }
        }
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        self.terminal.write(prompt).await?;
        self.terminal.flush().await?;
        let timeout = self.services.session_policy().input_timeout();
        self.terminal.read_line(echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        self.terminal.write(bytes).await?;
        self.terminal.flush().await
    }
}

/// Recognises the Phase-4 `J` / `J <num>` menu command. Returns
/// `None` for any other typed line so the menu loop can fall
/// through to its existing dispatch (currently only `G`). Mirrors
/// the legacy parsing in `amiexpress/express.e:25140` modulo the
/// `getInverse` macro, which Phase 4 doesn't model yet.
fn parse_join_command(line: &str) -> Option<JoinArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("J") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(JoinArg::Missing);
    };
    if tokens.next().is_some() {
        // Extra trailing tokens are treated as a malformed argument
        // rather than silently ignored.
        return Some(JoinArg::Invalid);
    }
    match arg.parse::<u32>() {
        Ok(n) => Some(JoinArg::Number(n)),
        Err(_) => Some(JoinArg::Invalid),
    }
}
