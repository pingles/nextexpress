//! Per-call payload types for the authenticated session phases.
//!
//! [`AuthenticatedCall`] groups the state that exists from successful
//! authentication until teardown. The authenticated phases carry it
//! whole and [`CallSalvage`] preserves it through `LoggingOff`/`Ended`,
//! so adding a per-call field is a single-site change instead of an
//! edit to every `SessionPhase` variant and salvage match.

use std::fmt;
use std::time::{Duration, SystemTime};

use crate::domain::user::User;

use super::SessionPhase;

/// Opaque, durable identity of one authenticated call.
///
/// Created by the application layer after successful authentication
/// (`designs/FILES.md`: the transfer ledger persists this value; node
/// numbers are never durable call identity). Like `now`, the value is
/// an input to the domain rules — entropy comes from the caller so the
/// rules stay deterministic under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallId(u128);

impl CallId {
    /// Wraps a caller-supplied 128-bit value (production callers pass
    /// freshly generated random bits).
    ///
    /// # Parameters
    /// - `value`: the identity bits; the domain never inspects them.
    ///
    /// # Returns
    /// The wrapped identifier.
    #[must_use]
    pub fn new(value: u128) -> Self {
        Self(value)
    }
}

impl fmt::Display for CallId {
    /// Renders the persistence TEXT form: 32 lowercase hex digits,
    /// zero-padded.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// State that exists from successful authentication to teardown.
///
/// Carried whole by `SessionPhase::Onboarded` and `SessionPhase::Menu`,
/// and salvaged into [`CallSalvage::Authenticated`] when the session
/// begins logging off — every transition moves the struct as one value.
#[derive(Debug, Clone)]
pub(super) struct AuthenticatedCall {
    /// The call's durable identity, stamped at authentication.
    pub(super) call_id: CallId,
    /// The authenticated user bound to this call.
    pub(super) user: User,
    /// When authentication completed.
    pub(super) authenticated_at: SystemTime,
    /// Per-call time budget: set by `initialise_daily_budget`,
    /// decremented by `tick_minute`.
    pub(super) time_remaining: Duration,
}

/// What the session still knows about its caller once teardown begins —
/// the payload `LoggingOff` and `Ended` retain.
///
/// Replaces the independently nullable `user`/`authenticated_at` option
/// pairs those phases used to carry, which could represent impossible
/// combinations (an authentication timestamp with no user). Only the
/// three reachable shapes are representable.
#[derive(Debug, Clone)]
pub(super) enum CallSalvage {
    /// Teardown before any user was identified.
    Unidentified,
    /// A user was identified (handle accepted) but the call never
    /// authenticated — e.g. lockout during password verification.
    Identified(User),
    /// Teardown of an authenticated call.
    Authenticated(AuthenticatedCall),
}

impl CallSalvage {
    /// Salvages whatever caller state `phase` carried.
    pub(super) fn from_phase(phase: SessionPhase) -> Self {
        match phase {
            SessionPhase::Connecting
            | SessionPhase::Identifying { .. }
            | SessionPhase::NewUserRegistering { .. } => Self::Unidentified,
            SessionPhase::Authenticating { user, .. } => Self::Identified(user),
            SessionPhase::Onboarded { call } | SessionPhase::Menu { call } => {
                Self::Authenticated(call)
            }
            SessionPhase::LoggingOff { call, .. } | SessionPhase::Ended { call, .. } => call,
        }
    }

    pub(super) fn user(&self) -> Option<&User> {
        match self {
            Self::Unidentified => None,
            Self::Identified(user) => Some(user),
            Self::Authenticated(call) => Some(&call.user),
        }
    }

    pub(super) fn user_mut(&mut self) -> Option<&mut User> {
        match self {
            Self::Unidentified => None,
            Self::Identified(user) => Some(user),
            Self::Authenticated(call) => Some(&mut call.user),
        }
    }

    /// When authentication completed, if this call ever authenticated.
    pub(super) fn authenticated_at(&self) -> Option<SystemTime> {
        match self {
            Self::Authenticated(call) => Some(call.authenticated_at),
            Self::Unidentified | Self::Identified(_) => None,
        }
    }

    /// Remaining per-call time; zero when the call never authenticated.
    pub(super) fn time_remaining(&self) -> Duration {
        match self {
            Self::Authenticated(call) => call.time_remaining,
            Self::Unidentified | Self::Identified(_) => Duration::ZERO,
        }
    }

    /// The call identity, if this call ever authenticated.
    pub(super) fn call_id(&self) -> Option<CallId> {
        match self {
            Self::Authenticated(call) => Some(call.call_id),
            Self::Unidentified | Self::Identified(_) => None,
        }
    }
}
