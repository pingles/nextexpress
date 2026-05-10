//! Error types raised by [`Session`] state-machine transitions.
//!
//! The set is exhaustive for the Phase 1 surface — each transition
//! method on [`super::Session`] returns one of these. Once
//! [`crate::app::typed_session`] phase wrappers cover a transition,
//! the wrong-state variant becomes unreachable from the typed call
//! path; the variant stays for domain-level callers (and tests) that
//! drive `Session` directly.

use crate::domain::password::PasswordError;

use super::SessionState;

/// Errors returned by [`super::Session::accept_connection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AcceptConnectionError {
    /// The node already has a non-ended session bound to it.
    #[error("node already has an active session")]
    AlreadyActiveSession,
}

/// Errors returned by [`super::Session::apply_new_user_password_attempt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerifyNewUserPasswordError {
    /// The session is not in [`SessionState::NewUserRegistering`].
    #[error("apply_new_user_password_attempt in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// The gate has already passed for this session; the caller
    /// should stop prompting.
    #[error("new-user password gate already verified")]
    AlreadyVerified,
}

/// Errors returned by [`super::Session::name_typed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameTypedError {
    /// The session is not in [`SessionState::Identifying`].
    #[error("name typed in unexpected state: {0:?}")]
    WrongState(SessionState),
}

/// Errors returned by [`super::Session::verify_password`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyPasswordError {
    /// The session is not in [`SessionState::Authenticating`].
    #[error("verify_password in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("verify_password called without a bound user")]
    UserMissing,
    /// The hasher rejected the user's stored hash kind.
    #[error(transparent)]
    HashKindUnsupported(#[from] PasswordError),
}

/// Errors returned by [`super::Session::enter_menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EnterMenuError {
    /// The session is not in [`SessionState::Onboarded`].
    #[error("enter_menu in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("enter_menu called without a bound user")]
    UserMissing,
    /// The bound user has `force_password_reset` set; the listener
    /// must run the password-change sub-flow before retrying
    /// (`session.allium:CompletePasswordReset`, Slice 15).
    #[error("enter_menu blocked: user must complete a forced password reset")]
    PasswordResetPending,
}

/// Errors returned by [`super::Session::complete_new_user_registration`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CompleteNewUserRegistrationError {
    /// The session is not in [`SessionState::NewUserRegistering`].
    #[error("complete_new_user_registration in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// The new-user password gate (Slice 20a) has not yet passed.
    /// The spec rule's `requires:
    /// session.new_user_password_verified` precondition is not
    /// satisfied — the listener should run the gate first.
    #[error("complete_new_user_registration blocked: new-user password gate not verified")]
    GateNotVerified,
}

/// Errors returned by [`super::Session::initialise_daily_budget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InitialiseDailyBudgetError {
    /// The session is not in [`SessionState::Onboarded`].
    #[error("initialise_daily_budget in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("initialise_daily_budget called without a bound user")]
    UserMissing,
}

/// Errors returned by [`super::Session::force_password_reset_if_due`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ForcePasswordResetError {
    /// The session is not in [`SessionState::Onboarded`].
    #[error("force_password_reset_if_due in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("force_password_reset_if_due called without a bound user")]
    UserMissing,
}

/// Errors returned by [`super::Session::apply_password_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CompletePasswordResetError {
    /// The session is not in [`SessionState::Onboarded`].
    #[error("apply_password_change in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("apply_password_change called without a bound user")]
    UserMissing,
    /// The bound user does not have `force_password_reset` set, so
    /// `CompletePasswordReset` doesn't apply.
    #[error("apply_password_change called when force_password_reset is not set")]
    ResetNotPending,
}

/// Errors returned by [`super::Session::apply_idle_timeout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdleTimeoutError {
    /// The session is not in one of the spec-permitted states for
    /// [`super::Session::apply_idle_timeout`] (`identifying`,
    /// `authenticating`, `onboarded`, `menu`).
    #[error("apply_idle_timeout in unexpected state: {0:?}")]
    WrongState(SessionState),
}

/// Errors returned by [`super::Session::apply_carrier_loss`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CarrierLostError {
    /// The session is already in [`SessionState::LoggingOff`] or
    /// [`SessionState::Ended`], so `CarrierLost` is a no-op.
    #[error("apply_carrier_loss in unexpected state: {0:?}")]
    WrongState(SessionState),
}

/// Errors returned by [`super::Session::auto_rejoin_conference`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AutoRejoinError {
    /// The session is not in [`SessionState::Onboarded`] or
    /// [`SessionState::Menu`] — the spec's `JoinConference`
    /// `requires` clause.
    #[error("auto_rejoin_conference in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("auto_rejoin_conference called without a bound user")]
    UserMissing,
}

/// Errors returned by [`super::Session::tick_minute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TickMinuteError {
    /// The session is not in [`SessionState::Onboarded`] or
    /// [`SessionState::Menu`].
    #[error("tick_minute in unexpected state: {0:?}")]
    WrongState(SessionState),
    /// No user is bound to the session.
    #[error("tick_minute called without a bound user")]
    UserMissing,
}
