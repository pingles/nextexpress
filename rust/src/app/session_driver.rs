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
use crate::app::wire_text::{render_stats_screen, COPYRIGHT_LINES, NO_CONFERENCE_ACCESS_LINE};
use crate::domain::conference::NameType;
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
                let transition = onboarded
                    .auto_rejoin_conference(self.services.conferences.as_ref(), SystemTime::now());
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
                        let mut menu = self.enter_menu(session);
                        MenuFlow::new(&mut self.terminal, &self.services)
                            .run_logon_conference_scan(&mut menu)
                            .await?;
                        // Replay the deferred auto-rejoin announcement, then
                        // the login stats (read after the `enter_menu` bump,
                        // matching the legacy `statPrintUser` order).
                        self.announce_auto_rejoin(&announcement).await?;
                        self.render_login_stats(&menu).await?;
                        MenuFlow::new(&mut self.terminal, &self.services)
                            .run(menu)
                            .await?
                    }
                    // The no-access line tells the user why their session
                    // is closing — the caller-log finalise entry already
                    // records `LogoffReason::NoConferenceAccess`.
                    AutoRejoinTransition::NoAccess(logging_off) => {
                        self.terminal.write(NO_CONFERENCE_ACCESS_LINE).await?;
                        self.terminal.flush().await?;
                        logging_off
                    }
                }
            }
            SignInResult::LoggingOff(logging_off) => logging_off,
            SignInResult::Ended(_ended) => return Ok(()),
        };

        self.finalise(logging_off);
        Ok(())
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
    ) -> Result<IdentifyingSession, T::Error> {
        // Plain connection preamble (the legacy "Running AmiExpress..."
        // lines, `amiexpress/express.e:29514`), shown before the
        // graphics question. The banner / title screen is rendered
        // afterwards by `LoginFlow` so an ASCII caller gets it
        // ANSI-stripped, mirroring the legacy `SCREEN_BBSTITLE` order
        // (`:29552`, after the `A/r/n` question).
        self.terminal.write(COPYRIGHT_LINES).await?;
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

    fn enter_menu(&mut self, onboarded: OnboardedSession) -> MenuSession {
        session_flow::enter_menu(
            onboarded,
            self.services.user_repo.as_ref(),
            self.services.caller_log.as_ref(),
            SystemTime::now(),
        )
        .expect("OnboardedSession with no force_password_reset enters menu cleanly")
    }

    fn finalise(&mut self, logging_off: LoggingOffSession) -> EndedSession {
        session_flow::finalise_logoff(
            logging_off,
            self.services.user_repo.as_ref(),
            self.services.caller_log.as_ref(),
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
        ansi_colour: bool,
    }

    impl FakeTerminal {
        fn new(inputs: impl IntoIterator<Item = TerminalRead>) -> Self {
            Self {
                inputs: inputs.into_iter().collect(),
                output: Vec::new(),
                echo_modes: Vec::new(),
                ansi_colour: true,
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
            caller_log,
            screens,
            conferences: Arc::new(conferences),
            mail_stores: test_mail_stores(),
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
        AppServices {
            user_repo: repo,
            hasher,
            caller_log,
            screens,
            conferences: Arc::new(conferences),
            mail_stores: test_mail_stores(),
            session_policy: SessionPolicy::default(),
            default_ratio: ratio,
            new_user_gate: Arc::new(gate),
            bbs_name: Arc::from("TestBBS"),
        }
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
            caller_log,
            screens,
            conferences: Arc::new(vec![]),
            mail_stores: test_mail_stores(),
            session_policy: SessionPolicy::default(),
            default_ratio: ratio,
            new_user_gate: Arc::new(gate),
            bbs_name: Arc::from("TestBBS"),
        };
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
}
