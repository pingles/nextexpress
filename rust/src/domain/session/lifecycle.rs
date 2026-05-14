//! Session activity, logoff, and menu-entry transitions.

use std::time::SystemTime;

use crate::domain::caller_log::CallerLog;

use super::log_format::{format_logoff_line, format_logon_line};
use super::{
    budget, lockout, CarrierLostError, EnterMenuError, IdleTimeoutError, LogoffReason, Session,
    SessionPhase, SessionPolicy, SessionState, SessionTransitionError,
};

impl Session {
    /// Updates [`Self::last_input_at`] to `at`.
    ///
    /// The telnet adapter (and any other user-facing transport
    /// adapter) calls this on every input chunk so the
    /// `session.allium:IdleTimeout` rule (Slice 17) and the
    /// per-minute `UpdateTimeUsed` rule (Slice 14) have an
    /// up-to-date last-activity timestamp.
    pub fn record_input(&mut self, at: SystemTime) {
        self.shared.last_input_at = at;
    }

    /// `session.allium:CarrierLost` rule (Slice 18).
    ///
    /// Transitions the session to [`SessionState::LoggingOff`] with
    /// [`LogoffReason::CarrierLoss`]. The transport adapter calls
    /// this when the underlying connection has gone away (clean
    /// EOF, RST, modem CD drop, etc.). The rule is allowed from
    /// every pre-terminal state the spec lists for `CarrierLost`:
    /// `connecting`, `identifying`, `authenticating`,
    /// `new_user_registering`, `onboarded`, `menu`.
    ///
    /// # Errors
    /// Returns [`CarrierLostError::WrongState`] when the session is
    /// already [`SessionState::LoggingOff`] or
    /// [`SessionState::Ended`].
    pub fn apply_carrier_loss(&mut self) -> Result<(), CarrierLostError> {
        if !matches!(
            self.state(),
            SessionState::Connecting
                | SessionState::Identifying
                | SessionState::Authenticating
                | SessionState::NewUserRegistering
                | SessionState::Onboarded
                | SessionState::Menu
        ) {
            return Err(CarrierLostError::WrongState(self.state()));
        }
        self.move_to_logging_off(Some(LogoffReason::CarrierLoss));
        Ok(())
    }

    /// `session.allium:IdleTimeout` rule (Slice 17).
    ///
    /// Transitions the session to [`SessionState::LoggingOff`] with
    /// [`LogoffReason::InputTimeout`] (when `treat_as_logoff` is
    /// `true`) or [`LogoffReason::CarrierLoss`] (otherwise). The
    /// caller — typically the telnet adapter, which owns the read
    /// timer — is responsible for deciding the timeout has elapsed
    /// and invoking this method.
    ///
    /// # Errors
    /// Returns [`IdleTimeoutError::WrongState`] when the session is
    /// not in one of the spec-permitted states (`identifying`,
    /// `authenticating`, `new_user_registering`, `onboarded`, or
    /// `menu`).
    pub fn apply_idle_timeout(&mut self, treat_as_logoff: bool) -> Result<(), IdleTimeoutError> {
        if !matches!(
            self.state(),
            SessionState::Identifying
                | SessionState::Authenticating
                | SessionState::NewUserRegistering
                | SessionState::Onboarded
                | SessionState::Menu
        ) {
            return Err(IdleTimeoutError::WrongState(self.state()));
        }
        self.move_to_logging_off(Some(if treat_as_logoff {
            LogoffReason::InputTimeout
        } else {
            LogoffReason::CarrierLoss
        }));
        Ok(())
    }

    /// `session.allium:UserRequestsLogoff` rule.
    ///
    /// Transitions [`SessionState::Onboarded`] or
    /// [`SessionState::Menu`] to [`SessionState::LoggingOff`] and
    /// records [`LogoffReason::NormalLogoff`].
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the session is not in
    /// `onboarded` or `menu` — the spec's `requires` for this rule.
    /// The state guard is explicit (rather than relying on the
    /// transition table alone) because the table allows other
    /// states to reach `logging_off` for unrelated reasons
    /// (idle / carrier loss in Slices 17/18).
    pub fn user_requests_logoff(&mut self) -> Result<(), SessionTransitionError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(SessionTransitionError {
                from: self.state(),
                to: SessionState::LoggingOff,
            });
        }
        self.move_to_logging_off(Some(LogoffReason::NormalLogoff));
        Ok(())
    }

    /// `session.allium:FinaliseLogoff` rule.
    ///
    /// Updates `user.last_call`, appends the goodbye line to the
    /// caller log, transitions to [`SessionState::Ended`] and records
    /// `logoff_at`.
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the session is not in
    /// [`SessionState::LoggingOff`].
    pub fn finalise_logoff(
        &mut self,
        now: SystemTime,
    ) -> Result<CallerLog, SessionTransitionError> {
        if self.state() != SessionState::LoggingOff {
            return Err(SessionTransitionError {
                from: self.state(),
                to: SessionState::Ended,
            });
        }
        if let Some(user) = self.phase.user_mut() {
            user.record_last_call(now);
        }
        let line = format_logoff_line(self);
        let entry = CallerLog {
            session_node: self.shared.node_number,
            at: now,
            text: line,
            is_password_failure: false,
        };
        self.move_to_ended(Some(now));
        Ok(entry)
    }

    /// `session.allium:EnterMenu` rule.
    ///
    /// Bumps `user.times_called`, transitions
    /// [`SessionState::Onboarded`] -> [`SessionState::Menu`] and
    /// appends a logon line to the caller log.
    ///
    /// # Errors
    /// Returns [`EnterMenuError::WrongState`] when not in
    /// [`SessionState::Onboarded`],
    /// [`EnterMenuError::PasswordResetPending`] when the bound user
    /// has `force_password_reset` set (Slice 15).
    pub fn enter_menu(&mut self, now: SystemTime) -> Result<CallerLog, EnterMenuError> {
        let SessionPhase::Onboarded { user, .. } = &mut self.phase else {
            return Err(EnterMenuError::WrongState(self.state()));
        };
        if user.force_password_reset() {
            return Err(EnterMenuError::PasswordResetPending);
        }
        user.bump_times_called();
        let previous = std::mem::replace(&mut self.phase, SessionPhase::Connecting);
        let SessionPhase::Onboarded {
            user,
            authenticated_at,
            time_remaining,
        } = previous
        else {
            unreachable!("phase checked above");
        };
        self.phase = SessionPhase::Menu {
            user,
            authenticated_at,
            time_remaining,
        };
        let line = format_logon_line(self);
        Ok(CallerLog {
            session_node: self.shared.node_number,
            at: now,
            text: line,
            is_password_failure: false,
        })
    }

    /// Fires every spec rule whose `when` clause is the transition
    /// into [`SessionState::Onboarded`].
    ///
    /// Called by every code path that drives a session into
    /// `Onboarded`: [`Session::apply_password_match`] and
    /// [`Session::complete_new_user_registration`] today; later,
    /// sysop direct logon (Slice 22) and local logon (Slice 23).
    /// Rules fire in spec order:
    ///
    /// 1. `session.allium:RejectLockedOrInsufficientAccess` (Slice 16)
    ///    — short-circuits the cluster by transitioning the session
    ///    to [`SessionState::LoggingOff`] when the bound user is
    ///    locked or below the minimum access tier. Returns the
    ///    rejection caller-log entry so the caller can append it.
    /// 2. `session.allium:InitialiseDailyBudget` (Slice 14).
    /// 3. `session.allium:ForcePasswordReset` (Slice 15).
    ///
    /// # Returns
    /// `Some(entry)` when rule 1 fired, otherwise `None`. The caller
    /// uses the presence of an entry as the signal to append it to
    /// the caller log.
    ///
    /// # Panics
    /// Panics if called outside [`SessionState::Onboarded`] or with no
    /// user bound — both invariants the caller is required to have
    /// just established by the transition. The guard violations are
    /// programmer errors, not runtime failures.
    pub(super) fn on_enter_onboarded(
        &mut self,
        policy: SessionPolicy,
        now: SystemTime,
    ) -> Option<CallerLog> {
        assert_eq!(
            self.state(),
            SessionState::Onboarded,
            "on_enter_onboarded called outside Onboarded state"
        );
        assert!(
            self.user().is_some(),
            "on_enter_onboarded called without a bound user"
        );
        if let Some(entry) = self.reject_locked_or_insufficient_access(now) {
            return Some(entry);
        }
        budget::initialise_daily_budget(self, now, policy.daily_reset_offset())
            .expect("guards hold immediately after transition to Onboarded");
        lockout::force_password_reset_if_due(self, policy.password_expiry_days(), now)
            .expect("guards hold immediately after transition to Onboarded");
        None
    }

    /// `session.allium:RejectLockedOrInsufficientAccess` rule
    /// (Slice 16).
    ///
    /// When the bound user is locked out (`account_locked` or
    /// `access_level` <= 1), transitions the session to
    /// [`SessionState::LoggingOff`] with the appropriate
    /// [`LogoffReason`] and returns the spec's rejection caller-log
    /// entry. Otherwise returns `None`.
    ///
    /// # Returns
    /// `Some(CallerLog)` when the rule fires (the caller is
    /// responsible for appending the entry); `None` when the user is
    /// allowed through.
    ///
    /// # Panics
    /// Panics if the session is not in [`SessionState::Onboarded`] or
    /// no user is bound — `on_enter_onboarded` is the canonical
    /// caller and establishes both invariants before invocation.
    pub(super) fn reject_locked_or_insufficient_access(
        &mut self,
        now: SystemTime,
    ) -> Option<CallerLog> {
        assert_eq!(
            self.state(),
            SessionState::Onboarded,
            "reject_locked_or_insufficient_access called outside Onboarded"
        );
        let user = self
            .user()
            .expect("reject_locked_or_insufficient_access without bound user");
        if !user.is_locked_out() {
            return None;
        }
        let reason = if user.is_account_locked() {
            LogoffReason::LockedAccount
        } else {
            LogoffReason::NewUserRejected
        };
        self.move_to_logging_off(Some(reason));
        Some(CallerLog {
            session_node: self.shared.node_number,
            at: now,
            text: "Logon rejected: account locked or below access threshold".to_string(),
            is_password_failure: false,
        })
    }
}
