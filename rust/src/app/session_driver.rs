//! Transport-agnostic application driver for an interactive BBS session.
//!
//! Driving adapters provide a [`Terminal`] implementation (see
//! [`crate::app::terminal`]). The driver owns the BBS workflow:
//! accepting the session, prompting for login, optional new-user
//! registration, authentication, menu entry and logoff finalisation.
//!
//! Wire-format byte constants live in [`crate::app::wire_text`].
//!
//! ## Phase types
//! Each step of the workflow consumes and returns a phase wrapper from
//! [`crate::app::typed_session`]. The wrong handle for a given
//! transition becomes unrepresentable at compile time; the driver no
//! longer needs to assert "session is in X" after every call.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow::{self, NewUserProfile, NewUserRegistrationFlow};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::typed_session::{
    AuthenticatingSession, ConnectingSession, EndedSession, IdentifyingSession, LoggingOffSession,
    MenuSession, NameTypedTransition, NewUserPasswordTransition, NewUserRegisteringSession,
    NewUserRegistrationResult, OnboardedSession, VerifyPasswordRejectionReason,
    VerifyPasswordTransition,
};
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
use crate::domain::session::LogonChannel;
use crate::domain::user_repository::NameLookupResult;

/// Maximum handle attempts during registration before the session
/// bails. Mirrors the original `AmiExpress` `doNewUser` retry budget at
/// `amiexpress/express.e:30150`.
const MAX_REGISTRATION_HANDLE_ATTEMPTS: u32 = 5;

/// App-layer session workflow over a terminal port.
///
/// The driver does not hold a [`crate::domain::session::Session`]
/// field; phase wrappers are stack-local as they thread through
/// [`Self::run`].
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
/// Lets [`SessionDriver::run`] decide how to enter the menu vs. how to
/// finalise.
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

/// Outcome of the password-verification loop. Lets the caller proceed
/// to the menu or skip straight to logoff finalisation.
enum AuthResult {
    /// Credentials matched and the post-auth cluster ran clean.
    Onboarded(OnboardedSession),
    /// Logon rejected (lockout, retries, post-auth gate). The driver
    /// has already written the wire message.
    LoggingOff(LoggingOffSession),
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
        let signed_in = self.identify(identifying).await?;

        let logging_off = match signed_in {
            SignInResult::Onboarded(onboarded) => {
                let menu = self.enter_menu(onboarded);
                self.run_menu(menu).await?
            }
            SignInResult::LoggingOff(logging_off) => logging_off,
            SignInResult::Ended(_ended) => return Ok(()),
        };

        self.finalise(logging_off);
        Ok(())
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

    async fn identify(
        &mut self,
        mut session: IdentifyingSession,
    ) -> Result<SignInResult, T::Error> {
        loop {
            let read = self
                .read_prompted(NAME_PROMPT, TerminalEcho::Visible)
                .await?;
            let line = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(SignInResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(SignInResult::LoggingOff(logoff));
                }
            };
            let trimmed = line.trim();
            let transition = session_flow::typed::name_typed(
                session,
                trimmed,
                self.services.user_repo(),
                self.services.new_user_gate(),
                SystemTime::now(),
            );
            match transition {
                NameTypedTransition::Authenticated(authenticating) => {
                    return self
                        .authenticate(authenticating)
                        .await
                        .map(|auth| match auth {
                            AuthResult::Onboarded(s) => SignInResult::Onboarded(s),
                            AuthResult::LoggingOff(s) => SignInResult::LoggingOff(s),
                        });
                }
                NameTypedTransition::Identifying(retry) => {
                    self.terminal.write(UNKNOWN_USER_LINE).await?;
                    session = retry;
                }
                NameTypedTransition::NewUserRegistering {
                    session: registering,
                    password_required,
                } => {
                    return self
                        .run_new_user_registration(registering, password_required)
                        .await;
                }
                NameTypedTransition::Disallowed(logging_off) => {
                    let screen = self.services.screens().no_new_users().await;
                    self.terminal.write(&screen).await?;
                    self.terminal.flush().await?;
                    return Ok(SignInResult::LoggingOff(logging_off));
                }
                NameTypedTransition::Ended(ended) => {
                    self.terminal.write(TOO_MANY_RETRIES_LINE).await?;
                    self.terminal.flush().await?;
                    return Ok(SignInResult::Ended(ended));
                }
            }
        }
    }

    async fn authenticate(
        &mut self,
        mut session: AuthenticatingSession,
    ) -> Result<AuthResult, T::Error> {
        loop {
            let read = self
                .read_prompted(PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let password = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(AuthResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(AuthResult::LoggingOff(logoff));
                }
            };
            let transition = session_flow::typed::verify_password(
                session,
                password.trim(),
                self.services.user_repo(),
                self.services.hasher(),
                self.services.caller_log(),
                self.services.session_policy(),
                SystemTime::now(),
            )
            .expect("AuthenticatingSession guarantees Authenticating + bound user");
            match transition {
                VerifyPasswordTransition::Onboarded(onboarded) => {
                    self.write_and_flush(AUTHENTICATED_LINE).await?;
                    return Ok(AuthResult::Onboarded(onboarded));
                }
                VerifyPasswordTransition::Authenticating(retry) => {
                    self.write_and_flush(WRONG_PASSWORD_LINE).await?;
                    session = retry;
                }
                VerifyPasswordTransition::LoggingOff {
                    session: logging_off,
                    reason,
                } => {
                    let line: &[u8] = match reason {
                        VerifyPasswordRejectionReason::AccountLocked => ACCOUNT_LOCKED_LINE,
                        VerifyPasswordRejectionReason::TooManyFailures => {
                            TOO_MANY_PASSWORD_FAILURES_LINE
                        }
                        VerifyPasswordRejectionReason::LogonRejected => LOGON_REJECTED_LINE,
                    };
                    self.write_and_flush(line).await?;
                    return Ok(AuthResult::LoggingOff(logging_off));
                }
            }
        }
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

    async fn run_menu(&mut self, mut session: MenuSession) -> Result<LoggingOffSession, T::Error> {
        loop {
            let access_level = session.user().access_level();
            let menu = self.services.screens().default_menu(access_level).await;
            self.terminal.write(&menu).await?;
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
            if line.trim().eq_ignore_ascii_case("G") {
                let logging_off = session.user_requests_logoff();
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(logging_off);
            }
            self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
        }
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

    async fn run_new_user_registration(
        &mut self,
        session: NewUserRegisteringSession,
        password_required: bool,
    ) -> Result<SignInResult, T::Error> {
        let screen = self.services.screens().new_user_password().await;
        self.terminal.write(&screen).await?;
        let session = if password_required {
            match self.run_new_user_password_gate(session).await? {
                GateResult::Verified(s) => s,
                GateResult::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
            }
        } else {
            session
        };

        let (session, handle) = match self.read_registration_handle(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, location) = match self.read_optional_field(session, LOCATION_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, phone_number) = match self.read_optional_field(session, PHONE_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, email) = match self.read_optional_field(session, EMAIL_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, password) = match self.read_registration_password(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, line_length) = match self.read_line_length(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, ansi_colour) = match self.read_ansi_colour(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
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
        self.complete_new_user_registration(session, profile).await
    }

    async fn run_new_user_password_gate(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<GateResult, T::Error> {
        loop {
            let read = self
                .read_prompted(NEW_USER_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(GateResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(GateResult::LoggingOff(logoff));
                }
            };
            let transition = session_flow::typed::verify_new_user_password(
                session,
                typed.trim(),
                self.services.new_user_gate(),
                self.services.caller_log(),
                SystemTime::now(),
            )
            .expect("NewUserRegisteringSession + configured gate guarantees flow ok");
            match transition {
                NewUserPasswordTransition::Verified(s) => {
                    self.write_and_flush(NEW_USER_PASSWORD_OK_LINE).await?;
                    return Ok(GateResult::Verified(s));
                }
                NewUserPasswordTransition::Mismatch(s) => {
                    self.write_and_flush(NEW_USER_INVALID_PASSWORD_LINE).await?;
                    session = s;
                }
                NewUserPasswordTransition::TooManyFailures(s) => {
                    self.write_and_flush(NEW_USER_EXCESSIVE_FAILURES_LINE)
                        .await?;
                    return Ok(GateResult::LoggingOff(s));
                }
            }
        }
    }

    async fn read_registration_handle(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<String>, T::Error> {
        let mut attempts: u32 = 0;
        loop {
            if attempts >= MAX_REGISTRATION_HANDLE_ATTEMPTS {
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                return Ok(ReadField::LoggingOff(
                    session.into_active().apply_carrier_loss(),
                ));
            }
            let read = self
                .read_prompted(REGISTRATION_HANDLE_PROMPT, TerminalEcho::Visible)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            let trimmed = typed.trim();
            let available = !trimmed.is_empty()
                && matches!(
                    self.services.user_repo().find_by_handle(trimmed),
                    NameLookupResult::NotFound
                );
            if available {
                return Ok(ReadField::Got(session, trimmed.to_string()));
            }
            self.terminal.write(HANDLE_TAKEN_LINE).await?;
            attempts += 1;
        }
    }

    async fn read_optional_field(
        &mut self,
        mut session: NewUserRegisteringSession,
        prompt: &[u8],
    ) -> Result<ReadField<Option<String>>, T::Error> {
        let read = self.read_prompted(prompt, TerminalEcho::Visible).await?;
        let typed = match read {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                line
            }
            other => return self.handle_interrupt_registering(session, other).await,
        };
        let trimmed = typed.trim();
        let value = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        Ok(ReadField::Got(session, value))
    }

    async fn read_registration_password(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<String>, T::Error> {
        loop {
            let read = self
                .read_prompted(REGISTRATION_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let password = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            if password.trim().is_empty() {
                continue;
            }
            let confirm_read = self
                .read_prompted(REGISTRATION_PASSWORD_CONFIRM_PROMPT, TerminalEcho::Masked)
                .await?;
            let confirmed = match confirm_read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            if password == confirmed {
                return Ok(ReadField::Got(session, password));
            }
            self.terminal.write(PASSWORDS_DO_NOT_MATCH_LINE).await?;
        }
    }

    async fn read_line_length(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<u32>, T::Error> {
        loop {
            let read = self
                .read_prompted(LINE_LENGTH_PROMPT, TerminalEcho::Visible)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            let trimmed = typed.trim();
            if trimmed.is_empty() {
                return Ok(ReadField::Got(session, 0));
            }
            match trimmed.parse::<u32>() {
                Ok(value) if value <= 255 => return Ok(ReadField::Got(session, value)),
                _ => {
                    self.terminal.write(INVALID_LINE_LENGTH_LINE).await?;
                }
            }
        }
    }

    async fn read_ansi_colour(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<bool>, T::Error> {
        let read = self
            .read_prompted(ANSI_PROMPT, TerminalEcho::Visible)
            .await?;
        let typed = match read {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                line
            }
            other => return self.handle_interrupt_registering(session, other).await,
        };
        let value = !typed.trim().eq_ignore_ascii_case("N");
        Ok(ReadField::Got(session, value))
    }

    /// Common handler for `Eof` / `IdleTimedOut` while collecting a
    /// registration field. Consumes the session, applies the
    /// appropriate domain transition, and emits the wire message.
    async fn handle_interrupt_registering<TVal>(
        &mut self,
        session: NewUserRegisteringSession,
        outcome: TerminalRead,
    ) -> Result<ReadField<TVal>, T::Error> {
        let logoff = match outcome {
            TerminalRead::Eof => session.into_active().apply_carrier_loss(),
            TerminalRead::IdleTimedOut => {
                let logoff = session
                    .into_active()
                    .apply_idle_timeout(self.services.session_policy().treat_timeout_as_logoff());
                self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                logoff
            }
            TerminalRead::Line(_) => unreachable!("interrupt path is for non-Line outcomes"),
        };
        Ok(ReadField::LoggingOff(logoff))
    }

    async fn complete_new_user_registration(
        &mut self,
        session: NewUserRegisteringSession,
        profile: NewUserProfile,
    ) -> Result<SignInResult, T::Error> {
        let flow = NewUserRegistrationFlow::new(
            self.services.user_repo(),
            self.services.hasher(),
            self.services.caller_log(),
            self.services.default_ratio(),
            self.services.session_policy(),
        );
        match flow.complete_typed(session, profile, SystemTime::now()) {
            Ok(NewUserRegistrationResult::Onboarded(onboarded)) => {
                self.write_and_flush(REGISTRATION_COMPLETE_LINE).await?;
                Ok(SignInResult::Onboarded(onboarded))
            }
            Ok(NewUserRegistrationResult::LoggingOff(logging_off)) => {
                // Post-onboarded RejectLockedOrInsufficientAccess
                // short-circuited the cluster. Wire-message parity with
                // the legacy path: tell the user the logon was rejected
                // and let `finalise` close the session.
                self.write_and_flush(LOGON_REJECTED_LINE).await?;
                Ok(SignInResult::LoggingOff(logging_off))
            }
            Err(boxed) => {
                let (session, _error) = *boxed;
                // Hash, repo, or constructor error. The session is
                // unchanged (still NewUserRegistering); apply
                // carrier-loss so finalise can close it cleanly.
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                let logoff = session.into_active().apply_carrier_loss();
                Ok(SignInResult::LoggingOff(logoff))
            }
        }
    }
}

/// Result of reading a single registration field. The success arm
/// returns the session by value alongside the field — the caller
/// continues with both. The interrupt arm carries the session-now-
/// logging-off so the caller bails up to [`SignInResult::LoggingOff`].
enum ReadField<TVal> {
    Got(NewUserRegisteringSession, TVal),
    LoggingOff(LoggingOffSession),
}

/// Result of the new-user password gate. Either it passed (returns
/// the wrapper for the caller to continue with) or it failed with a
/// terminal outcome.
enum GateResult {
    Verified(NewUserRegisteringSession),
    LoggingOff(LoggingOffSession),
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
