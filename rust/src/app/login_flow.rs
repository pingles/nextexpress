//! Login sub-flow of the BBS workflow.
//!
//! Owns the name prompt, the authentication prompt, and the dispatch
//! into the new-user registration sub-flow. Reports back to
//! [`crate::app::session_driver`] which branch fired so the driver can
//! either dispatch into [`crate::app::registration_flow::RegistrationFlow`],
//! enter the menu, or finalise the session.

use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow::{self, VerifyPasswordFlowError};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{ANSI_PROMPT, IDLE_TIMEOUT_LINE, LOGON_REJECTED_LINE};
use crate::domain::session::typed::{
    AuthenticatingSession, EndedSession, IdentifyingSession, LoggingOffSession,
    NameTypedTransition, NewUserRegisteringSession, OnboardedSession,
    VerifyPasswordRejectionReason, VerifyPasswordTransition,
};

/// Prompt sent before reading the user's handle. Mirrors the original
/// `AmiExpress` wire format: a CRLF prefix and trailing space around the
/// default `NAME_PROMPT` of `Enter your Name:` (see
/// `amiexpress/express.e:29571` and `:31774`).
const NAME_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's password.
pub(crate) const PASSWORD_PROMPT: &[u8] = b"PassWord: ";

/// Sent after a not-found name lookup to invite a retry.
const UNKNOWN_USER_LINE: &[u8] = b"Unknown user.\r\n";

/// Sent when the user has burned through all five name retries.
const TOO_MANY_RETRIES_LINE: &[u8] = b"Too many failed login attempts. Goodbye.\r\n";

/// Sent after a successful authentication.
const AUTHENTICATED_LINE: &[u8] = b"Authenticated.\r\n";

/// Sent when the password didn't match.
const WRONG_PASSWORD_LINE: &[u8] = b"Incorrect password.\r\n";

/// Sent when the post-auth cluster locks the account.
const ACCOUNT_LOCKED_LINE: &[u8] = b"Account locked. Goodbye.\r\n";

/// Sent when the per-session retry budget is exhausted at the password
/// prompt.
const TOO_MANY_PASSWORD_FAILURES_LINE: &[u8] = b"Too many password failures. Goodbye.\r\n";

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
    /// An unrecoverable persistence failure during sign-in (e.g. the
    /// user record could not be saved after a correct password). The
    /// session state is indeterminate, so there is nothing to finalise;
    /// the driver closes the connection. Logged operationally, not a
    /// normal logoff.
    Aborted,
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
        // Graphics question, asked at connect before the name prompt
        // (legacy `amiexpress/express.e:29528`). An answer beginning
        // `n`/`N` selects ASCII and turns the terminal's live colour
        // mode off; the default (CR / `Y`) keeps ANSI. EOF / idle here
        // close the session exactly as they do at the name prompt.
        match self
            .read_prompted(ANSI_PROMPT, TerminalEcho::Visible)
            .await?
        {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                if matches!(line.trim().chars().next(), Some('n' | 'N')) {
                    self.terminal.set_ansi_colour(false);
                }
            }
            TerminalRead::Eof => {
                return Ok(LoginOutcome::LoggingOff(
                    session.into_active().apply_carrier_loss(),
                ));
            }
            TerminalRead::IdleTimedOut => {
                let logoff = session
                    .into_active()
                    .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                return Ok(LoginOutcome::LoggingOff(logoff));
            }
        }
        // Banner / title screen, rendered after the graphics answer so
        // an ASCII caller gets it ANSI-stripped — the legacy
        // `SCREEN_BBSTITLE` order (`amiexpress/express.e:29552`, shown
        // after the `A/r/n` question).
        let banner = self.services.screens.as_ref().banner().await;
        self.terminal.write(&banner).await?;
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
                    let logoff = session
                        .into_active()
                        .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(LoginOutcome::LoggingOff(logoff));
                }
            };
            let trimmed = line.trim();
            let transition = match session_flow::name_typed(
                session,
                trimmed,
                self.services.user_repo.as_ref(),
                self.services.new_user_gate.as_ref(),
                SystemTime::now(),
            ) {
                Ok(transition) => transition,
                Err(error) => {
                    eprintln!("login: failed to resolve typed user name: {error}");
                    return Ok(LoginOutcome::Aborted);
                }
            };
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
                    let screen = self.services.screens.as_ref().no_new_users().await;
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
                    let logoff = session
                        .into_active()
                        .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(LoginOutcome::LoggingOff(logoff));
                }
            };
            let transition = match session_flow::verify_password(
                session,
                password.trim(),
                self.services.user_repo.as_ref(),
                self.services.hasher.as_ref(),
                self.services.caller_log.as_ref(),
                self.services.session_policy,
                SystemTime::now(),
            ) {
                Ok(transition) => transition,
                Err(VerifyPasswordFlowError::Save(error)) => {
                    // Persistence failed *after* the password check. The
                    // session was consumed by the rule and its persisted
                    // state is now indeterminate, so we cannot safely
                    // admit the caller. Log operationally and close the
                    // connection rather than panicking the task.
                    eprintln!("login: failed to persist user after password verification: {error}");
                    return Ok(LoginOutcome::Aborted);
                }
                Err(VerifyPasswordFlowError::Session(error)) => {
                    // The `AuthenticatingSession` wrapper makes the
                    // wrong-state / user-missing modes unrepresentable,
                    // so this arm is genuinely unreachable.
                    unreachable!(
                        "AuthenticatingSession guarantees Authenticating + bound user: {error:?}"
                    );
                }
            };
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
        let timeout = self.services.session_policy.input_timeout();
        crate::app::terminal::read_prompted(self.terminal, prompt, echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        crate::app::terminal::write_and_flush(self.terminal, bytes).await
    }
}
