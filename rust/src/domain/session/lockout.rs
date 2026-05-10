//! Lockout / password-failure policy applied to a [`Session`].
//!
//! Free functions (rather than methods on [`Session`]) so the
//! domain entity stops growing as more policies land. Each function
//! takes `&mut Session` and runs the spec rule cluster:
//!
//! - [`apply_password_match`] handles the matching branch of
//!   `session.allium:VerifyPassword` and runs the post-onboarded
//!   cluster (Slice 11, 14, 15, 16).
//! - [`apply_password_mismatch`] handles the non-matching branch and
//!   the lockout / excessive-failure escalation (Slice 11, 16).
//! - [`force_password_reset_if_due`] is the standalone Slice 15 rule
//!   that fires from the post-onboarded cluster.
//! - [`apply_password_change`] applies the credential update spec'd
//!   by `session.allium:CompletePasswordReset` (Slice 15).

use std::time::{Duration, SystemTime};

use crate::domain::caller_log::CallerLog;
use crate::domain::password::PasswordHashKind;

use super::{
    CompletePasswordResetError, ForcePasswordResetError, LogoffReason, PasswordFailureDecision,
    Session, SessionPhase, SessionPolicy, VerifyPasswordError, VerifyPasswordOutcome,
};

/// Applies the matching branch of `session.allium:VerifyPassword`.
///
/// Clears `user.invalid_attempts`, sets `authenticated_at`, and
/// transitions to [`SessionState::Onboarded`], then fires the
/// `state becomes onboarded` rule cluster.
///
/// # Returns
/// A tuple of:
/// - the [`VerifyPasswordOutcome`] — `Authenticated` on the normal
///   path, or `LogonRejected` when
///   `session.allium:RejectLockedOrInsufficientAccess` (Slice 16)
///   short-circuited the post-auth cluster;
/// - an optional [`CallerLog`] entry the rejection rule emits.
///   The caller is responsible for appending this to the log.
///
/// # Errors
/// Returns [`VerifyPasswordError::WrongState`] if the session is
/// not in [`SessionState::Authenticating`].
pub fn apply_password_match(
    session: &mut Session,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<(VerifyPasswordOutcome, Option<CallerLog>), VerifyPasswordError> {
    let SessionPhase::Authenticating { user, .. } = &mut session.phase else {
        return Err(VerifyPasswordError::WrongState(session.state()));
    };
    user.clear_invalid_attempts();
    let previous = std::mem::replace(&mut session.phase, SessionPhase::Connecting);
    let SessionPhase::Authenticating { user, .. } = previous else {
        unreachable!("phase checked above");
    };
    session.phase = SessionPhase::Onboarded {
        user,
        authenticated_at: now,
        time_remaining: Duration::ZERO,
    };
    let rejection = session.on_enter_onboarded(policy, now);
    let outcome = if rejection.is_some() {
        VerifyPasswordOutcome::LogonRejected
    } else {
        VerifyPasswordOutcome::Authenticated
    };
    Ok((outcome, rejection))
}

/// Applies the non-matching branch of `session.allium:VerifyPassword`.
///
/// Increments `user.invalid_attempts` and `password_retry_count`,
/// returns the caller-log "Password failure" entry, and may move the
/// session to [`SessionState::LoggingOff`] when the
/// [`SessionPolicy`] failure limit is reached.
///
/// # Errors
/// Returns [`VerifyPasswordError::WrongState`] if the session is
/// not in [`SessionState::Authenticating`].
pub fn apply_password_mismatch(
    session: &mut Session,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<(VerifyPasswordOutcome, CallerLog), VerifyPasswordError> {
    let SessionPhase::Authenticating {
        user,
        password_retry_count,
        ..
    } = &mut session.phase
    else {
        return Err(VerifyPasswordError::WrongState(session.state()));
    };
    user.bump_invalid_attempts();
    *password_retry_count = (*password_retry_count).saturating_add(1);

    let entry = CallerLog {
        session_node: session.shared.node_number,
        at: now,
        text: "Password failure".to_string(),
        is_password_failure: true,
    };

    let outcome = match policy.password_failure_decision(session) {
        PasswordFailureDecision::LockAccount => {
            let SessionPhase::Authenticating { user, .. } = &mut session.phase else {
                unreachable!("phase checked before password failure decision");
            };
            user.lock_account();
            session.move_to_logging_off(Some(LogoffReason::LockedAccount));
            VerifyPasswordOutcome::AccountLocked
        }
        PasswordFailureDecision::EndSession => {
            session.move_to_logging_off(Some(LogoffReason::ExcessivePasswordFails));
            VerifyPasswordOutcome::TooManyFailures
        }
        PasswordFailureDecision::Continue => VerifyPasswordOutcome::NotMatching,
    };
    Ok((outcome, entry))
}

/// `session.allium:ForcePasswordReset` rule (Slice 15).
///
/// Sets `user.force_password_reset` when `password_expiry_days >
/// 0` and the elapsed time since `password_last_updated` exceeds
/// that many days, **or** when the sysop has already set the flag
/// on the user. The rule is a no-op for locked accounts (per the
/// spec's `requires: not user.account_locked`).
///
/// # Errors
/// Returns [`ForcePasswordResetError::WrongState`] when the
/// session is not in [`SessionState::Onboarded`].
pub fn force_password_reset_if_due(
    session: &mut Session,
    password_expiry_days: u32,
    now: SystemTime,
) -> Result<(), ForcePasswordResetError> {
    let SessionPhase::Onboarded { user, .. } = &mut session.phase else {
        return Err(ForcePasswordResetError::WrongState(session.state()));
    };
    if user.is_account_locked() {
        return Ok(());
    }
    let already_flagged = user.force_password_reset();
    let expired = password_expiry_days > 0
        && now
            .duration_since(user.password_last_updated())
            .map(|d| d > Duration::from_secs(u64::from(password_expiry_days) * 86_400))
            .unwrap_or(false);
    if expired || already_flagged {
        user.set_force_password_reset(true);
    }
    Ok(())
}

/// Applies `session.allium:CompletePasswordReset` to the bound
/// user (Slice 15).
///
/// Replaces the user's stored credentials with the freshly computed
/// `(hash, salt, kind)` triple, sets `password_last_updated = now`,
/// and clears `force_password_reset`. The strength check and the
/// "differs-from-old" check are the caller's responsibility (see
/// `app::session_flow::complete_password_reset`).
///
/// # Errors
/// Returns [`CompletePasswordResetError::WrongState`] when the
/// session is not in [`SessionState::Onboarded`], or
/// [`CompletePasswordResetError::ResetNotPending`] when the bound
/// user does not have `force_password_reset` set.
pub fn apply_password_change(
    session: &mut Session,
    hash: String,
    salt: Option<String>,
    kind: PasswordHashKind,
    now: SystemTime,
) -> Result<(), CompletePasswordResetError> {
    let SessionPhase::Onboarded { user, .. } = &mut session.phase else {
        return Err(CompletePasswordResetError::WrongState(session.state()));
    };
    if !user.force_password_reset() {
        return Err(CompletePasswordResetError::ResetNotPending);
    }
    user.record_password_change(hash, salt, kind, now);
    Ok(())
}
