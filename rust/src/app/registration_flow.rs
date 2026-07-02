//! New-user registration sub-flow.
//!
//! Owns the optional password gate and the registration form
//! collection (handle, location, phone, email, password,
//! line-length, ANSI). Hands the assembled profile to
//! [`crate::app::session_flow::NewUserRegistrationFlow::complete`]
//! and renders the appropriate wire-message for each outcome.

use std::collections::BTreeSet;

use crate::app::services::AppServices;
use crate::app::session_flow::{
    self, is_handle_available_for_registration, NewUserProfile, NewUserRegistrationFlow,
};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{ANSI_PROMPT, IDLE_TIMEOUT_LINE, LOGON_REJECTED_LINE};
use crate::domain::session::typed::{
    LoggingOffSession, NewUserPasswordTransition, NewUserRegisteringSession,
    NewUserRegistrationResult, OnboardedSession,
};
use crate::domain::user::MAX_LINE_LENGTH;

/// Prompt asking a registering user for the handle they want.
/// Mirrors the wire format of [`NAME_PROMPT`] (CRLF prefix, trailing
/// space) — `amiexpress/express.e:30141`.
///
/// [`NAME_PROMPT`]: crate::app::login_flow::NAME_PROMPT
pub(crate) const REGISTRATION_HANDLE_PROMPT: &[u8] = b"\r\nEnter your Name: ";

/// Prompt for the user's location during registration. Verbatim from
/// `amiexpress/express.e:30194`.
pub(crate) const LOCATION_PROMPT: &[u8] = b"City, State: ";

/// Prompt for the user's phone number during registration. Verbatim
/// from `amiexpress/express.e:30204`.
pub(crate) const PHONE_PROMPT: &[u8] = b"Phone Number: ";

/// Prompt for the user's email address during registration. Verbatim
/// from `amiexpress/express.e:30215`.
pub(crate) const EMAIL_PROMPT: &[u8] = b"E-Mail Address: ";

/// First password prompt during registration. Verbatim from
/// `amiexpress/express.e:30227`.
pub(crate) const REGISTRATION_PASSWORD_PROMPT: &[u8] = b"Enter a PassWord: ";

/// Confirmation password prompt during registration. Verbatim from
/// `amiexpress/express.e:30233`.
pub(crate) const REGISTRATION_PASSWORD_CONFIRM_PROMPT: &[u8] = b"Reenter the PassWord: ";

/// Prompt asking the user for their preferred line length. Simplified
/// from `amiexpress/express.e:11307` (which streams a 70..2 ladder
/// before asking).
pub(crate) const LINE_LENGTH_PROMPT: &[u8] = b"Enter line length (or 0 for Auto): ";

/// Prompt for the sysop-set new-user password gate. Verbatim from
/// `amiexpress/express.e:30018`.
pub(crate) const NEW_USER_PASSWORD_PROMPT: &[u8] = b"Enter New User Password: ";

/// Sent when the typed handle is `NEW` (reserved) or already taken
/// during registration. Followed by a fresh handle prompt.
const HANDLE_TAKEN_LINE: &[u8] = b"That name is taken. Try another.\r\n";

/// Sent when the user has burned through five handle retries during
/// registration.
const REGISTRATION_RETRIES_EXHAUSTED_LINE: &[u8] =
    b"Too many failed registration attempts. Goodbye.\r\n";

/// Sent when the two registration passwords don't match. Verbatim from
/// `amiexpress/express.e:30237`.
const PASSWORDS_DO_NOT_MATCH_LINE: &[u8] = b"\r\nPasswords do not match, try again..\r\n";

/// Sent when the line-length input doesn't parse as a number in
/// `0..=255`.
const INVALID_LINE_LENGTH_LINE: &[u8] = b"Invalid line length.\r\n";

/// Sent after the registration succeeds; immediately followed by the
/// menu sequence inherited by every authenticated session.
const REGISTRATION_COMPLETE_LINE: &[u8] = b"\r\nWelcome aboard!\r\n";

/// Sent after each failed new-user password attempt. Verbatim from
/// `amiexpress/express.e:30036`.
const NEW_USER_INVALID_PASSWORD_LINE: &[u8] = b"Invalid PassWord\r\n";

/// Sent when the gate's retry budget is exhausted. Verbatim from
/// `amiexpress/express.e:30039`. Followed by a goodbye line.
const NEW_USER_EXCESSIVE_FAILURES_LINE: &[u8] = b"\r\nExcessive Password Failure\r\nGoodbye.\r\n";

/// Sent on a successful gate match. Verbatim from
/// `amiexpress/express.e:30046`.
const NEW_USER_PASSWORD_OK_LINE: &[u8] = b"Correct\r\n";

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
        let screen = self.services.screens.as_ref().new_user_password().await;
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
                    session.record_input(self.services.clock.now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(GateResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session
                        .into_active()
                        .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(GateResult::LoggingOff(logoff));
                }
            };
            let transition = session_flow::verify_new_user_password(
                session,
                typed.trim(),
                self.services.new_user_gate.as_ref(),
                self.services.caller_log.as_ref(),
                self.services.clock.now(),
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
            .session_policy
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
                    session.record_input(self.services.clock.now());
                    line
                }
                other => return self.handle_interrupt(session, other).await,
            };
            match is_handle_available_for_registration(self.services.user_repo.as_ref(), &typed) {
                Ok(true) => return Ok(ReadField::Got(session, typed.trim().to_string())),
                Ok(false) => {}
                Err(error) => {
                    eprintln!("registration: failed to check handle availability: {error}");
                    self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                        .await?;
                    return Ok(ReadField::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
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
                session.record_input(self.services.clock.now());
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
                    session.record_input(self.services.clock.now());
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
                    session.record_input(self.services.clock.now());
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
                    session.record_input(self.services.clock.now());
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
                session.record_input(self.services.clock.now());
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
                    .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
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
            self.services.user_repo.as_ref(),
            self.services.hasher.as_ref(),
            self.services.caller_log.as_ref(),
            self.services.default_ratio,
            self.services.session_policy,
        );
        match flow.complete(session, profile, self.services.clock.now()) {
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
        let timeout = self.services.session_policy.input_timeout();
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
