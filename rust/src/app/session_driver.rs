//! Transport-agnostic application driver for an interactive BBS session.
//!
//! Driving adapters provide a [`Terminal`] implementation (see
//! [`crate::app::terminal`]). The driver owns the top-level workflow:
//! it renders the banner, hands off to the
//! [`crate::app::login_flow::LoginFlow`] for sign-in, the
//! [`crate::app::registration_flow::RegistrationFlow`] for new users,
//! [`crate::app::password_reset_flow::PasswordResetFlow`] for forced
//! credential rotation,
//! resolves the auto-rejoin path, drives the
//! [`crate::app::menu_flow::MenuFlow`] command loop, and finalises the
//! session.
//!
//! Wire-format byte constants live in [`crate::app::wire_text`];
//! rendering helpers shared between the join paths live in
//! [`crate::app::session_presenter`].
//!
//! ## Phase types
//! Each step of the workflow consumes and returns a phase wrapper
//! from [`crate::domain::session::typed`]. The wrong handle for a given
//! transition becomes unrepresentable at compile time; the driver no
//! longer needs to assert "session is in X" after every call.
//!
//! Same-phase menu work is deliberately different: the driver retains one
//! [`MenuSession`] and lends `&mut MenuSession` to [`MenuFlow`] and its command
//! handlers. Only a genuine lifecycle transition consumes the wrapper.
//!
//! ## Completion ownership
//! [`Self::run`] is the single connection completion and finalisation boundary.
//! Terminal-driven phase-owning flows return
//! [`crate::app::session_terminal::SessionTerminalError`], coupling the
//! adapter's original error with the exact typed phase that owned the failed
//! operation. The boundary can therefore apply carrier loss to a recoverable
//! phase, retain an already-selected logoff reason, avoid re-finalising an
//! ended session, and still return the original terminal error.

use crate::app::login_flow::{LoginFlow, LoginOutcome};
use crate::app::menu_flow::{MenuExit, MenuFlow, MenuFlowError};
use crate::app::password_reset_flow::{
    PasswordResetFlow, PasswordResetOutcome, PasswordResetServices,
};
use crate::app::registration_flow::{RegistrationFlow, RegistrationOutcome};
use crate::app::services::AppServices;
use crate::app::session_flow;
use crate::app::session_presenter::{
    format_auto_rejoin_line, render_name_type_promotion, render_stats_screen,
};
use crate::app::session_terminal::{preserve_phase, SessionFlowResult, SessionTerminalError};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{IDLE_TIMEOUT_LINE, TIME_EXPIRED_LINE};
use crate::domain::conference::NameType;
use crate::domain::session::typed::{
    AutoRejoinTransition, ConnectingSession, EndedSession, IdentifyingSession, LoggingOffSession,
    MenuSession, OnboardedSession,
};
use crate::domain::session::LogonChannel;
use crate::domain::user_repository::UserRepositoryError;

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
const COPYRIGHT_LINES: &[u8] = concat!(
    "NextExpress (",
    env!("NEXTEXPRESS_GIT_SHA"),
    ") Copyright \u{00A9}2026\r\n",
    "AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n",
)
.as_bytes();

/// Sent when the auto-rejoin / explicit-join flow can't find any
/// conference the user has access to (Slice 30 / Slice 34a). The
/// session terminates with `LogoffReason::NoConferenceAccess`.
const NO_CONFERENCE_ACCESS_LINE: &[u8] = b"\r\nNo accessible conferences. Goodbye.\r\n";

/// App-layer session workflow over a terminal port.
///
/// The driver does not hold an untyped [`crate::domain::session::Session`]
/// field. It owns one typed wrapper at a time as stack-local state threaded
/// through [`Self::run`] and the sub-flow structs in
/// [`crate::app::login_flow`], [`crate::app::registration_flow`],
/// [`crate::app::password_reset_flow`] and [`crate::app::menu_flow`]. That
/// ownership returns to the driver on every normal outcome and every terminal
/// failure.
pub(crate) struct SessionDriver<T>
where
    T: Terminal,
{
    terminal: T,
    services: AppServices,
    node_number: u32,
    channel: LogonChannel,
}

/// Outcome of the sign-in chain (handle + password / registration).
/// Lets [`SessionDriver::run`] decide how to enter the menu vs. how
/// to finalise.
enum SignInResult {
    /// Sign-in produced an authenticated, fully-onboarded session.
    Onboarded(OnboardedSession),
    /// Sign-in moved the session into `LoggingOff` (rejection,
    /// timeout, carrier loss, exhausted retries, ...).
    LoggingOff(LoggingOffSession),
    /// Sign-in ended the session outright (handle retry budget
    /// exhausted moves straight to `Ended`).
    Ended(EndedSession),
}

/// The auto-rejoin's deferred user-visible announcement, captured when
/// the home conference is resolved and replayed by
/// [`SessionDriver::announce_auto_rejoin`] after the logon conference
/// scan has run (the legacy emits it at `SUBSTATE_DISPLAY_CONF_BULL`,
/// `amiexpress/express.e:28574`, after `confScan`).
struct AutoRejoinAnnouncement {
    conference_number: u32,
    msgbase_number: u32,
    name_type_promoted_to: Option<NameType>,
}

enum EnterMenuDriverOutcome {
    Menu(MenuSession),
    LoggingOff(LoggingOffSession),
    Aborted,
}

enum DriverCompletion {
    LoggingOff(Box<LoggingOffSession>),
    Ended,
    Aborted,
}

impl<T> SessionDriver<T>
where
    T: Terminal,
{
    /// Constructs a driver for a newly accepted connection. The
    /// session itself is not constructed until [`Self::run`] starts.
    #[must_use]
    pub(crate) fn new(
        terminal: T,
        node_number: u32,
        channel: LogonChannel,
        services: AppServices,
    ) -> Self {
        Self {
            terminal,
            services,
            node_number,
            channel,
        }
    }

    /// Runs the BBS workflow until the terminal closes or the session reaches
    /// a final logoff path. This is the sole boundary that finalises a
    /// logging-off session; each path reaches it at most once.
    ///
    /// # Errors
    /// Returns the terminal adapter's original error after recovering the
    /// exact phase, applying carrier loss where necessary, and finalising any
    /// resulting `LoggingOffSession`.
    pub(crate) async fn run(&mut self) -> Result<(), T::Error> {
        match self.drive().await {
            Ok(DriverCompletion::LoggingOff(logging_off)) => {
                self.finalise(*logging_off);
                Ok(())
            }
            Ok(DriverCompletion::Ended | DriverCompletion::Aborted) => Ok(()),
            Err(failure) => {
                let (phase, source) = failure.into_parts();
                if let Some(logging_off) = phase.into_logging_off() {
                    self.finalise(logging_off);
                }
                Err(source)
            }
        }
    }

    async fn drive(&mut self) -> SessionFlowResult<DriverCompletion, T::Error> {
        let connecting =
            ConnectingSession::accept(self.node_number, self.channel, 0, self.services.clock.now())
                .expect("freshly allocated node has no existing session");
        let identifying = self.start(connecting).await?;

        let login = LoginFlow::new(&mut self.terminal, &self.services)
            .identify(identifying)
            .await?;
        let signed_in = match login {
            LoginOutcome::Onboarded(onboarded) => SignInResult::Onboarded(onboarded),
            LoginOutcome::LoggingOff(logging_off) => SignInResult::LoggingOff(logging_off),
            LoginOutcome::Ended(ended) => SignInResult::Ended(ended),
            // An unrecoverable persistence failure during sign-in: the
            // session is gone and already logged, so just close.
            LoginOutcome::Aborted => return Ok(DriverCompletion::Aborted),
            LoginOutcome::NeedsRegistration {
                session,
                password_required,
            } => {
                let outcome = RegistrationFlow::new(&mut self.terminal, &self.services)
                    .run(session, password_required)
                    .await?;
                match outcome {
                    RegistrationOutcome::Onboarded(s) => SignInResult::Onboarded(s),
                    RegistrationOutcome::LoggingOff(s) => SignInResult::LoggingOff(s),
                }
            }
        };

        let logging_off = match signed_in {
            SignInResult::Onboarded(onboarded) => {
                // Resolve `conferences.allium:JoinConference` for the
                // auto-rejoin path (Slice 30), attaching the home visit.
                // The JOINED line and name-type promotion screen (Slices
                // 31 / 34) are *captured*, not emitted: the legacy shows
                // them at `SUBSTATE_DISPLAY_CONF_BULL`
                // (`amiexpress/express.e:28574`), *after* the logon
                // conference scan (`confScan`, `:28564`). No scan-on-join
                // fires here — the legacy auto-rejoin join carries
                // `FORCE_MAILSCAN_SKIP` because the logon scan already
                // covered every flagged base.
                let transition = onboarded.auto_rejoin_conference(
                    self.services.conferences.as_ref(),
                    self.services.clock.now(),
                );
                match transition {
                    AutoRejoinTransition::Joined {
                        session,
                        conference_number,
                        msgbase_number,
                        show_bulletin: _,
                        name_type_promoted_to,
                    } => {
                        let announcement = AutoRejoinAnnouncement {
                            conference_number,
                            msgbase_number,
                            name_type_promoted_to,
                        };
                        // Enter the menu state first so the logon conference
                        // scan can reuse the `MenuSession` read-it-now flow —
                        // the legacy runs `confScan` with the user already
                        // fully logged on, before the join announcement.
                        match self.enter_menu_after_password_reset(session).await? {
                            EnterMenuDriverOutcome::Menu(menu) => {
                                self.drive_menu(menu, &announcement).await?
                            }
                            EnterMenuDriverOutcome::LoggingOff(logging_off) => logging_off,
                            EnterMenuDriverOutcome::Aborted => {
                                return Ok(DriverCompletion::Aborted);
                            }
                        }
                    }
                    // The no-access line tells the user why their session
                    // is closing — the caller-log finalise entry already
                    // records `LogoffReason::NoConferenceAccess`.
                    AutoRejoinTransition::NoAccess(logging_off) => {
                        let (logging_off, ()) = preserve_phase(
                            logging_off,
                            crate::app::terminal::write_and_flush(
                                &mut self.terminal,
                                NO_CONFERENCE_ACCESS_LINE,
                            ),
                        )
                        .await?;
                        logging_off
                    }
                }
            }
            SignInResult::LoggingOff(logging_off) => logging_off,
            SignInResult::Ended(_ended) => return Ok(DriverCompletion::Ended),
        };

        Ok(DriverCompletion::LoggingOff(Box::new(logging_off)))
    }

    async fn drive_menu(
        &mut self,
        mut menu: MenuSession,
        announcement: &AutoRejoinAnnouncement,
    ) -> SessionFlowResult<LoggingOffSession, T::Error> {
        if let Err(error) = MenuFlow::new(&mut self.terminal, &self.services)
            .run_logon_conference_scan(&mut menu)
            .await
        {
            return self.complete_menu_error(menu, error).await;
        }

        // Replay the deferred auto-rejoin announcement, then the login stats
        // (read after the `enter_menu` bump, matching the legacy
        // `statPrintUser` order). The driver retains `menu` around every
        // fallible render.
        if let Err(source) = self.announce_auto_rejoin(announcement).await {
            return Err(SessionTerminalError::new(menu, source));
        }
        if let Err(source) = self.render_login_stats(&menu).await {
            return Err(SessionTerminalError::new(menu, source));
        }
        if let Err(error) = MenuFlow::new(&mut self.terminal, &self.services)
            .restore_flags_and_announce(&mut menu)
            .await
        {
            return self.complete_menu_error(menu, error).await;
        }

        match MenuFlow::new(&mut self.terminal, &self.services)
            .run(&mut menu)
            .await
        {
            Ok(exit) => self.complete_menu_exit(menu, exit).await,
            Err(error) => self.complete_menu_error(menu, error).await,
        }
    }

    async fn complete_menu_error(
        &mut self,
        menu: MenuSession,
        error: MenuFlowError<T::Error>,
    ) -> SessionFlowResult<LoggingOffSession, T::Error> {
        match error {
            MenuFlowError::Exit(exit) => self.complete_menu_exit(menu, exit).await,
            MenuFlowError::Terminal(source) => Err(SessionTerminalError::new(menu, source)),
        }
    }

    async fn complete_menu_exit(
        &mut self,
        menu: MenuSession,
        exit: MenuExit,
    ) -> SessionFlowResult<LoggingOffSession, T::Error> {
        match exit {
            MenuExit::UserRequestedLogoff => {
                // The handler returned intent while borrowing MenuSession.
                // This is the sole consuming normal-logoff transition; the
                // tail runs only after the typed phase has changed.
                let logging_off = menu.user_requests_logoff();
                let (logging_off, ()) = preserve_phase(
                    logging_off,
                    MenuFlow::new(&mut self.terminal, &self.services).write_logoff_tail(),
                )
                .await?;
                Ok(logging_off)
            }
            MenuExit::CarrierLost => Ok(menu.into_active().apply_carrier_loss()),
            MenuExit::IdleTimedOut => {
                let logging_off = menu
                    .into_active()
                    .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                let (logging_off, ()) = preserve_phase(
                    logging_off,
                    crate::app::terminal::write_and_flush(&mut self.terminal, IDLE_TIMEOUT_LINE),
                )
                .await?;
                Ok(logging_off)
            }
            MenuExit::TimeExpired => {
                // The per-call budget ran out (item 27b). Like idle
                // timeout, this writes its own self-contained notice
                // (which carries its own goodbye) and never reaches the
                // normal logoff tail. Flag persistence on a forced exit
                // remains a shared gap with idle/carrier — see SYSTEM.md.
                let logging_off = menu.expire_time();
                let (logging_off, ()) = preserve_phase(
                    logging_off,
                    crate::app::terminal::write_and_flush(&mut self.terminal, TIME_EXPIRED_LINE),
                )
                .await?;
                Ok(logging_off)
            }
        }
    }

    /// Replays the deferred auto-rejoin announcement — the JOINED line
    /// (legacy `joinConf`, `amiexpress/express.e:5071-5073`) and any
    /// name-type promotion screen (Slices 31 / 34) — after the logon
    /// conference scan has run.
    async fn announce_auto_rejoin(
        &mut self,
        announcement: &AutoRejoinAnnouncement,
    ) -> Result<(), T::Error> {
        let line = format_auto_rejoin_line(
            self.services.conferences.as_ref(),
            announcement.conference_number,
            announcement.msgbase_number,
        );
        self.terminal.write(&line).await?;
        self.terminal.flush().await?;
        render_name_type_promotion(
            &mut self.terminal,
            self.services.screens.as_ref(),
            announcement.name_type_promoted_to,
        )
        .await
    }

    /// Returns the terminal after the driver has finished. Intended
    /// for tests and adapter-owned cleanup.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn into_terminal(self) -> T {
        self.terminal
    }

    async fn start(
        &mut self,
        connecting: ConnectingSession,
    ) -> SessionFlowResult<IdentifyingSession, T::Error> {
        // Plain connection preamble (the legacy "Running AmiExpress..."
        // lines, `amiexpress/express.e:29514`), shown before the
        // graphics question. The banner / title screen is rendered
        // afterwards by `LoginFlow` so an ASCII caller gets it
        // ANSI-stripped, mirroring the legacy `SCREEN_BBSTITLE` order
        // (`:29552`, after the `A/r/n` question).
        let (connecting, ()) =
            preserve_phase(connecting, self.terminal.write(COPYRIGHT_LINES)).await?;
        Ok(connecting.prompt_for_name())
    }

    /// Renders the user-stats screen during login, mirroring the
    /// legacy `statPrintUser` block shown at logon
    /// (`amiexpress/express.e:29850`). Reuses the same
    /// [`render_stats_screen`] bytes the `S` command emits. Called after
    /// `enter_menu`, so the figures (notably `times_called`) reflect the
    /// logon bump — matching the legacy, which prints the stats once the
    /// user is fully logged on.
    async fn render_login_stats(&mut self, session: &MenuSession) -> Result<(), T::Error> {
        let user = session.user();
        let screen = render_stats_screen(
            user.slot_number(),
            user.last_call(),
            user.access_level(),
            user.times_called(),
            user.times_called_today(),
            user.messages_posted(),
        );
        self.terminal.write(&screen).await?;
        self.terminal.flush().await
    }

    async fn enter_menu_after_password_reset(
        &mut self,
        mut onboarded: OnboardedSession,
    ) -> SessionFlowResult<EnterMenuDriverOutcome, T::Error> {
        loop {
            match self.enter_menu(onboarded) {
                Ok(session_flow::EnterMenuFlowOutcome::Menu(menu)) => {
                    return Ok(EnterMenuDriverOutcome::Menu(menu));
                }
                Ok(session_flow::EnterMenuFlowOutcome::PasswordResetRequired(reset_session)) => {
                    match PasswordResetFlow::new(
                        &mut self.terminal,
                        PasswordResetServices::from(&self.services),
                    )
                    .run(reset_session)
                    .await?
                    {
                        PasswordResetOutcome::Onboarded(session) => {
                            onboarded = session;
                        }
                        PasswordResetOutcome::LoggingOff(logging_off) => {
                            return Ok(EnterMenuDriverOutcome::LoggingOff(logging_off));
                        }
                        PasswordResetOutcome::Aborted => {
                            return Ok(EnterMenuDriverOutcome::Aborted)
                        }
                    }
                }
                Err(error) => {
                    // Persistence failed entering the menu; the caller's
                    // logon state is unsaved, so close the connection
                    // rather than admit them.
                    eprintln!("login: failed to persist user on menu entry: {error}");
                    return Ok(EnterMenuDriverOutcome::Aborted);
                }
            }
        }
    }

    /// Enters the menu, persisting the logon bump.
    fn enter_menu(
        &mut self,
        onboarded: OnboardedSession,
    ) -> Result<session_flow::EnterMenuFlowOutcome, UserRepositoryError> {
        match session_flow::enter_menu(
            onboarded,
            self.services.user_repo.as_ref(),
            self.services.caller_log.as_ref(),
            self.services.clock.now(),
        ) {
            Ok(outcome) => Ok(outcome),
            Err(session_flow::EnterMenuFlowError::Save(error)) => Err(error),
            Err(session_flow::EnterMenuFlowError::Session(error)) => {
                unreachable!("OnboardedSession enter_menu guard failed unexpectedly: {error:?}");
            }
        }
    }

    /// Finalises the logoff, persisting the user's final state. Only
    /// [`Self::run`] calls this completion boundary.
    ///
    /// A persistence failure here is logged but cannot change the
    /// outcome — the session is already closing — so it does not
    /// propagate. The `Session` arm is unreachable: the
    /// `LoggingOffSession` wrapper guarantees the transition.
    fn finalise(&mut self, logging_off: LoggingOffSession) {
        match session_flow::finalise_logoff(
            logging_off,
            self.services.user_repo.as_ref(),
            self.services.caller_log.as_ref(),
            self.services.clock.now(),
        ) {
            Ok(_ended) => {}
            Err(session_flow::FinaliseLogoffFlowError::Save(error)) => {
                eprintln!("logoff: failed to persist user on finalise: {error}");
            }
            Err(session_flow::FinaliseLogoffFlowError::Session(error)) => {
                unreachable!("LoggingOffSession finalises cleanly: {error:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fmt;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::mail_stores::MailStores;
    use crate::app::screens::{ScreenFuture, ScreenRepository};
    use crate::app::services::{AppServices, SharedMailStores, SharedUserRepo};
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::{LogonChannel, SessionPolicy};
    use crate::domain::user::{NewUserDraft, RatioMode, User};
    use crate::domain::user_repository::{
        NameLookupResult, UserCreationError, UserRepository, UserRepositoryError,
    };

    fn test_mail_stores() -> SharedMailStores {
        Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>
    }

    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};

    use super::SessionDriver;

    struct FakeTerminal {
        inputs: VecDeque<TerminalRead>,
        output: Vec<u8>,
        echo_modes: Vec<TerminalEcho>,
        ansi_colour: bool,
        fail_write_containing: Option<Vec<u8>>,
        fail_read_at: Option<usize>,
        fail_flush_at: Option<usize>,
        read_count: usize,
        flush_count: usize,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeTerminalError {
        Write,
        Read,
        Flush,
    }

    impl fmt::Display for FakeTerminalError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "injected terminal {self:?} failure")
        }
    }

    impl std::error::Error for FakeTerminalError {}

    impl FakeTerminal {
        fn new(inputs: impl IntoIterator<Item = TerminalRead>) -> Self {
            Self {
                inputs: inputs.into_iter().collect(),
                output: Vec::new(),
                echo_modes: Vec::new(),
                ansi_colour: true,
                fail_write_containing: None,
                fail_read_at: None,
                fail_flush_at: None,
                read_count: 0,
                flush_count: 0,
            }
        }

        fn failing_write_containing(mut self, needle: &[u8]) -> Self {
            self.fail_write_containing = Some(needle.to_vec());
            self
        }

        fn failing_read_at(mut self, operation: usize) -> Self {
            self.fail_read_at = Some(operation);
            self
        }

        #[allow(dead_code)]
        fn failing_flush_at(mut self, operation: usize) -> Self {
            self.fail_flush_at = Some(operation);
            self
        }

        fn output(&self) -> &[u8] {
            &self.output
        }
    }

    impl Terminal for FakeTerminal {
        type Error = FakeTerminalError;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            Box::pin(async move {
                if self.fail_write_containing.as_deref().is_some_and(|needle| {
                    bytes.windows(needle.len()).any(|window| window == needle)
                }) {
                    return Err(FakeTerminalError::Write);
                }
                self.output.extend_from_slice(bytes);
                Ok(())
            })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async move {
                let operation = self.flush_count;
                self.flush_count += 1;
                if self.fail_flush_at == Some(operation) {
                    return Err(FakeTerminalError::Flush);
                }
                Ok(())
            })
        }

        fn read_line(
            &mut self,
            echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async move {
                self.echo_modes.push(echo);
                let operation = self.read_count;
                self.read_count += 1;
                if self.fail_read_at == Some(operation) {
                    return Err(FakeTerminalError::Read);
                }
                Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof))
            })
        }

        fn ansi_colour(&self) -> bool {
            self.ansi_colour
        }

        fn set_ansi_colour(&mut self, enabled: bool) {
            self.ansi_colour = enabled;
        }
    }

    struct StaticScreens;

    impl ScreenRepository for StaticScreens {
        fn banner(&self) -> ScreenFuture<'_> {
            bytes(b"BANNER\r\n")
        }

        fn default_menu(&self, _access_level: u8) -> ScreenFuture<'_> {
            bytes(b"MENU\r\n")
        }

        fn conference_menu(&self, _conference_number: u32, _access_level: u8) -> ScreenFuture<'_> {
            bytes(b"CONFMENU\r\n")
        }

        fn new_user_password(&self) -> ScreenFuture<'_> {
            bytes(b"NEW USER\r\n")
        }

        fn no_new_users(&self) -> ScreenFuture<'_> {
            bytes(b"NO NEW USERS\r\n")
        }

        fn joinconf_screen(&self) -> ScreenFuture<'_> {
            bytes(b"JOINCONF\r\n")
        }

        fn joinmsgbase_screen(&self, _conference_number: u32) -> ScreenFuture<'_> {
            bytes(b"JOINMSGBASE\r\n")
        }

        fn realnames_screen(&self) -> ScreenFuture<'_> {
            bytes(b"REALNAMES\r\n")
        }

        fn internetnames_screen(&self) -> ScreenFuture<'_> {
            bytes(b"INTERNETNAMES\r\n")
        }

        fn mailscan_screen(&self) -> ScreenFuture<'_> {
            bytes(b"MAILSCAN\r\n")
        }

        fn logoff_screen(&self) -> ScreenFuture<'_> {
            bytes(b"LOGOFF SCREEN\r\n")
        }

        fn bbs_help_screen(&self) -> ScreenFuture<'_> {
            bytes(b"BBSHELP\r\n")
        }

        fn topic_help(&self, _topic: &str) -> ScreenFuture<'_> {
            bytes(b"TOPICHELP\r\n")
        }
    }

    fn bytes(value: &'static [u8]) -> ScreenFuture<'static> {
        Box::pin(async move { value.to_vec() })
    }

    fn alice_with_password(password: &str) -> User {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash(password, PasswordHashKind::Pbkdf210000)
            .expect("compute");
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    /// A [`UserRepository`] whose persist calls (`record_auth_outcome`,
    /// `record_password_change`, `apply_user_patch`) start failing
    /// once `fail_from` of them have run
    /// (0 = every persist fails); reads delegate to an inner
    /// [`InMemoryUserRepository`]. Used to prove the sign-in flow
    /// survives a persistence failure at each point that used to
    /// `.expect()` the save: after the password match
    /// (`verify_password`, call 0), on menu entry (`enter_menu`,
    /// call 1 for a granted user), and on logoff (`finalise_logoff`,
    /// call 1 for an ungranted user that reaches
    /// `NoConferenceAccess`).
    struct SaveFailingRepo {
        inner: InMemoryUserRepository,
        fail_from: usize,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl SaveFailingRepo {
        fn new(inner: InMemoryUserRepository, fail_from: usize) -> Self {
            Self {
                inner,
                fail_from,
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        /// Ordinal gate shared by every persist entry point: the
        /// `fail_from`-th persist call (of any kind) fails.
        fn gate(&self) -> Result<(), UserRepositoryError> {
            use std::sync::atomic::Ordering;
            let nth = self.calls.fetch_add(1, Ordering::SeqCst);
            if nth >= self.fail_from {
                Err(UserRepositoryError::UserNotFound {
                    handle: "save failed".to_string(),
                })
            } else {
                Ok(())
            }
        }
    }

    impl UserRepository for SaveFailingRepo {
        fn find_by_handle(&self, typed: &str) -> Result<NameLookupResult, UserRepositoryError> {
            self.inner.find_by_handle(typed)
        }
        fn find_sysop(&self) -> Result<NameLookupResult, UserRepositoryError> {
            self.inner.find_sysop()
        }
        fn record_auth_outcome(
            &self,
            slot: u32,
            outcome: &crate::domain::user::AuthOutcome,
        ) -> Result<(), UserRepositoryError> {
            self.gate()?;
            self.inner.record_auth_outcome(slot, outcome)
        }
        fn record_password_change(
            &self,
            slot: u32,
            change: &crate::domain::user::PasswordChange,
        ) -> Result<(), UserRepositoryError> {
            self.gate()?;
            self.inner.record_password_change(slot, change)
        }
        fn apply_user_patch(
            &self,
            slot: u32,
            patch: &crate::domain::user::UserPatch,
        ) -> Result<(), UserRepositoryError> {
            self.gate()?;
            self.inner.apply_user_patch(slot, patch)
        }
        fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError> {
            self.inner.create_user(draft)
        }
    }

    #[tokio::test]
    async fn driver_aborts_cleanly_when_user_save_fails_after_password_match() {
        // Regression: a `UserRepository::save` failure after a correct
        // password used to be `.expect()`-ed in `LoginFlow::authenticate`,
        // panicking the connection task. It must instead end the session
        // without panicking and without admitting the caller to the menu.
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(SaveFailingRepo::new(
            InMemoryUserRepository::new(vec![alice]),
            0,
        ));
        let services = services_with(repo, conferences);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        // The bug made this panic; the fix returns cleanly.
        driver
            .run()
            .await
            .expect("driver completes without panicking on a save failure");

        let output = driver.into_terminal().output().to_vec();
        // We reached the password prompt (so the save genuinely fired) ...
        assert!(
            output
                .windows(b"PassWord: ".len())
                .any(|w| w == b"PassWord: "),
            "should have reached the password prompt"
        );
        // ... but the caller was NOT admitted: no auth confirmation, no menu.
        assert!(
            !output
                .windows(b"Authenticated".len())
                .any(|w| w == b"Authenticated"),
            "a save failure must not admit the caller"
        );
        assert!(
            !output.windows(b"MENU".len()).any(|w| w == b"MENU"),
            "a save failure must not reach the menu"
        );
    }

    /// Builds the default test services around a caller-supplied user
    /// repository and conference set. Shared by the driver tests that
    /// only vary the repo and the user's conference grants (save-failure
    /// regressions, forced-reset, graphics-prompt, registration, …).
    fn services_with(
        user_repo: SharedUserRepo,
        conferences: Vec<crate::domain::conference::Conference>,
    ) -> AppServices {
        AppServices {
            user_repo,
            hasher: Arc::new(Pbkdf2PasswordHasher::new()),
            caller_log: Arc::new(InMemoryCallerLog::new()),
            screens: Arc::new(StaticScreens),
            conferences: Arc::new(conferences),
            mail_stores: test_mail_stores(),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            flagged_store: Arc::new(
                crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new(),
            ),
            clock: Arc::new(crate::adapters::system_clock::SystemClock),
            session_policy: SessionPolicy::default(),
            default_ratio: DefaultRatio {
                mode: RatioMode::ByFiles,
                value: 3,
            },
            new_user_gate: Arc::new(NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            }),
            bbs_name: Arc::from("TestBBS"),
        }
    }

    fn authenticated_fixture() -> (
        AppServices,
        Arc<InMemoryUserRepository>,
        Arc<InMemoryCallerLog>,
    ) {
        use crate::domain::conference::{Conference, MessageBase};

        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let caller_log = Arc::new(InMemoryCallerLog::new());
        let mut services = services_with(repo.clone(), conferences);
        services.caller_log = caller_log.clone();
        (services, repo, caller_log)
    }

    fn logoff_entries(caller_log: &InMemoryCallerLog) -> Vec<String> {
        caller_log
            .entries()
            .into_iter()
            .filter(|entry| entry.text.starts_with("Logoff:"))
            .map(|entry| entry.text)
            .collect()
    }

    fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    #[tokio::test]
    async fn preamble_write_failure_finalises_the_connecting_session_once() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([]).failing_write_containing(b"NextExpress");
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Write));

        let entries = logoff_entries(&caller_log);
        assert_eq!(
            entries.len(),
            1,
            "the retained Connecting phase must be finalised exactly once"
        );
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn prompt_flush_failure_returns_original_error_and_finalises_once() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([]).failing_flush_at(0);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Flush));

        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn nested_prompt_eof_is_a_carrier_loss_without_command_abort_or_remenu() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("E".to_string()),
            TerminalRead::Eof,
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("EOF closes the session cleanly");

        let output = driver.into_terminal().output().to_vec();
        assert_eq!(
            occurrences(&output, b"CONFMENU"),
            1,
            "EOF inside the E command must not return to the menu"
        );
        assert_eq!(
            occurrences(&output, crate::app::menu_flow::mail_text::POST_ABORTED_LINE),
            0,
            "carrier loss is not a command-level message abort"
        );
        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1, "carrier loss must finalise exactly once");
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn nested_prompt_idle_timeout_finalises_immediately_with_timeout_reason() {
        let (mut services, _repo, caller_log) = authenticated_fixture();
        services.session_policy = services.session_policy.with_treat_timeout_as_logoff(true);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("E".to_string()),
            TerminalRead::IdleTimedOut,
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver
            .run()
            .await
            .expect("idle timeout closes the session cleanly");

        let output = driver.into_terminal().output().to_vec();
        assert_eq!(occurrences(&output, b"CONFMENU"), 1);
        assert_eq!(
            occurrences(&output, crate::app::menu_flow::mail_text::POST_ABORTED_LINE),
            0
        );
        assert_eq!(
            occurrences(&output, crate::app::wire_text::IDLE_TIMEOUT_LINE),
            1,
            "idle timeout emits only the standard lifecycle notice"
        );
        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1, "idle timeout must finalise exactly once");
        assert!(entries[0].contains("reason input_timeout"), "{entries:?}");
    }

    #[tokio::test]
    async fn terminal_failure_after_menu_mutation_persists_latest_session() {
        let (services, repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("X".to_string()),
        ])
        .failing_write_containing(b"Expert mode enabled");
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Write));

        let alice = match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(user) => user,
            NameLookupResult::NotFound => panic!("alice should still exist"),
        };
        assert!(
            alice.expert_mode(),
            "finalisation must persist the mutation made before output failed"
        );
        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn terminal_read_failure_returns_original_error_and_finalises_once() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
        ])
        .failing_read_at(3);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Read));

        let entries = logoff_entries(&caller_log);
        assert_eq!(
            entries.len(),
            1,
            "the menu phase retained at the failed read must finalise once"
        );
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn terminal_failure_with_ended_session_does_not_finalise_again() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let mut inputs = vec![TerminalRead::Line("Y".to_string())];
        inputs.extend((0..5).map(|_| TerminalRead::Line("nobody".to_string())));
        let terminal =
            FakeTerminal::new(inputs).failing_write_containing(b"Too many failed login attempts");
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Write));
        assert!(
            logoff_entries(&caller_log).is_empty(),
            "a session already in Ended must not pass through finalise_logoff"
        );
    }

    #[tokio::test]
    async fn failed_goodbye_preserves_normal_logoff_and_finalises_once() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ])
        .failing_write_containing(b"Goodbye!");
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Write));

        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1, "normal logoff must finalise exactly once");
        assert!(entries[0].contains("reason normal_logoff"), "{entries:?}");
    }

    #[tokio::test]
    async fn post_login_output_failure_finalises_the_owned_menu_session() {
        let (services, _repo, caller_log) = authenticated_fixture();
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
        ])
        .failing_write_containing(b"Security Lv");
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        assert_eq!(driver.run().await, Err(FakeTerminalError::Write));

        let entries = logoff_entries(&caller_log);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].contains("reason carrier_loss"), "{entries:?}");
    }

    #[tokio::test]
    async fn driver_aborts_cleanly_when_user_save_fails_on_menu_entry() {
        // Same bug class, one step later: `session_flow::enter_menu`
        // persists the logon bump and the driver used to `.expect()` it.
        // A save failure on menu entry must close the connection cleanly,
        // not panic.
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        // verify_password's save (call 0) succeeds; enter_menu's (1) fails.
        let repo = Arc::new(SaveFailingRepo::new(
            InMemoryUserRepository::new(vec![alice]),
            1,
        ));
        let services = services_with(repo, conferences);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver
            .run()
            .await
            .expect("driver completes without panicking on a menu-entry save failure");

        let output = driver.into_terminal().output().to_vec();
        assert!(
            !output.windows(b"MENU".len()).any(|w| w == b"MENU"),
            "a save failure on menu entry must not reach the menu"
        );
    }

    #[tokio::test]
    async fn driver_does_not_panic_when_finalise_save_fails() {
        // `session_flow::finalise_logoff` persists the user on logoff and
        // the driver used to `.expect()` it. An ungranted user reaches
        // NoConferenceAccess -> finalise; the final save failing must be
        // logged, not panic the task.
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        // No grant_all_memberships: alice cannot rejoin -> NoConferenceAccess.
        let alice = alice_with_password("secret");
        // verify_password's save (call 0) succeeds; finalise's (1) fails.
        let repo = Arc::new(SaveFailingRepo::new(
            InMemoryUserRepository::new(vec![alice]),
            1,
        ));
        let services = services_with(repo, conferences);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver
            .run()
            .await
            .expect("driver completes without panicking on a finalise save failure");

        let output = driver.into_terminal().output().to_vec();
        // The no-access notice precedes the failing finalise, confirming we
        // reached the finalise step rather than aborting earlier.
        let no_access = super::NO_CONFERENCE_ACCESS_LINE;
        assert!(
            output.windows(no_access.len()).any(|w| w == no_access),
            "the no-conference-access path should have been taken"
        );
    }

    #[tokio::test]
    async fn driver_runs_forced_password_reset_before_menu_entry() {
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        alice.set_force_password_reset(true);
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let caller_log = Arc::new(InMemoryCallerLog::new());
        let hasher = Arc::new(Pbkdf2PasswordHasher::new());
        let services = AppServices {
            user_repo: repo.clone(),
            hasher: hasher.clone(),
            caller_log: caller_log.clone(),
            screens: Arc::new(StaticScreens),
            conferences: Arc::new(conferences),
            mail_stores: test_mail_stores(),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            flagged_store: Arc::new(
                crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new(),
            ),
            clock: Arc::new(crate::adapters::system_clock::SystemClock),
            session_policy: SessionPolicy::default(),
            default_ratio: DefaultRatio {
                mode: RatioMode::ByFiles,
                value: 3,
            },
            new_user_gate: Arc::new(NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            }),
            bbs_name: Arc::from("TestBBS"),
        };
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("Newpass123".to_string()),
            TerminalRead::Line("Newpass123".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        assert!(
            output
                .windows(crate::app::password_reset_flow::PASSWORD_RESET_REQUIRED_LINE.len())
                .any(|w| w == crate::app::password_reset_flow::PASSWORD_RESET_REQUIRED_LINE),
            "forced reset notice should be shown"
        );
        assert!(
            output.windows(b"MENU".len()).any(|w| w == b"MENU"),
            "successful reset should allow menu entry"
        );
        assert_eq!(
            terminal.echo_modes,
            vec![
                TerminalEcho::Visible,
                TerminalEcho::Visible,
                TerminalEcho::Masked,
                TerminalEcho::Masked,
                TerminalEcho::Masked,
                TerminalEcho::Visible,
            ]
        );
        let loaded = match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(user) => *user,
            NameLookupResult::NotFound => panic!("alice should still exist"),
        };
        assert!(!loaded.force_password_reset());
        assert!(
            hasher
                .verify_password(&loaded, "Newpass123")
                .expect("verify"),
            "new password should authenticate after reset"
        );
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logon:")));
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logoff:")));
    }

    #[tokio::test]
    async fn driver_disconnects_when_forced_password_reset_is_not_completed() {
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        alice.set_force_password_reset(true);
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let services = services_with(repo, conferences);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("secret".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let output = driver.into_terminal().output().to_vec();
        assert!(
            output
                .windows(crate::app::password_reset_flow::PASSWORD_RESET_EXHAUSTED_LINE.len())
                .any(|w| w == crate::app::password_reset_flow::PASSWORD_RESET_EXHAUSTED_LINE),
            "reset exhaustion notice should be shown"
        );
        assert!(
            !output.windows(b"MENU".len()).any(|w| w == b"MENU"),
            "failed reset should not admit the caller"
        );
    }

    #[tokio::test]
    async fn driver_runs_signin_menu_and_logoff_without_a_telnet_transport() {
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let hasher = Arc::new(Pbkdf2PasswordHasher::new());
        let caller_log = Arc::new(InMemoryCallerLog::new());
        let screens = Arc::new(StaticScreens);
        let gate = NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        };
        let ratio = DefaultRatio {
            mode: RatioMode::ByFiles,
            value: 3,
        };
        let services = AppServices {
            user_repo: repo,
            hasher,
            caller_log: caller_log.clone(),
            screens,
            conferences: Arc::new(conferences),
            mail_stores: test_mail_stores(),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            flagged_store: Arc::new(
                crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new(),
            ),
            clock: Arc::new(crate::adapters::system_clock::SystemClock),
            session_policy: SessionPolicy::default(),
            default_ratio: ratio,
            new_user_gate: Arc::new(gate),
            bbs_name: Arc::from("TestBBS"),
        };
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        assert!(output.windows(b"BANNER".len()).any(|w| w == b"BANNER"));
        assert!(output
            .windows(b"PassWord: ".len())
            .any(|w| w == b"PassWord: "));
        assert!(output
            .windows(b"Authenticated".len())
            .any(|w| w == b"Authenticated"));
        assert!(output.windows(b"MENU".len()).any(|w| w == b"MENU"));
        // SCREEN_LOGOFF is rendered before the Goodbye line on a user-
        // requested logoff (G command). The StaticScreens mock returns
        // "LOGOFF SCREEN\r\n"; assert it lands and precedes "Goodbye".
        let logoff_screen_pos = output
            .windows(b"LOGOFF SCREEN".len())
            .position(|w| w == b"LOGOFF SCREEN")
            .expect("SCREEN_LOGOFF should be rendered before goodbye");
        let goodbye_pos = output
            .windows(b"Goodbye".len())
            .position(|w| w == b"Goodbye")
            .expect("goodbye should be rendered");
        assert!(
            logoff_screen_pos < goodbye_pos,
            "SCREEN_LOGOFF must precede the Goodbye line"
        );
        assert_eq!(
            terminal.echo_modes,
            vec![
                // graphics prompt, name, password, menu command (G)
                TerminalEcho::Visible,
                TerminalEcho::Visible,
                TerminalEcho::Masked,
                TerminalEcho::Visible
            ]
        );
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logon:") && entry.text.contains("alice")));
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logoff:") && entry.text.contains("alice")));
    }

    #[tokio::test]
    async fn login_renders_user_stats_screen_before_the_menu() {
        // The legacy login sequence renders the user-stats screen
        // (`internalCommandS()` layout) between the mail scan and the
        // menu (`amiexpress/express.e` logon path). NextExpress shows
        // the same six-row block at login, without the user typing `S`.
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let services = services_with(repo, conferences);
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        let stats_pos = output
            .windows(b"Security Lv".len())
            .position(|w| w == b"Security Lv")
            .expect("user-stats screen should render at login");
        let menu_pos = output
            .windows(b"MENU".len())
            .position(|w| w == b"MENU")
            .expect("menu should render");
        assert!(
            stats_pos < menu_pos,
            "the user-stats screen must precede the menu at login"
        );
        // The stats read *after* `enter_menu`, so `times_called` shows the
        // logon bump (alice starts at 0 -> 1), matching the legacy
        // `statPrintUser` order. A regression rendering the stats from the
        // pre-`enter_menu` `OnboardedSession` would show `0` here.
        let times_on = b"# Times On \x1b[33m:\x1b[0m 1";
        assert!(
            output.windows(times_on.len()).any(|w| w == times_on),
            "login stats must show the post-enter_menu times_called (1), got {:?}",
            String::from_utf8_lossy(output)
        );
    }

    /// Builds the services + alice/Main fixture shared by the
    /// graphics-prompt tests.
    fn graphics_test_services() -> AppServices {
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        services_with(repo, conferences)
    }

    #[tokio::test]
    async fn login_asks_for_graphics_before_the_name_prompt() {
        // AmiExpress asks `ANSI, RIP or No graphics (A/r/n)?` at connect,
        // before the name prompt (`amiexpress/express.e:29528`).
        // NextExpress asks the RIP-less `ANSI Graphics (Y/n)? ` in the
        // same slot.
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver =
            SessionDriver::new(terminal, 1, LogonChannel::Remote, graphics_test_services());

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        // Answering `Y` keeps ANSI on (only `n`/`N` disables it).
        assert!(
            terminal.ansi_colour(),
            "answering Y at the graphics prompt must keep colour on"
        );
        let output = terminal.output().to_vec();
        let graphics_pos = output
            .windows(b"ANSI Graphics (Y/n)? ".len())
            .position(|w| w == b"ANSI Graphics (Y/n)? ")
            .expect("graphics prompt should be asked at login");
        let name_pos = output
            .windows(b"Enter your Name: ".len())
            .position(|w| w == b"Enter your Name: ")
            .expect("name prompt should follow");
        assert!(
            graphics_pos < name_pos,
            "the graphics prompt must precede the name prompt"
        );
    }

    #[tokio::test]
    async fn login_asks_for_graphics_before_the_banner() {
        // The legacy renders the BBS title screen (`SCREEN_BBSTITLE`,
        // `amiexpress/express.e:29552`) only *after* the graphics
        // question (`:29528`), so an ASCII caller's title art is
        // ANSI-stripped. NextExpress likewise asks the question before
        // the banner. (The plain copyright preamble stays before it.)
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver =
            SessionDriver::new(terminal, 1, LogonChannel::Remote, graphics_test_services());

        driver.run().await.expect("driver completes");

        let output = driver.into_terminal().output().to_vec();
        let graphics_pos = output
            .windows(b"ANSI Graphics (Y/n)? ".len())
            .position(|w| w == b"ANSI Graphics (Y/n)? ")
            .expect("graphics prompt should be asked");
        let banner_pos = output
            .windows(b"BANNER".len())
            .position(|w| w == b"BANNER")
            .expect("banner should render");
        assert!(
            graphics_pos < banner_pos,
            "the graphics question must precede the banner/title screen"
        );
    }

    #[tokio::test]
    async fn choosing_ascii_at_login_disables_ansi_colour() {
        // Answering with `n` (no graphics) sets the terminal's live
        // colour mode off, so the ColourTerminal decorator strips ANSI
        // SGR from every subsequent screen (the legacy `ansiColour`
        // flag, `amiexpress/express.e:29543`).
        let terminal = FakeTerminal::new([
            TerminalRead::Line("n".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver =
            SessionDriver::new(terminal, 1, LogonChannel::Remote, graphics_test_services());

        driver.run().await.expect("driver completes");

        assert!(
            !driver.into_terminal().ansi_colour(),
            "choosing ASCII at login must turn the terminal's colour mode off"
        );
    }

    #[tokio::test]
    async fn logon_conference_scan_runs_before_the_auto_rejoin_announcement() {
        // The legacy runs `confScan` (the multi-conference logon mail
        // scan) at `SUBSTATE_DISPLAY_BULL` (`amiexpress/express.e:28564`),
        // before the auto-rejoin join at `SUBSTATE_DISPLAY_CONF_BULL`
        // (`:28574`). NextExpress emits the `Scanning conferences for
        // mail...` header before the `Auto-ReJoined` line.
        let terminal = FakeTerminal::new([
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver =
            SessionDriver::new(terminal, 1, LogonChannel::Remote, graphics_test_services());

        driver.run().await.expect("driver completes");

        let output = driver.into_terminal().output().to_vec();
        let scan_pos = output
            .windows(b"Scanning conferences for mail".len())
            .position(|w| w == b"Scanning conferences for mail")
            .expect("the logon conference scan header should render");
        let rejoin_pos = output
            .windows(b"Auto-ReJoined".len())
            .position(|w| w == b"Auto-ReJoined")
            .expect("the auto-rejoin announcement should render");
        assert!(
            scan_pos < rejoin_pos,
            "the logon conference scan must precede the auto-rejoin announcement"
        );
    }

    #[tokio::test]
    async fn registration_handle_prompt_rejects_new_literal() {
        // After moving the NEW literal out of UserRepository the
        // registration handle prompt has to explicitly reject the
        // command word; otherwise a user could register themselves
        // under the same name the login flow uses to trigger
        // registration in the first place.
        let repo = Arc::new(InMemoryUserRepository::new(vec![]));
        let services = services_with(repo, vec![]);
        let terminal = FakeTerminal::new([
            // Answer the graphics prompt, then drive registration.
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("NEW".to_string()),
            // First registration handle attempt — should be rejected.
            TerminalRead::Line("NEW".to_string()),
            // EOF ends the run; the test only cares that we
            // see HANDLE_TAKEN_LINE after the NEW attempt.
            TerminalRead::Eof,
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        let taken = b"That name is taken.";
        assert!(
            output.windows(taken.len()).any(|w| w == taken),
            "expected handle-taken line to appear after typing NEW at the registration prompt",
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
        let copyright = std::str::from_utf8(super::COPYRIGHT_LINES).expect("utf8 copyright");
        let needle = format!("NextExpress ({sha}) Copyright");
        assert!(
            copyright.contains(&needle),
            "expected `{needle}` in copyright lines: {copyright:?}",
        );
    }
}
