//! New-user registration sub-flow.
//!
//! Owns the optional password gate and the registration form
//! collection (handle, location, phone, email, password,
//! line-length, ANSI). Hands the assembled profile to
//! [`crate::app::session_flow::NewUserRegistrationFlow::complete_typed`]
//! and renders the appropriate wire-message for each outcome.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow::{
    self, is_handle_available_for_registration, NewUserProfile, NewUserRegistrationFlow,
};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    ANSI_PROMPT, EMAIL_PROMPT, HANDLE_TAKEN_LINE, IDLE_TIMEOUT_LINE, INVALID_LINE_LENGTH_LINE,
    LINE_LENGTH_PROMPT, LOCATION_PROMPT, LOGON_REJECTED_LINE, NEW_USER_EXCESSIVE_FAILURES_LINE,
    NEW_USER_INVALID_PASSWORD_LINE, NEW_USER_PASSWORD_OK_LINE, NEW_USER_PASSWORD_PROMPT,
    PASSWORDS_DO_NOT_MATCH_LINE, PHONE_PROMPT, REGISTRATION_COMPLETE_LINE,
    REGISTRATION_HANDLE_PROMPT, REGISTRATION_PASSWORD_CONFIRM_PROMPT, REGISTRATION_PASSWORD_PROMPT,
    REGISTRATION_RETRIES_EXHAUSTED_LINE,
};
use crate::domain::session::typed::{
    LoggingOffSession, NewUserPasswordTransition, NewUserRegisteringSession,
    NewUserRegistrationResult, OnboardedSession,
};
use crate::domain::user::MAX_LINE_LENGTH;

/// Outcome reported by [`RegistrationFlow::run`]. Mirrors the two
/// terminal branches of `session.allium:CompleteNewUserRegistration`
/// plus the interrupt path that drops the user into `LoggingOff`.
pub(crate) enum RegistrationOutcome {
    /// Registration succeeded and the post-onboarded cluster ran
    /// clean.
    Onboarded(OnboardedSession),
    /// Registration produced a `LoggingOff` session (post-onboarded
    /// rejection, interrupt, or repo/hash failure).
    LoggingOff(LoggingOffSession),
}

/// Registration sub-flow.
pub(crate) struct RegistrationFlow<'a, T>
where
    T: Terminal,
{
    terminal: &'a mut T,
    services: &'a AppServices,
}

impl<'a, T> RegistrationFlow<'a, T>
where
    T: Terminal,
{
    /// Constructs a flow that drives `terminal` against the supplied
    /// driven adapters.
    pub(crate) fn new(terminal: &'a mut T, services: &'a AppServices) -> Self {
        Self { terminal, services }
    }

    /// Runs the registration sub-flow from a freshly initialised
    /// `new_user_registering` session through to either an onboarded
    /// account or a logging-off session.
    pub(crate) async fn run(
        &mut self,
        session: NewUserRegisteringSession,
        password_required: bool,
    ) -> Result<RegistrationOutcome, T::Error> {
        let screen = self.services.screens().new_user_password().await;
        self.terminal.write(&screen).await?;
        let session = if password_required {
            match self.run_password_gate(session).await? {
                GateResult::Verified(s) => s,
                GateResult::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
            }
        } else {
            session
        };

        let (session, handle) = match self.read_handle(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, location) = match self.read_optional_field(session, LOCATION_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, phone_number) = match self.read_optional_field(session, PHONE_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, email) = match self.read_optional_field(session, EMAIL_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, password) = match self.read_password(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, line_length) = match self.read_line_length(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
        };
        let (session, ansi_colour) = match self.read_ansi_colour(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(RegistrationOutcome::LoggingOff(s)),
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
        self.complete(session, profile).await
    }

    async fn run_password_gate(
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

    async fn read_handle(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<String>, T::Error> {
        let max_attempts = self
            .services
            .session_policy()
            .max_registration_handle_attempts();
        let mut attempts: u32 = 0;
        loop {
            if attempts >= max_attempts {
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
                other => return self.handle_interrupt(session, other).await,
            };
            if is_handle_available_for_registration(self.services.user_repo(), &typed) {
                return Ok(ReadField::Got(session, typed.trim().to_string()));
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
            other => return self.handle_interrupt(session, other).await,
        };
        let trimmed = typed.trim();
        let value = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        Ok(ReadField::Got(session, value))
    }

    async fn read_password(
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
                other => return self.handle_interrupt(session, other).await,
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
                other => return self.handle_interrupt(session, other).await,
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
                other => return self.handle_interrupt(session, other).await,
            };
            let trimmed = typed.trim();
            if trimmed.is_empty() {
                return Ok(ReadField::Got(session, 0));
            }
            match trimmed.parse::<u32>() {
                Ok(value) if value <= MAX_LINE_LENGTH => {
                    return Ok(ReadField::Got(session, value));
                }
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
            other => return self.handle_interrupt(session, other).await,
        };
        let value = !typed.trim().eq_ignore_ascii_case("N");
        Ok(ReadField::Got(session, value))
    }

    /// Common handler for `Eof` / `IdleTimedOut` while collecting a
    /// registration field. Consumes the session, applies the
    /// appropriate domain transition, and emits the wire message.
    async fn handle_interrupt<TVal>(
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

    async fn complete(
        &mut self,
        session: NewUserRegisteringSession,
        profile: NewUserProfile,
    ) -> Result<RegistrationOutcome, T::Error> {
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
                Ok(RegistrationOutcome::Onboarded(onboarded))
            }
            Ok(NewUserRegistrationResult::LoggingOff(logging_off)) => {
                // Post-onboarded RejectLockedOrInsufficientAccess
                // short-circuited the cluster. Wire-message parity
                // with the legacy path: tell the user the logon was
                // rejected and let the driver's finalise close the
                // session.
                self.write_and_flush(LOGON_REJECTED_LINE).await?;
                Ok(RegistrationOutcome::LoggingOff(logging_off))
            }
            Err(boxed) => {
                let (session, _error) = *boxed;
                // Hash, repo, or constructor error. The session is
                // unchanged (still NewUserRegistering); apply
                // carrier-loss so finalise can close it cleanly.
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                let logoff = session.into_active().apply_carrier_loss();
                Ok(RegistrationOutcome::LoggingOff(logoff))
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

/// Result of reading a single registration field. The success arm
/// returns the session by value alongside the field — the caller
/// continues with both. The interrupt arm carries the session-now-
/// logging-off so the caller bails up to
/// [`RegistrationOutcome::LoggingOff`].
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
