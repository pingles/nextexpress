//! Phase-preserving terminal failure ownership for interactive flows.
//!
//! A phase-typed session is normally threaded through an async flow by
//! value. A bare terminal `?` discards that value on failure, leaving the
//! connection boundary unable to apply carrier loss and finalise the call.
//! This module couples the original terminal error with the exact lifecycle
//! phase that was current when the I/O began.

use std::fmt;
use std::future::Future;

use crate::domain::session::typed::{
    ActivePhase, AuthenticatingSession, ConnectingSession, EndedSession, IdentifyingSession,
    LoggingOffSession, MenuSession, NewUserRegisteringSession, OnboardedSession,
};

/// Sent immediately before the connection closes on idle timeout. The
/// idle exit is written by every interactive flow (login, registration,
/// password-reset) and by the menu driver, each threading it through
/// [`preserve_phase`], so the line lives here beside that shared
/// forced-exit machinery rather than in any one flow.
pub(crate) const IDLE_TIMEOUT_LINE: &[u8] = b"Idle timeout. Goodbye.\r\n";

/// Session ownership retained when a terminal operation fails.
pub(crate) enum SessionAtTerminalFailure {
    /// Failure before the initial transition to name identification.
    Connecting(ConnectingSession),
    /// Failure in one of the input-capable active phases.
    Active(ActivePhase),
    /// Failure after a logoff reason has already been established.
    LoggingOff(LoggingOffSession),
    /// Failure after the lifecycle has already reached its terminal phase.
    Ended(EndedSession),
}

impl SessionAtTerminalFailure {
    fn name(&self) -> &'static str {
        match self {
            Self::Connecting(_) => "Connecting",
            Self::Active(ActivePhase::Identifying(_)) => "Identifying",
            Self::Active(ActivePhase::Authenticating(_)) => "Authenticating",
            Self::Active(ActivePhase::NewUserRegistering(_)) => "NewUserRegistering",
            Self::Active(ActivePhase::Onboarded(_)) => "Onboarded",
            Self::Active(ActivePhase::Menu(_)) => "Menu",
            Self::LoggingOff(_) => "LoggingOff",
            Self::Ended(_) => "Ended",
        }
    }

    /// Converts a recoverable phase into the logging-off phase used by the
    /// connection completion boundary.
    ///
    /// `Connecting` and active failures apply carrier loss. An existing
    /// `LoggingOff` value is returned unchanged so its original reason is
    /// retained. `Ended` has already completed its lifecycle and therefore
    /// returns `None` rather than triggering a second finalisation.
    #[must_use]
    pub(crate) fn into_logging_off(self) -> Option<LoggingOffSession> {
        match self {
            Self::Connecting(session) => Some(session.apply_carrier_loss()),
            Self::Active(session) => Some(session.apply_carrier_loss()),
            Self::LoggingOff(session) => Some(session),
            Self::Ended(_session) => None,
        }
    }
}

/// A terminal error coupled to the session phase that owned the operation.
pub(crate) struct SessionTerminalError<E> {
    phase: SessionAtTerminalFailure,
    source: E,
}

impl<E> fmt::Debug for SessionTerminalError<E>
where
    E: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionTerminalError")
            .field("phase", &self.phase.name())
            .field("source", &self.source)
            .finish()
    }
}

impl<E> SessionTerminalError<E> {
    /// Constructs a failure from any phase-typed session supported by
    /// [`SessionAtTerminalFailure`].
    pub(crate) fn new<S>(session: S, source: E) -> Self
    where
        S: Into<SessionAtTerminalFailure>,
    {
        Self {
            phase: session.into(),
            source,
        }
    }

    /// Separates retained session ownership from the original terminal error.
    #[must_use]
    pub(crate) fn into_parts(self) -> (SessionAtTerminalFailure, E) {
        (self.phase, self.source)
    }
}

/// Result type used by phase-owning terminal-driven flows.
pub(crate) type SessionFlowResult<T, E> = Result<T, SessionTerminalError<E>>;

/// Runs one terminal operation while retaining `session` if it fails.
///
/// On success, the unchanged phase wrapper is returned alongside the
/// operation output so the caller can continue threading ownership. On
/// failure, the wrapper is moved into [`SessionTerminalError`] with the
/// adapter's original error unchanged.
///
/// # Errors
/// Returns [`SessionTerminalError`] when `operation` fails, retaining both
/// `session` and the original error value.
pub(crate) async fn preserve_phase<S, T, E, F>(
    session: S,
    operation: F,
) -> SessionFlowResult<(S, T), E>
where
    S: Into<SessionAtTerminalFailure>,
    F: Future<Output = Result<T, E>>,
{
    match operation.await {
        Ok(output) => Ok((session, output)),
        Err(source) => Err(SessionTerminalError::new(session, source)),
    }
}

impl From<ConnectingSession> for SessionAtTerminalFailure {
    fn from(session: ConnectingSession) -> Self {
        Self::Connecting(session)
    }
}

impl From<ActivePhase> for SessionAtTerminalFailure {
    fn from(session: ActivePhase) -> Self {
        Self::Active(session)
    }
}

macro_rules! impl_active_failure_conversion {
    ($wrapper:ty) => {
        impl From<$wrapper> for SessionAtTerminalFailure {
            fn from(session: $wrapper) -> Self {
                Self::Active(session.into_active())
            }
        }
    };
}

impl_active_failure_conversion!(IdentifyingSession);
impl_active_failure_conversion!(AuthenticatingSession);
impl_active_failure_conversion!(NewUserRegisteringSession);
impl_active_failure_conversion!(OnboardedSession);
impl_active_failure_conversion!(MenuSession);

impl From<LoggingOffSession> for SessionAtTerminalFailure {
    fn from(session: LoggingOffSession) -> Self {
        Self::LoggingOff(session)
    }
}

impl From<EndedSession> for SessionAtTerminalFailure {
    fn from(session: EndedSession) -> Self {
        Self::Ended(session)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use crate::domain::session::typed::ConnectingSession;
    use crate::domain::session::{LogoffReason, LogonChannel, SessionState};

    use super::{preserve_phase, SessionAtTerminalFailure};

    #[tokio::test]
    async fn preserve_phase_returns_the_connecting_session_with_the_original_error() {
        let connecting =
            ConnectingSession::accept(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH)
                .expect("fresh session");

        let Err(failure) =
            preserve_phase(connecting, async { Err::<(), _>("terminal failed") }).await
        else {
            panic!("terminal failure must carry the session");
        };
        assert_eq!(
            format!("{failure:?}"),
            "SessionTerminalError { phase: \"Connecting\", source: \"terminal failed\" }"
        );

        let (phase, source) = failure.into_parts();
        assert_eq!(source, "terminal failed");
        let SessionAtTerminalFailure::Connecting(connecting) = phase else {
            panic!("connecting ownership must be retained exactly");
        };
        let inner = connecting.apply_carrier_loss().into_inner();
        assert_eq!(inner.state(), SessionState::LoggingOff);
        assert_eq!(inner.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[tokio::test]
    async fn preserve_phase_returns_the_active_variant_with_the_original_error() {
        let identifying =
            ConnectingSession::accept(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH)
                .expect("fresh session")
                .prompt_for_name();

        let Err(failure) = preserve_phase(identifying, async { Err::<(), _>("read failed") }).await
        else {
            panic!("terminal failure must carry the session");
        };

        let (phase, source) = failure.into_parts();
        assert_eq!(source, "read failed");
        assert!(matches!(
            phase,
            SessionAtTerminalFailure::Active(
                crate::domain::session::typed::ActivePhase::Identifying(_)
            )
        ));
    }
}
