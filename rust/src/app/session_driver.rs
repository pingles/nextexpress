//! Transport-agnostic application driver for an interactive BBS session.
//!
//! Driving adapters provide a [`Terminal`] implementation (see
//! [`crate::app::terminal`]). The driver owns the BBS workflow:
//! accepting the session, prompting for login, optional new-user
//! registration, authentication, menu entry and logoff finalisation.
//!
//! Wire-format byte constants live in [`crate::app::wire_text`].

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow::{self, NewUserProfile, NewUserRegistrationFlow};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    ACCOUNT_LOCKED_LINE, ANSI_PROMPT, AUTHENTICATED_LINE, COPYRIGHT_LINES, EMAIL_PROMPT,
    GOODBYE_LINE, HANDLE_TAKEN_LINE, IDLE_TIMEOUT_LINE, INVALID_LINE_LENGTH_LINE,
    LINE_LENGTH_PROMPT, LOCATION_PROMPT, LOGON_REJECTED_LINE, MENU_PROMPT, NAME_PROMPT,
    NEW_USER_EXCESSIVE_FAILURES_LINE, NEW_USER_INVALID_PASSWORD_LINE, NEW_USER_PASSWORD_OK_LINE,
    NEW_USER_PASSWORD_PROMPT, PASSWORDS_DO_NOT_MATCH_LINE, PASSWORD_PROMPT, PHONE_PROMPT,
    REGISTRATION_COMPLETE_LINE, REGISTRATION_HANDLE_PROMPT, REGISTRATION_PASSWORD_CONFIRM_PROMPT,
    REGISTRATION_PASSWORD_PROMPT, REGISTRATION_RETRIES_EXHAUSTED_LINE,
    TOO_MANY_PASSWORD_FAILURES_LINE, TOO_MANY_RETRIES_LINE, UNKNOWN_COMMAND_LINE,
    UNKNOWN_USER_LINE, WRONG_PASSWORD_LINE,
};
use crate::domain::session::{
    LogonChannel, NameTypedOutcome, NewUserPasswordOutcome, Session, SessionState,
    VerifyPasswordOutcome,
};
use crate::domain::user_repository::NameLookupResult;

/// Maximum handle attempts during registration before the session
/// bails. Mirrors the original `AmiExpress` `doNewUser` retry budget at
/// `amiexpress/express.e:30150`.
const MAX_REGISTRATION_HANDLE_ATTEMPTS: u32 = 5;

/// App-layer session workflow over a terminal port.
pub(crate) struct SessionDriver<T>
where
    T: Terminal,
{
    terminal: T,
    session: Session,
    services: AppServices,
}

impl<T> SessionDriver<T>
where
    T: Terminal,
{
    /// Constructs a driver for a newly accepted connection.
    #[must_use]
    pub(crate) fn new(
        terminal: T,
        node_number: u32,
        channel: LogonChannel,
        services: AppServices,
    ) -> Self {
        let session = Session::accept_connection(node_number, channel, 0, SystemTime::now(), None)
            .expect("freshly allocated node has no existing session");

        Self {
            terminal,
            session,
            services,
        }
    }

    /// Runs the BBS workflow until the terminal closes or the session
    /// reaches a final logoff path.
    pub(crate) async fn run(&mut self) -> Result<(), T::Error> {
        self.start().await?;
        if !self.identify().await? {
            return Ok(());
        }
        if self.session.state() == SessionState::Authenticating && !self.authenticate().await? {
            return Ok(());
        }
        self.enter_menu();
        self.run_menu().await
    }

    /// Returns the terminal after the driver has finished. Intended
    /// for tests and adapter-owned cleanup.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn into_terminal(self) -> T {
        self.terminal
    }

    async fn start(&mut self) -> Result<(), T::Error> {
        let banner = self.services.screens().banner().await;
        self.terminal.write(&banner).await?;
        self.terminal.write(COPYRIGHT_LINES).await?;
        self.session
            .prompt_for_name()
            .expect("connecting -> identifying");
        Ok(())
    }

    async fn identify(&mut self) -> Result<bool, T::Error> {
        loop {
            let Some(typed) = self
                .prompt_for_line(NAME_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(false);
            };
            let outcome = session_flow::name_typed(
                &mut self.session,
                typed.trim(),
                self.services.user_repo(),
                self.services.new_user_gate(),
                SystemTime::now(),
            )
            .expect("session is in identifying");
            match outcome {
                NameTypedOutcome::Authenticated => return Ok(true),
                NameTypedOutcome::NotFound => {
                    self.terminal.write(UNKNOWN_USER_LINE).await?;
                }
                NameTypedOutcome::NewUserRegistering { password_required } => {
                    return self.run_new_user_registration(password_required).await;
                }
                NameTypedOutcome::NewUserRegistrationDisallowed => {
                    self.reject_disallowed_registration().await?;
                    return Ok(false);
                }
                NameTypedOutcome::SessionEnded => {
                    self.terminal.write(TOO_MANY_RETRIES_LINE).await?;
                    self.terminal.flush().await?;
                    return Ok(false);
                }
            }
        }
    }

    async fn reject_disallowed_registration(&mut self) -> Result<(), T::Error> {
        let screen = self.services.screens().no_new_users().await;
        self.terminal.write(&screen).await?;
        self.terminal.flush().await?;
        self.finalise_logoff();
        Ok(())
    }

    async fn authenticate(&mut self) -> Result<bool, T::Error> {
        loop {
            let Some(password) = self
                .prompt_for_line(PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?
            else {
                return Ok(false);
            };
            let outcome = session_flow::verify_password(
                &mut self.session,
                password.trim(),
                self.services.user_repo(),
                self.services.hasher(),
                self.services.caller_log(),
                self.services.session_policy(),
                SystemTime::now(),
            )
            .expect("session is in authenticating with a user");
            match outcome {
                VerifyPasswordOutcome::Authenticated => {
                    self.terminal.write(AUTHENTICATED_LINE).await?;
                    self.terminal.flush().await?;
                    return Ok(true);
                }
                VerifyPasswordOutcome::NotMatching => {
                    self.terminal.write(WRONG_PASSWORD_LINE).await?;
                    self.terminal.flush().await?;
                }
                VerifyPasswordOutcome::AccountLocked => {
                    self.write_and_flush(ACCOUNT_LOCKED_LINE).await?;
                    return Ok(false);
                }
                VerifyPasswordOutcome::TooManyFailures => {
                    self.write_and_flush(TOO_MANY_PASSWORD_FAILURES_LINE)
                        .await?;
                    return Ok(false);
                }
                VerifyPasswordOutcome::LogonRejected => {
                    self.write_and_flush(LOGON_REJECTED_LINE).await?;
                    return Ok(false);
                }
            }
        }
    }

    fn enter_menu(&mut self) {
        session_flow::enter_menu(
            &mut self.session,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("session is in onboarded with a user");
    }

    async fn run_menu(&mut self) -> Result<(), T::Error> {
        loop {
            let access_level = self
                .session
                .user()
                .expect("session is in menu with a user")
                .access_level();
            let menu = self.services.screens().default_menu(access_level).await;
            self.terminal.write(&menu).await?;
            let Some(line) = self
                .prompt_for_line(MENU_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(());
            };
            if line.trim().eq_ignore_ascii_case("G") {
                self.session
                    .user_requests_logoff()
                    .expect("session is in menu");
                self.finalise_logoff();
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(());
            }
            self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
        }
    }

    async fn prompt_for_line(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<Option<String>, T::Error> {
        self.terminal.write(prompt).await?;
        self.terminal.flush().await?;
        match self.read_line(echo).await? {
            TerminalRead::Line(line) => Ok(Some(line)),
            TerminalRead::Eof => {
                self.handle_carrier_loss();
                Ok(None)
            }
            TerminalRead::IdleTimedOut => {
                self.handle_idle_timeout().await?;
                Ok(None)
            }
        }
    }

    async fn read_line(&mut self, echo: TerminalEcho) -> Result<TerminalRead, T::Error> {
        let timeout = self.services.session_policy().input_timeout();
        let outcome = self.terminal.read_line(echo, timeout).await?;
        if matches!(outcome, TerminalRead::Line(_)) {
            self.session.record_input(SystemTime::now());
        }
        Ok(outcome)
    }

    async fn handle_idle_timeout(&mut self) -> Result<(), T::Error> {
        self.session
            .apply_idle_timeout(self.services.session_policy().treat_timeout_as_logoff())
            .expect("idle-permitted state when read times out");
        self.finalise_logoff();
        self.write_and_flush(IDLE_TIMEOUT_LINE).await
    }

    fn handle_carrier_loss(&mut self) {
        self.session
            .apply_carrier_loss()
            .expect("carrier-permitted state when peer closes");
        self.finalise_logoff();
    }

    fn finalise_logoff(&mut self) {
        session_flow::finalise_logoff(
            &mut self.session,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("logging_off can finalise");
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        self.terminal.write(bytes).await?;
        self.terminal.flush().await
    }

    async fn run_new_user_registration(
        &mut self,
        password_required: bool,
    ) -> Result<bool, T::Error> {
        let screen = self.services.screens().new_user_password().await;
        self.terminal.write(&screen).await?;
        if password_required && !self.run_new_user_password_gate().await? {
            return Ok(false);
        }

        let Some(handle) = self.read_registration_handle().await? else {
            return Ok(false);
        };
        let Some(location) = self.read_optional_field(LOCATION_PROMPT).await? else {
            return Ok(false);
        };
        let Some(phone_number) = self.read_optional_field(PHONE_PROMPT).await? else {
            return Ok(false);
        };
        let Some(email) = self.read_optional_field(EMAIL_PROMPT).await? else {
            return Ok(false);
        };
        let Some(password) = self.read_registration_password().await? else {
            return Ok(false);
        };
        let Some(line_length) = self.read_line_length().await? else {
            return Ok(false);
        };
        let Some(ansi_colour) = self.read_ansi_colour().await? else {
            return Ok(false);
        };

        let profile = NewUserProfile {
            handle,
            location,
            phone_number,
            email,
            password,
            line_length,
            ansi_colour,
            flags: BTreeSet::new(),
        };
        self.complete_new_user_registration(profile).await
    }

    async fn run_new_user_password_gate(&mut self) -> Result<bool, T::Error> {
        loop {
            let Some(typed) = self
                .prompt_for_line(NEW_USER_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?
            else {
                return Ok(false);
            };
            let outcome = session_flow::verify_new_user_password(
                &mut self.session,
                typed.trim(),
                self.services.new_user_gate(),
                self.services.caller_log(),
                SystemTime::now(),
            )
            .expect("session is in new_user_registering and gate is configured");
            match outcome {
                NewUserPasswordOutcome::Verified => {
                    self.write_and_flush(NEW_USER_PASSWORD_OK_LINE).await?;
                    return Ok(true);
                }
                NewUserPasswordOutcome::Mismatch => {
                    self.write_and_flush(NEW_USER_INVALID_PASSWORD_LINE).await?;
                }
                NewUserPasswordOutcome::TooManyFailures => {
                    self.write_and_flush(NEW_USER_EXCESSIVE_FAILURES_LINE)
                        .await?;
                    self.finalise_logoff();
                    return Ok(false);
                }
            }
        }
    }

    async fn read_registration_handle(&mut self) -> Result<Option<String>, T::Error> {
        let mut attempts: u32 = 0;
        loop {
            if attempts >= MAX_REGISTRATION_HANDLE_ATTEMPTS {
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                self.handle_carrier_loss();
                return Ok(None);
            }
            let Some(typed) = self
                .prompt_for_line(REGISTRATION_HANDLE_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(None);
            };
            let trimmed = typed.trim();
            let available = !trimmed.is_empty()
                && matches!(
                    self.services.user_repo().find_by_handle(trimmed),
                    NameLookupResult::NotFound
                );
            if available {
                return Ok(Some(trimmed.to_string()));
            }
            self.terminal.write(HANDLE_TAKEN_LINE).await?;
            attempts += 1;
        }
    }

    async fn read_optional_field(
        &mut self,
        prompt: &[u8],
    ) -> Result<Option<Option<String>>, T::Error> {
        let Some(typed) = self.prompt_for_line(prompt, TerminalEcho::Visible).await? else {
            return Ok(None);
        };
        let trimmed = typed.trim();
        Ok(Some(if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }))
    }

    async fn read_registration_password(&mut self) -> Result<Option<String>, T::Error> {
        loop {
            let Some(password) = self
                .prompt_for_line(REGISTRATION_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?
            else {
                return Ok(None);
            };
            if password.trim().is_empty() {
                continue;
            }
            let Some(confirmed) = self
                .prompt_for_line(REGISTRATION_PASSWORD_CONFIRM_PROMPT, TerminalEcho::Masked)
                .await?
            else {
                return Ok(None);
            };
            if password == confirmed {
                return Ok(Some(password));
            }
            self.terminal.write(PASSWORDS_DO_NOT_MATCH_LINE).await?;
        }
    }

    async fn read_line_length(&mut self) -> Result<Option<u32>, T::Error> {
        loop {
            let Some(typed) = self
                .prompt_for_line(LINE_LENGTH_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(None);
            };
            let trimmed = typed.trim();
            if trimmed.is_empty() {
                return Ok(Some(0));
            }
            match trimmed.parse::<u32>() {
                Ok(value) if value <= 255 => return Ok(Some(value)),
                _ => {
                    self.terminal.write(INVALID_LINE_LENGTH_LINE).await?;
                }
            }
        }
    }

    async fn read_ansi_colour(&mut self) -> Result<Option<bool>, T::Error> {
        let Some(ansi_typed) = self
            .prompt_for_line(ANSI_PROMPT, TerminalEcho::Visible)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(!ansi_typed.trim().eq_ignore_ascii_case("N")))
    }

    async fn complete_new_user_registration(
        &mut self,
        profile: NewUserProfile,
    ) -> Result<bool, T::Error> {
        let flow = NewUserRegistrationFlow::new(
            self.services.user_repo(),
            self.services.hasher(),
            self.services.caller_log(),
            self.services.default_ratio(),
            self.services.session_policy(),
        );
        if flow
            .complete(&mut self.session, profile, SystemTime::now())
            .is_err()
        {
            self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                .await?;
            self.handle_carrier_loss();
            return Ok(false);
        }
        self.write_and_flush(REGISTRATION_COMPLETE_LINE).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::screens::{ScreenFuture, ScreenRepository};
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::{LogonChannel, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

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

        fn new_user_password(&self) -> ScreenFuture<'_> {
            bytes(b"NEW USER\r\n")
        }

        fn no_new_users(&self) -> ScreenFuture<'_> {
            bytes(b"NO NEW USERS\r\n")
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
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice_with_password(
            "secret",
        )]));
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
            SessionPolicy::default(),
            ratio,
            gate,
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
        assert!(output.windows(b"Goodbye".len()).any(|w| w == b"Goodbye"));
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
}
