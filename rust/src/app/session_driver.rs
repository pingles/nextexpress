//! Transport-agnostic application driver for an interactive BBS session.
//!
//! Driving adapters provide a [`Terminal`] implementation (see
//! [`crate::app::terminal`]). The driver owns the top-level workflow:
//! it renders the banner, hands off to the
//! [`crate::app::login_flow::LoginFlow`] for sign-in, the
//! [`crate::app::registration_flow::RegistrationFlow`] for new users,
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

use std::time::SystemTime;

use crate::app::login_flow::{LoginFlow, LoginOutcome};
use crate::app::menu_flow::MenuFlow;
use crate::app::registration_flow::{RegistrationFlow, RegistrationOutcome};
use crate::app::services::AppServices;
use crate::app::session_flow;
use crate::app::session_presenter::{format_auto_rejoin_line, render_name_type_promotion};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{COPYRIGHT_LINES, NO_CONFERENCE_ACCESS_LINE};
use crate::domain::session::typed::{
    AutoRejoinTransition, ConnectingSession, EndedSession, IdentifyingSession, LoggingOffSession,
    MenuSession, OnboardedSession,
};
use crate::domain::session::LogonChannel;

/// App-layer session workflow over a terminal port.
///
/// The driver does not hold a [`crate::domain::session::Session`]
/// field; phase wrappers are stack-local as they thread through
/// [`Self::run`] and the sub-flow structs in
/// [`crate::app::login_flow`], [`crate::app::registration_flow`] and
/// [`crate::app::menu_flow`].
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

/// Outcome of [`SessionDriver::auto_rejoin`]. Mirrors the spec's
/// `JoinConference` two-branch consequent (resolved vs.
/// `no_conference_access`).
enum AutoRejoinResult {
    /// The session attached to a conference and may proceed into the
    /// menu loop.
    Joined(OnboardedSession),
    /// The user has no granted membership; the session has moved to
    /// `LoggingOff` with `LogoffReason::NoConferenceAccess`.
    NoAccess(LoggingOffSession),
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

    /// Runs the BBS workflow until the terminal closes or the session
    /// reaches a final logoff path.
    pub(crate) async fn run(&mut self) -> Result<(), T::Error> {
        let connecting =
            ConnectingSession::accept(self.node_number, self.channel, 0, SystemTime::now())
                .expect("freshly allocated node has no existing session");
        let identifying = self.start(connecting).await?;

        let login = LoginFlow::new(&mut self.terminal, &self.services)
            .identify(identifying)
            .await?;
        let signed_in = match login {
            LoginOutcome::Onboarded(onboarded) => SignInResult::Onboarded(onboarded),
            LoginOutcome::LoggingOff(logging_off) => SignInResult::LoggingOff(logging_off),
            LoginOutcome::Ended(ended) => SignInResult::Ended(ended),
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
            SignInResult::Onboarded(onboarded) => match self.auto_rejoin(onboarded).await? {
                AutoRejoinResult::Joined(onboarded) => {
                    let menu = self.enter_menu(onboarded);
                    MenuFlow::new(&mut self.terminal, &self.services)
                        .run(menu)
                        .await?
                }
                AutoRejoinResult::NoAccess(logging_off) => logging_off,
            },
            SignInResult::LoggingOff(logging_off) => logging_off,
            SignInResult::Ended(_ended) => return Ok(()),
        };

        self.finalise(logging_off);
        Ok(())
    }

    /// Resolves `conferences.allium:JoinConference` for the
    /// auto-rejoin path (Slice 30) and renders the JOINED screen and
    /// any name-type promotion screen (Slices 31 / 34). On
    /// `NoAccess` the listener writes the no-access line so the user
    /// understands why their session is closing — the
    /// caller-log finalise entry will already record the underlying
    /// `LogoffReason::NoConferenceAccess`.
    async fn auto_rejoin(
        &mut self,
        onboarded: OnboardedSession,
    ) -> Result<AutoRejoinResult, T::Error> {
        let conferences = self.services.conferences();
        match onboarded.auto_rejoin_conference(conferences, SystemTime::now()) {
            AutoRejoinTransition::Joined {
                mut session,
                conference_number,
                msgbase_number,
                show_bulletin: _,
                name_type_promoted_to,
            } => {
                let line = format_auto_rejoin_line(conferences, conference_number, msgbase_number);
                self.terminal.write(&line).await?;
                self.terminal.flush().await?;
                render_name_type_promotion(
                    &mut self.terminal,
                    self.services.screens(),
                    name_type_promoted_to,
                )
                .await?;
                // Slice 41: fire `conferences.allium:ScanMailOnJoin`
                // in `follow_pointer` mode for the auto-rejoin path.
                crate::app::mail_scan_on_join::scan_mail_on_join(
                    &mut self.terminal,
                    &self.services,
                    &mut session,
                    crate::app::mail_scan_on_join::JoinScanMode::FollowPointer,
                )
                .await?;
                Ok(AutoRejoinResult::Joined(session))
            }
            AutoRejoinTransition::NoAccess(logging_off) => {
                self.terminal.write(NO_CONFERENCE_ACCESS_LINE).await?;
                self.terminal.flush().await?;
                Ok(AutoRejoinResult::NoAccess(logging_off))
            }
        }
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
    ) -> Result<IdentifyingSession, T::Error> {
        let banner = self.services.screens().banner().await;
        self.terminal.write(&banner).await?;
        self.terminal.write(COPYRIGHT_LINES).await?;
        Ok(connecting.prompt_for_name())
    }

    fn enter_menu(&mut self, onboarded: OnboardedSession) -> MenuSession {
        session_flow::typed::enter_menu(
            onboarded,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("OnboardedSession with no force_password_reset enters menu cleanly")
    }

    fn finalise(&mut self, logging_off: LoggingOffSession) -> EndedSession {
        session_flow::typed::finalise_logoff(
            logging_off,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("LoggingOffSession finalises cleanly when persistence succeeds")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::mail_stores::MailStores;
    use crate::app::screens::{ScreenFuture, ScreenRepository};
    use crate::app::services::{AppServices, SharedMailStores};
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::{LogonChannel, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

    fn test_mail_stores() -> SharedMailStores {
        Arc::new(InMemoryMailStores::new()) as Arc<dyn MailStores + Send + Sync>
    }

    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};

    use super::SessionDriver;

    struct FakeTerminal {
        inputs: VecDeque<TerminalRead>,
        output: Vec<u8>,
        echo_modes: Vec<TerminalEcho>,
    }

    impl FakeTerminal {
        fn new(inputs: impl IntoIterator<Item = TerminalRead>) -> Self {
            Self {
                inputs: inputs.into_iter().collect(),
                output: Vec::new(),
                echo_modes: Vec::new(),
            }
        }

        fn output(&self) -> &[u8] {
            &self.output
        }
    }

    impl Terminal for FakeTerminal {
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
            echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async move {
                self.echo_modes.push(echo);
                Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof))
            })
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
        let services = AppServices::new(
            repo,
            hasher,
            caller_log.clone(),
            screens,
            Arc::new(conferences),
            test_mail_stores(),
            SessionPolicy::default(),
            ratio,
            gate,
            "TestBBS".to_string(),
        );
        let terminal = FakeTerminal::new([
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
    async fn registration_handle_prompt_rejects_new_literal() {
        // After moving the NEW literal out of UserRepository the
        // registration handle prompt has to explicitly reject the
        // command word; otherwise a user could register themselves
        // under the same name the login flow uses to trigger
        // registration in the first place.
        let repo = Arc::new(InMemoryUserRepository::new(vec![]));
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
        let services = AppServices::new(
            repo,
            hasher,
            caller_log,
            screens,
            Arc::new(vec![]),
            test_mail_stores(),
            SessionPolicy::default(),
            ratio,
            gate,
            "TestBBS".to_string(),
        );
        let terminal = FakeTerminal::new([
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
}
