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

use std::time::SystemTime;

use self::scan_all_mail::ScanFilter;
use crate::app::mail_stores::{MailStoreGuard, MailStores};
use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg};
use crate::app::services::AppServices;
use crate::app::session_presenter::format_menu_prompt;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_stats_screen, render_time_line, ANSI_COLOR_OFF_LINE, ANSI_COLOR_ON_LINE,
    EXPERT_MODE_DISABLED_LINE, EXPERT_MODE_ENABLED_LINE, GOODBYE_LINE, HELP_UNAVAILABLE_LINE,
    IDLE_TIMEOUT_LINE, INVALID_MESSAGE_NUMBER_LINE, QUIET_MODE_OFF_LINE, QUIET_MODE_ON_LINE,
    UNKNOWN_COMMAND_LINE, VERSION_BANNER,
};
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
                let logoff_screen = self.services.screens.as_ref().logoff_screen().await;
                if !logoff_screen.is_empty() {
                    self.terminal.write(&logoff_screen).await?;
                }
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(DispatchOutcome::LogoffComplete(logging_off));
            }
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

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::file_screen_repository::FileScreenRepository;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
    use crate::domain::password::PasswordHashKind;
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

    #[derive(Default)]
    struct CaptureTerminal {
        output: Vec<u8>,
    }

    impl Terminal for CaptureTerminal {
        type Error = Infallible;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            Box::pin(async move {
                self.output.extend_from_slice(bytes);
                Ok(())
            })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(
            &mut self,
            _echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async { Ok(TerminalRead::Eof) })
        }
    }

    fn test_services() -> AppServices {
        AppServices {
            user_repo: Arc::new(InMemoryUserRepository::default()),
            hasher: Arc::new(Pbkdf2PasswordHasher::new()),
            caller_log: Arc::new(InMemoryCallerLog::new()),
            screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
            conferences: Arc::new(Vec::new()),
            mail_stores: Arc::new(InMemoryMailStores::new()),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            session_policy: SessionPolicy::default(),
            default_ratio: DefaultRatio {
                mode: RatioMode::Disabled,
                value: 0,
            },
            new_user_gate: Arc::new(NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            }),
            bbs_name: Arc::from("Test BBS"),
        }
    }

    /// Builds a menu-phase session with the `quick_logon` flag set.
    fn quick_logon_menu_session() -> MenuSession {
        let user = User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user");
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("alice", user)
            .expect("identify");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("password match");
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        session.set_quick_logon(true);
        MenuSession::from_session(session)
    }

    #[tokio::test]
    async fn quick_logon_skips_the_logon_conference_scan() {
        // Spec `messaging.allium:ScanConferencesOnLogon` gates on
        // `not quick_logon`; a quick logon must skip the scan entirely —
        // not even the `Scanning conferences for mail...` header is
        // written. (Pins `MenuSession::quick_logon`: a mutant forcing it
        // to `false` would let the scan run and emit the header.)
        let services = test_services();
        let mut terminal = CaptureTerminal::default();
        let mut menu = quick_logon_menu_session();
        {
            let mut flow = super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.run_logon_conference_scan(&mut menu)
                .await
                .expect("scan");
        }
        assert!(
            terminal.output.is_empty(),
            "a quick logon must skip the logon conference scan, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }
}
