//! New-user registration gate and completion rules for [`Session`].

use std::time::{Duration, SystemTime};

use crate::domain::caller_log::CallerLog;
use crate::domain::user::User;

use super::{
    AuthenticatedCall, CallId, CompleteNewUserRegistrationError, LogoffReason, NameTypedError,
    NewUserPasswordOutcome, NewUserRequestOutcome, Session, SessionPhase, SessionPolicy,
    SessionState, VerifyNewUserPasswordError,
};

impl Session {
    /// Applies the `user_typed_NEW` branch of
    /// `session.allium:NameTyped`, plus the on-enter rules for the
    /// new state: `RejectDisallowedRegistration` (Slice 20a) and
    /// `InitialiseNewUserGate` (Slice 20a).
    ///
    /// # Parameters
    /// - `allow_new_users`: mirrors `core/config.allow_new_users`.
    ///   When `false`, the session moves on through
    ///   [`SessionState::NewUserRegistering`] and immediately into
    ///   [`SessionState::LoggingOff`] with
    ///   [`LogoffReason::NewUserRejected`].
    /// - `password_required`: mirrors `core/config.new_user_password
    ///   != null`. When `true`, the gate is armed and
    ///   [`Self::new_user_password_verified`] starts `false`; when
    ///   `false`, no gate runs and the flag starts `true`.
    /// - `_now`: timestamp of the rule firing. Retained for symmetry
    ///   with the application flow; logoff timestamps are recorded by
    ///   [`Session::finalise_logoff`].
    ///
    /// # Errors
    /// Returns [`NameTypedError::WrongState`] if the session is not in
    /// [`SessionState::Identifying`].
    pub fn record_new_user_request(
        &mut self,
        allow_new_users: bool,
        password_required: bool,
        _now: SystemTime,
    ) -> Result<NewUserRequestOutcome, NameTypedError> {
        if self.state() != SessionState::Identifying {
            return Err(NameTypedError::WrongState(self.state()));
        }
        if !allow_new_users {
            // RejectDisallowedRegistration.
            self.move_to_logging_off(Some(LogoffReason::NewUserRejected));
            return Ok(NewUserRequestOutcome::Rejected);
        }
        // InitialiseNewUserGate.
        self.phase = SessionPhase::NewUserRegistering {
            password_verified: !password_required,
            password_attempts: 0,
        };
        Ok(NewUserRequestOutcome::Initialised { password_required })
    }

    /// Applies `session.allium:VerifyNewUserPassword` (Slice 20a).
    ///
    /// `matches` is the result of comparing the user's typed candidate
    /// against `core/config.new_user_password`. The application layer
    /// owns that comparison so this method stays free of any
    /// presentation- or hash-storage decisions.
    ///
    /// On a match the session is marked verified. On a mismatch the
    /// attempt counter climbs and a "New-user password failure" caller
    /// log entry is emitted; once the counter reaches
    /// `max_attempts`, the session moves to
    /// [`SessionState::LoggingOff`] with
    /// [`LogoffReason::NewUserRejected`].
    ///
    /// # Errors
    /// Returns [`VerifyNewUserPasswordError::WrongState`] when the
    /// session is not in [`SessionState::NewUserRegistering`], or
    /// [`VerifyNewUserPasswordError::AlreadyVerified`] when the gate
    /// has already passed (the caller should stop prompting).
    pub fn apply_new_user_password_attempt(
        &mut self,
        matches: bool,
        max_attempts: u32,
        now: SystemTime,
    ) -> Result<(NewUserPasswordOutcome, Option<CallerLog>), VerifyNewUserPasswordError> {
        let SessionPhase::NewUserRegistering {
            password_verified,
            password_attempts,
        } = &mut self.phase
        else {
            return Err(VerifyNewUserPasswordError::WrongState(self.state()));
        };
        if *password_verified {
            return Err(VerifyNewUserPasswordError::AlreadyVerified);
        }
        if matches {
            *password_verified = true;
            return Ok((NewUserPasswordOutcome::Verified, None));
        }
        *password_attempts = (*password_attempts).saturating_add(1);
        let entry = CallerLog {
            session_node: self.shared.node_number,
            at: now,
            text: "New-user password failure".to_string(),
            is_password_failure: true,
        };
        if *password_attempts >= max_attempts {
            self.move_to_logging_off(Some(LogoffReason::NewUserRejected));
            Ok((NewUserPasswordOutcome::TooManyFailures, Some(entry)))
        } else {
            Ok((NewUserPasswordOutcome::Mismatch, Some(entry)))
        }
    }

    /// Applies `session.allium:CompleteNewUserRegistration`
    /// (Slice 20).
    ///
    /// Binds the freshly built `user`, sets `authenticated_at`, stamps
    /// the caller-supplied `call_id` as the call's durable identity,
    /// transitions [`SessionState::NewUserRegistering`] to
    /// [`SessionState::Onboarded`], then fires the
    /// `state becomes onboarded` rule cluster via
    /// [`Session::on_enter_onboarded`].
    ///
    /// # Returns
    /// An optional [`CallerLog`] entry produced by
    /// `RejectLockedOrInsufficientAccess` when it short-circuits the
    /// post-onboarded cluster. Practically this never fires for a
    /// freshly registered new user (`access_level = 2`,
    /// `account_locked = false`); the result type carries it for
    /// consistency with [`Session::apply_password_match`] and so
    /// future access-level configuration changes don't surprise the
    /// caller.
    ///
    /// # Errors
    /// Returns [`CompleteNewUserRegistrationError::WrongState`] when
    /// the session is not in [`SessionState::NewUserRegistering`], or
    /// [`CompleteNewUserRegistrationError::GateNotVerified`] when the
    /// new-user password gate (Slice 20a) has not yet passed — the
    /// spec rule's `requires:
    /// session.new_user_password_verified` precondition.
    pub fn complete_new_user_registration(
        &mut self,
        user: User,
        policy: SessionPolicy,
        now: SystemTime,
        call_id: CallId,
    ) -> Result<Option<CallerLog>, CompleteNewUserRegistrationError> {
        let password_verified = match &self.phase {
            SessionPhase::NewUserRegistering {
                password_verified, ..
            } => *password_verified,
            _ => return Err(CompleteNewUserRegistrationError::WrongState(self.state())),
        };
        if !password_verified {
            return Err(CompleteNewUserRegistrationError::GateNotVerified);
        }
        // The created user was just persisted verbatim by
        // `create_user`: baseline the persist diff before the
        // post-onboarded rules mutate the aggregate.
        self.persist_baseline = Some(Box::new(user.to_persisted()));
        self.phase = SessionPhase::Onboarded {
            call: AuthenticatedCall {
                call_id,
                user,
                authenticated_at: now,
                time_remaining: Duration::ZERO,
            },
        };
        Ok(self.on_enter_onboarded(policy, now))
    }
}
