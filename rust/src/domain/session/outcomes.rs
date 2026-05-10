//! Outcome enums returned by [`super::Session`] state-machine
//! transitions.
//!
//! Outcomes are pure data — what happened — separate from the
//! transition itself. The driver uses them to choose what to render
//! to the wire and which next-phase wrapper to construct.

/// Outcome of [`super::Session::name_typed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTypedOutcome {
    /// User found; session has moved to authenticating and is ready
    /// for [`super::Session::user`] to drive the password prompt.
    Authenticated,
    /// Handle did not match any user. The retry counter has been
    /// incremented. The listener should re-prompt.
    NotFound,
    /// Five not-found strikes in a row. The session has ended with
    /// [`super::LogoffReason::NewUserRejected`].
    SessionEnded,
    /// The literal `NEW` was typed and registration is permitted. The
    /// session is in [`super::SessionState::NewUserRegistering`]; the
    /// listener now drives the registration sub-flow.
    /// `password_required` is `true` when the new-user password gate
    /// (Slice 20a) is armed and must pass before
    /// [`super::Session::complete_new_user_registration`] will accept
    /// the form.
    NewUserRegistering {
        /// `true` when the gate must pass before registration.
        password_required: bool,
    },
    /// The literal `NEW` was typed but registration is disabled
    /// (`core/config.allow_new_users = false`). The session has
    /// already moved to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`] via
    /// `RejectDisallowedRegistration`; the caller should run
    /// `FinaliseLogoff`.
    NewUserRegistrationDisallowed,
}

/// Outcome of [`super::Session::record_new_user_request`]. Mirrors
/// the spec's `becomes new_user_registering` cluster: the transition
/// either initialises the gate (Initialised) or is short-circuited by
/// `RejectDisallowedRegistration` (Rejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewUserRequestOutcome {
    /// `core/config.allow_new_users = true`. The session is in
    /// [`super::SessionState::NewUserRegistering`] with gate state
    /// initialised. `password_required` is `true` when the
    /// `new_user_password` gate must pass before completion.
    Initialised {
        /// Mirrors `core/config.new_user_password != null`.
        password_required: bool,
    },
    /// `core/config.allow_new_users = false`. The session has
    /// transitioned to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`].
    Rejected,
}

/// Outcome of [`super::Session::apply_new_user_password_attempt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewUserPasswordOutcome {
    /// The candidate matched. The gate is now verified and
    /// [`super::Session::complete_new_user_registration`] may proceed
    /// once the registration form is collected.
    Verified,
    /// The candidate did not match. The attempt counter has been
    /// incremented and a caller-log entry returned. The listener
    /// should re-prompt.
    Mismatch,
    /// The attempt counter has reached
    /// `core/config.max_new_user_password_attempts`. The session has
    /// moved to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`].
    TooManyFailures,
}

/// Outcome of [`super::Session::verify_password`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyPasswordOutcome {
    /// Credentials match. The session has moved to
    /// [`super::SessionState::Onboarded`], `authenticated_at` is set,
    /// and `user.invalid_attempts` is cleared.
    Authenticated,
    /// Credentials do not match. The session stays in
    /// [`super::SessionState::Authenticating`]; the listener should
    /// re-prompt.
    NotMatching,
    /// `user.invalid_attempts` reached `max_password_failures`. The
    /// account is now locked, the session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::LockedAccount`].
    AccountLocked,
    /// `password_retry_count` reached `max_password_failures` for
    /// this session. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::ExcessivePasswordFails`].
    TooManyFailures,
    /// Credentials matched, but
    /// `session.allium:RejectLockedOrInsufficientAccess` (Slice 16)
    /// short-circuited the post-auth rule cluster: the user's
    /// account was already locked or below the minimum access tier.
    /// The session has moved to [`super::SessionState::LoggingOff`]
    /// with [`super::LogoffReason::LockedAccount`] or
    /// [`super::LogoffReason::NewUserRejected`].
    LogonRejected,
}

/// Outcome of [`super::Session::tick_minute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickMinuteOutcome {
    /// The session has time left and remains in `onboarded` or `menu`.
    Continued,
    /// `time_remaining` has reached zero. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::OutOfTime`].
    TimeExpired,
}
