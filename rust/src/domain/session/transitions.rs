//! Session-state transition table and the error returned for
//! unrepresented transitions.
//!
//! The table itself is the spec's transition list for the Phase 1
//! subset; it lives here so the predicate stays adjacent to the error
//! type that surfaces violations to callers.

use super::SessionState;

/// Returned when the requested transition is not in the spec's
/// transition table for the Phase 1 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid session transition: {from:?} -> {to:?}")]
pub struct SessionTransitionError {
    /// State the session was in when the transition was attempted.
    pub from: SessionState,
    /// State the caller asked to move into.
    pub to: SessionState,
}

/// Returns whether the spec's transition table permits `from -> to`.
///
/// Some transitions land in the table because a rule's body requires
/// them even though the explicit transition list in
/// `session.allium:Session` doesn't enumerate them:
/// - `Authenticating -> LoggingOff` lets
///   `session.allium:VerifyPassword` end the session via its
///   `FinaliseLogoff` hand-off (Slice 11).
/// - `Identifying -> LoggingOff` and `Connecting -> LoggingOff` let
///   `session.allium:IdleTimeout` (Slice 17) and
///   `session.allium:CarrierLost` (Slice 18) end an
///   unauthenticated session via the
///   `FinaliseUnauthenticatedLogoff` rule, which is itself only
///   reachable through `LoggingOff`.
/// - `NewUserRegistering -> LoggingOff` lets the same idle / carrier
///   rules end an in-progress registration (Slice 19 brings the
///   state in; both rules' `requires:` lists already name it).
pub(super) fn is_session_transition_allowed(from: SessionState, to: SessionState) -> bool {
    use SessionState::{
        Authenticating, Connecting, Ended, Identifying, LoggingOff, Menu, NewUserRegistering,
        Onboarded,
    };
    matches!(
        (from, to),
        (Connecting, Identifying | LoggingOff | Ended)
            | (
                Identifying,
                Authenticating | NewUserRegistering | LoggingOff | Ended
            )
            | (Authenticating | NewUserRegistering, Onboarded)
            | (Authenticating | NewUserRegistering | LoggingOff, Ended)
            | (
                Authenticating | NewUserRegistering | Onboarded | Menu,
                LoggingOff
            )
            | (Onboarded, Menu)
    )
}
