//! Forced password-reset sub-flow.
//!
//! Runs after authentication and auto-rejoin, but before menu entry,
//! when `session.allium:EnterMenu` reports that the bound user has
//! `force_password_reset` set. The domain reset rule already lives in
//! [`crate::app::session_flow::complete_password_reset`]; this module
//! owns the terminal prompts, retry budget and interrupt handling.

use crate::app::clock::Clock;
use crate::app::services::AppServices;
use crate::app::session_flow::{self, CompletePasswordResetFlowError};
use crate::app::session_terminal::{preserve_phase, SessionFlowResult};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::IDLE_TIMEOUT_LINE;
use crate::domain::password::PasswordHasher;
use crate::domain::session::typed::{LoggingOffSession, OnboardedSession};
use crate::domain::session::SessionPolicy;
use crate::domain::user_repository::UserRepository;

const MAX_PASSWORD_RESET_ATTEMPTS: u32 = 3;

/// Notice shown when a user must rotate their password before menu
/// entry. Verbatim from `amiexpress/express.e:29805`.
pub(crate) const PASSWORD_RESET_REQUIRED_LINE: &[u8] =
    b"\r\nYour account requires your password to be changed.\r\n\r\n";

/// Prompt for the first forced-reset password entry. Verbatim from
/// `amiexpress/express.e:29808`.
const PASSWORD_RESET_PROMPT: &[u8] = b"Enter New Password: ";

/// Prompt for confirming the forced-reset password. Verbatim from
/// `amiexpress/express.e:29810`.
const PASSWORD_RESET_CONFIRM_PROMPT: &[u8] = b"Reenter New Password: ";

/// Sent when the two forced-reset password entries don't match.
/// Verbatim from `amiexpress/express.e:29835`.
const PASSWORD_RESET_MISMATCH_LINE: &[u8] =
    b"\r\nPasswords do not match, please try again.\r\n\r\n";

/// Sent when the forced-reset candidate matches the current password.
/// Verbatim from `amiexpress/express.e:29813`.
const PASSWORD_RESET_SAME_AS_CURRENT_LINE: &[u8] =
    b"\r\nYour new password must be different from your old password...\r\n\r\n";

/// Sent when the forced-reset candidate fails the configured password
/// strength policy. The legacy distinguishes length vs category
/// failures, but the app-layer rule currently reports a single weak
/// password error.
const PASSWORD_RESET_WEAK_LINE: &[u8] = b"\r\nInvalid PassWord\r\n";

/// Sent when the user exhausts forced-reset attempts without changing
/// their password. Verbatim from `amiexpress/express.e:29841`.
pub(crate) const PASSWORD_RESET_EXHAUSTED_LINE: &[u8] =
    b"\r\nYou have not updated your password so you will now be disconnected...\r\n\r\n";

/// Outcome reported by [`PasswordResetFlow::run`].
pub(crate) enum PasswordResetOutcome {
    /// Password reset succeeded; the caller may retry menu entry.
    Onboarded(OnboardedSession),
    /// The reset flow reached a normal logging-off transition.
    LoggingOff(LoggingOffSession),
    /// A persistence or hashing failure left the session unsuitable for
    /// menu entry; the driver closes the connection without finalising.
    Aborted,
}

/// Driven ports and policy needed by [`PasswordResetFlow`].
pub(crate) struct PasswordResetServices<'a> {
    user_repo: &'a (dyn UserRepository + Send + Sync),
    hasher: &'a (dyn PasswordHasher + Send + Sync),
    clock: &'a (dyn Clock + Send + Sync),
    session_policy: SessionPolicy,
}

impl<'a> PasswordResetServices<'a> {
    /// Constructs the narrow dependency set for the reset flow.
    pub(crate) fn new(
        user_repo: &'a (dyn UserRepository + Send + Sync),
        hasher: &'a (dyn PasswordHasher + Send + Sync),
        clock: &'a (dyn Clock + Send + Sync),
        session_policy: SessionPolicy,
    ) -> Self {
        Self {
            user_repo,
            hasher,
            clock,
            session_policy,
        }
    }
}

impl<'a> From<&'a AppServices> for PasswordResetServices<'a> {
    fn from(services: &'a AppServices) -> Self {
        Self::new(
            services.user_repo.as_ref(),
            services.hasher.as_ref(),
            services.clock.as_ref(),
            services.session_policy,
        )
    }
}

/// Terminal-driven forced-password-reset flow.
pub(crate) struct PasswordResetFlow<'a, T>
where
    T: Terminal,
{
    terminal: &'a mut T,
    services: PasswordResetServices<'a>,
}

impl<'a, T> PasswordResetFlow<'a, T>
where
    T: Terminal,
{
    /// Constructs a flow that drives `terminal` against the supplied
    /// driven adapters.
    pub(crate) fn new(terminal: &'a mut T, services: PasswordResetServices<'a>) -> Self {
        Self { terminal, services }
    }

    /// Prompts for and applies a forced password reset.
    ///
    /// # Errors
    /// Returns a phase-carrying terminal failure when reset-flow I/O fails.
    /// Hashing and persistence failures remain represented by
    /// [`PasswordResetOutcome::Aborted`].
    pub(crate) async fn run(
        &mut self,
        mut session: OnboardedSession,
    ) -> SessionFlowResult<PasswordResetOutcome, T::Error> {
        let (next_session, ()) =
            preserve_phase(session, self.write_and_flush(PASSWORD_RESET_REQUIRED_LINE)).await?;
        session = next_session;
        let mut attempts = 0;
        while attempts < MAX_PASSWORD_RESET_ATTEMPTS {
            let candidate = match self.read_password(session, PASSWORD_RESET_PROMPT).await? {
                PasswordRead::Got(next_session, candidate) => {
                    session = next_session;
                    candidate
                }
                PasswordRead::LoggingOff(logging_off) => {
                    return Ok(PasswordResetOutcome::LoggingOff(logging_off));
                }
            };
            let confirm = match self
                .read_password(session, PASSWORD_RESET_CONFIRM_PROMPT)
                .await?
            {
                PasswordRead::Got(next_session, confirm) => {
                    session = next_session;
                    confirm
                }
                PasswordRead::LoggingOff(logging_off) => {
                    return Ok(PasswordResetOutcome::LoggingOff(logging_off));
                }
            };
            if candidate != confirm {
                attempts += 1;
                let (next_session, ()) =
                    preserve_phase(session, self.write_and_flush(PASSWORD_RESET_MISMATCH_LINE))
                        .await?;
                session = next_session;
                continue;
            }

            let mut inner = session.into_inner();
            match session_flow::complete_password_reset(
                &mut inner,
                &candidate,
                self.services.user_repo,
                self.services.hasher,
                self.services.session_policy,
                self.services.clock.now(),
            ) {
                Ok(()) => {
                    return Ok(PasswordResetOutcome::Onboarded(
                        OnboardedSession::from_session(inner),
                    ));
                }
                Err(CompletePasswordResetFlowError::WeakPassword) => {
                    attempts += 1;
                    session = OnboardedSession::from_session(inner);
                    let (next_session, ()) =
                        preserve_phase(session, self.write_and_flush(PASSWORD_RESET_WEAK_LINE))
                            .await?;
                    session = next_session;
                }
                Err(CompletePasswordResetFlowError::SameAsCurrent) => {
                    attempts += 1;
                    session = OnboardedSession::from_session(inner);
                    let (next_session, ()) = preserve_phase(
                        session,
                        self.write_and_flush(PASSWORD_RESET_SAME_AS_CURRENT_LINE),
                    )
                    .await?;
                    session = next_session;
                }
                Err(CompletePasswordResetFlowError::Hash(error)) => {
                    eprintln!("password reset: failed to hash password: {error}");
                    return Ok(PasswordResetOutcome::Aborted);
                }
                Err(CompletePasswordResetFlowError::Save(error)) => {
                    eprintln!("password reset: failed to persist user: {error}");
                    return Ok(PasswordResetOutcome::Aborted);
                }
                Err(CompletePasswordResetFlowError::Session(error)) => {
                    eprintln!("password reset: unexpected session guard failed: {error}");
                    return Ok(PasswordResetOutcome::Aborted);
                }
            }
        }

        let (session, ()) =
            preserve_phase(session, self.write_and_flush(PASSWORD_RESET_EXHAUSTED_LINE)).await?;
        Ok(PasswordResetOutcome::LoggingOff(
            session.into_active().apply_carrier_loss(),
        ))
    }

    async fn read_password(
        &mut self,
        mut session: OnboardedSession,
        prompt: &[u8],
    ) -> SessionFlowResult<PasswordRead, T::Error> {
        let (next_session, read) =
            preserve_phase(session, self.read_prompted(prompt, TerminalEcho::Masked)).await?;
        session = next_session;
        match read {
            TerminalRead::Line(line) => {
                session.record_input(self.services.clock.now());
                if line.trim().is_empty() {
                    Ok(PasswordRead::Got(session, String::new()))
                } else {
                    Ok(PasswordRead::Got(session, line))
                }
            }
            TerminalRead::Eof => Ok(PasswordRead::LoggingOff(
                session.into_active().apply_carrier_loss(),
            )),
            TerminalRead::IdleTimedOut => {
                let logoff = session
                    .into_active()
                    .apply_idle_timeout(self.services.session_policy.treat_timeout_as_logoff());
                let (logoff, ()) =
                    preserve_phase(logoff, self.write_and_flush(IDLE_TIMEOUT_LINE)).await?;
                Ok(PasswordRead::LoggingOff(logoff))
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

enum PasswordRead {
    Got(OnboardedSession, String),
    LoggingOff(LoggingOffSession),
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::session_terminal::SessionAtTerminalFailure;
    use crate::app::terminal::{TerminalFuture, TerminalRead};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::typed::{ActivePhase, OnboardedSession};
    use crate::domain::session::{
        apply_password_match, CallId, LogonChannel, Session, SessionPolicy,
    };
    use crate::domain::user::User;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        Write,
    }

    struct FaultTerminal {
        inputs: VecDeque<TerminalRead>,
        fail_write_at: Option<usize>,
        fail_write_containing: Option<Vec<u8>>,
        write_count: usize,
    }

    impl FaultTerminal {
        fn failing_first_write() -> Self {
            Self {
                inputs: VecDeque::new(),
                fail_write_at: Some(0),
                fail_write_containing: None,
                write_count: 0,
            }
        }

        fn idle_then_fail_timeout_notice() -> Self {
            Self {
                inputs: [TerminalRead::IdleTimedOut].into(),
                fail_write_at: None,
                fail_write_containing: Some(IDLE_TIMEOUT_LINE.to_vec()),
                write_count: 0,
            }
        }
    }

    impl Terminal for FaultTerminal {
        type Error = Fault;

        fn write<'b>(&'b mut self, bytes: &'b [u8]) -> TerminalFuture<'b, (), Self::Error> {
            Box::pin(async move {
                let operation = self.write_count;
                self.write_count += 1;
                if self.fail_write_at == Some(operation)
                    || self.fail_write_containing.as_deref().is_some_and(|needle| {
                        bytes.windows(needle.len()).any(|window| window == needle)
                    })
                {
                    return Err(Fault::Write);
                }
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
            Box::pin(async move { Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof)) })
        }
    }

    #[tokio::test]
    async fn opening_notice_failure_retains_the_onboarded_session() {
        let (fixture, session) = reset_fixture_and_session(SessionPolicy::default());
        let mut terminal = FaultTerminal::failing_first_write();

        let Err(failure) = PasswordResetFlow::new(&mut terminal, fixture.services())
            .run(session)
            .await
        else {
            panic!("notice failure must escape with session ownership");
        };

        let (phase, source) = failure.into_parts();
        assert_eq!(source, Fault::Write);
        assert!(matches!(
            phase,
            SessionAtTerminalFailure::Active(ActivePhase::Onboarded(_))
        ));
    }

    #[tokio::test]
    async fn idle_timeout_transitions_before_a_timeout_notice_failure() {
        let (fixture, session) = reset_fixture_and_session(SessionPolicy::default());
        let mut terminal = FaultTerminal::idle_then_fail_timeout_notice();

        let Err(failure) = PasswordResetFlow::new(&mut terminal, fixture.services())
            .run(session)
            .await
        else {
            panic!("timeout notice failure must escape with session ownership");
        };

        let (phase, source) = failure.into_parts();
        assert_eq!(source, Fault::Write);
        assert!(matches!(phase, SessionAtTerminalFailure::LoggingOff(_)));
    }

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

    #[tokio::test]
    async fn mismatched_confirmations_exhaust_the_reset_attempt_budget() {
        let (fixture, session) = reset_fixture_and_session(SessionPolicy::default());
        let mut terminal = FakeTerminal::new([
            line("Newpass123"),
            line("Otherpass123"),
            line("Second123"),
            line("Different123"),
            line("Third123"),
            line("Mismatch123"),
        ]);

        let outcome = PasswordResetFlow::new(&mut terminal, fixture.services())
            .run(session)
            .await
            .expect("flow completes");

        assert!(matches!(outcome, PasswordResetOutcome::LoggingOff(_)));
        assert_eq!(
            count_occurrences(terminal.output(), PASSWORD_RESET_MISMATCH_LINE),
            3
        );
        assert_contains(terminal.output(), PASSWORD_RESET_EXHAUSTED_LINE);
        assert_eq!(terminal.echo_modes, vec![TerminalEcho::Masked; 6]);
    }

    #[tokio::test]
    async fn weak_passwords_exhaust_the_reset_attempt_budget() {
        let policy = SessionPolicy::default().with_min_password_length(10);
        let (fixture, session) = reset_fixture_and_session(policy);
        let mut terminal = FakeTerminal::new([
            line("short"),
            line("short"),
            line("tiny"),
            line("tiny"),
            line("small"),
            line("small"),
        ]);

        let outcome = PasswordResetFlow::new(&mut terminal, fixture.services())
            .run(session)
            .await
            .expect("flow completes");

        assert!(matches!(outcome, PasswordResetOutcome::LoggingOff(_)));
        assert_eq!(
            count_occurrences(terminal.output(), PASSWORD_RESET_WEAK_LINE),
            3
        );
        assert_contains(terminal.output(), PASSWORD_RESET_EXHAUSTED_LINE);
        assert_eq!(terminal.echo_modes, vec![TerminalEcho::Masked; 6]);
    }

    struct ResetFixture {
        user_repo: InMemoryUserRepository,
        hasher: Pbkdf2PasswordHasher,
        clock: crate::adapters::system_clock::SystemClock,
        policy: SessionPolicy,
    }

    impl ResetFixture {
        fn services(&self) -> PasswordResetServices<'_> {
            PasswordResetServices::new(&self.user_repo, &self.hasher, &self.clock, self.policy)
        }
    }

    fn reset_fixture_and_session(policy: SessionPolicy) -> (ResetFixture, OnboardedSession) {
        let user = alice_with_reset_pending();
        let fixture = ResetFixture {
            user_repo: InMemoryUserRepository::new(vec![user.clone()]),
            hasher: Pbkdf2PasswordHasher::new(),
            clock: crate::adapters::system_clock::SystemClock,
            policy,
        };
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("alice", user)
            .expect("identified");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
            CallId::new(1),
        )
        .expect("matched");
        (fixture, OnboardedSession::from_session(session))
    }

    fn alice_with_reset_pending() -> User {
        let kind = PasswordHashKind::Pbkdf210000;
        let computed = Pbkdf2PasswordHasher::new()
            .compute_password_hash("secret", kind)
            .expect("hash");
        let mut user = User::new(
            2,
            "alice".to_string(),
            kind,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user");
        user.set_force_password_reset(true);
        user
    }

    fn line(text: &str) -> TerminalRead {
        TerminalRead::Line(text.to_string())
    }

    fn assert_contains(haystack: &[u8], needle: &[u8]) {
        assert!(
            haystack.windows(needle.len()).any(|w| w == needle),
            "expected output to contain {}",
            String::from_utf8_lossy(needle)
        );
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }
}
