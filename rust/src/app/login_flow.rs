//! Login sub-flow of the BBS workflow.
//!
//! Owns the name prompt, the authentication prompt, and the dispatch
//! into the new-user registration sub-flow. Reports back to
//! [`crate::app::session_driver`] which branch fired so the driver can
//! either dispatch into [`crate::app::registration_flow::RegistrationFlow`],
//! enter the menu, or finalise the session.

use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    ACCOUNT_LOCKED_LINE, AUTHENTICATED_LINE, IDLE_TIMEOUT_LINE, LOGON_REJECTED_LINE, NAME_PROMPT,
    PASSWORD_PROMPT, TOO_MANY_PASSWORD_FAILURES_LINE, TOO_MANY_RETRIES_LINE, UNKNOWN_USER_LINE,
    WRONG_PASSWORD_LINE,
};
use crate::domain::session::typed::{
    AuthenticatingSession, EndedSession, IdentifyingSession, LoggingOffSession,
    NameTypedTransition, NewUserRegisteringSession, OnboardedSession,
    VerifyPasswordRejectionReason, VerifyPasswordTransition,
};

/// Outcome reported by [`LoginFlow::identify`]. The new-user branch is
/// surfaced as a discrete variant so the driver dispatches into the
/// registration flow without `LoginFlow` reaching across module
/// boundaries.
pub(crate) enum LoginOutcome {
    /// Sign-in produced an authenticated, fully-onboarded session.
    Onboarded(OnboardedSession),
    /// Sign-in moved the session into `LoggingOff` (lockout,
    /// rejection, timeout, carrier loss, ...).
    LoggingOff(LoggingOffSession),
    /// Handle retry budget exhausted; the session moved straight to
    /// `Ended`.
    Ended(EndedSession),
    /// The user typed the `NEW` literal. The driver hands off to
    /// [`crate::app::registration_flow::RegistrationFlow`] with the
    /// session it produced.
    NeedsRegistration {
        /// The fresh `new_user_registering` session.
        session: NewUserRegisteringSession,
        /// Whether the gate password must be verified before the
        /// registration form starts.
        password_required: bool,
    },
}

/// Login sub-flow: handles the handle prompt and password loop.
pub(crate) struct LoginFlow<'a, T>
where
    T: Terminal,
{
    terminal: &'a mut T,
    services: &'a AppServices,
}

impl<'a, T> LoginFlow<'a, T>
where
    T: Terminal,
{
    /// Constructs a flow that drives `terminal` against the supplied
    /// driven adapters.
    pub(crate) fn new(terminal: &'a mut T, services: &'a AppServices) -> Self {
        Self { terminal, services }
    }

    /// Runs the name-prompt loop until the session reaches a terminal
    /// outcome (auth result, registration request, retry exhaustion,
    /// or interrupt).
    pub(crate) async fn identify(
        &mut self,
        mut session: IdentifyingSession,
    ) -> Result<LoginOutcome, T::Error> {
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
                    return Ok(LoginOutcome::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(LoginOutcome::LoggingOff(logoff));
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
                    return self.authenticate(authenticating).await;
                }
                NameTypedTransition::Identifying(retry) => {
                    self.terminal.write(UNKNOWN_USER_LINE).await?;
                    session = retry;
                }
                NameTypedTransition::NewUserRegistering {
                    session: registering,
                    password_required,
                } => {
                    return Ok(LoginOutcome::NeedsRegistration {
                        session: registering,
                        password_required,
                    });
                }
                NameTypedTransition::Disallowed(logging_off) => {
                    let screen = self.services.screens().no_new_users().await;
                    self.terminal.write(&screen).await?;
                    self.terminal.flush().await?;
                    return Ok(LoginOutcome::LoggingOff(logging_off));
                }
                NameTypedTransition::Ended(ended) => {
                    self.terminal.write(TOO_MANY_RETRIES_LINE).await?;
                    self.terminal.flush().await?;
                    return Ok(LoginOutcome::Ended(ended));
                }
            }
        }
    }

    async fn authenticate(
        &mut self,
        mut session: AuthenticatingSession,
    ) -> Result<LoginOutcome, T::Error> {
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
                    return Ok(LoginOutcome::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(LoginOutcome::LoggingOff(logoff));
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
                    return Ok(LoginOutcome::Onboarded(onboarded));
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
                    return Ok(LoginOutcome::LoggingOff(logging_off));
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
