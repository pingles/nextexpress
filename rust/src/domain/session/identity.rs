//! Name-prompt and handle-resolution transitions for [`Session`].

use std::time::SystemTime;

use crate::domain::user::User;

use super::{
    AuthenticatingAttempt, CallSalvage, LogoffReason, NameTypedError, NameTypedOutcome, Session,
    SessionPhase, SessionState, SessionTransitionError,
};

/// Maximum number of unknown handle entries before a session is ended.
const MAX_NAME_RETRIES: u32 = 5;

impl Session {
    /// `session.allium:PromptForName` rule.
    ///
    /// Transitions the session from [`SessionState::Connecting`] to
    /// [`SessionState::Identifying`], indicating the banner is done
    /// and the listener is about to prompt for the user's handle.
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the session is not in
    /// [`SessionState::Connecting`].
    pub fn prompt_for_name(&mut self) -> Result<(), SessionTransitionError> {
        let from = self.state();
        if from != SessionState::Connecting {
            return Err(SessionTransitionError {
                from,
                to: SessionState::Identifying,
            });
        }
        self.phase = SessionPhase::Identifying {
            name_retry_count: 0,
        };
        Ok(())
    }

    /// Applies the successful branch of `session.allium:NameTyped`.
    ///
    /// The caller has already resolved `typed` to `user` through a
    /// repository. This method stores both on the session and moves it
    /// to [`SessionState::Authenticating`].
    ///
    /// # Errors
    /// Returns [`NameTypedError::WrongState`] if the session is not in
    /// [`SessionState::Identifying`].
    pub fn record_identified_user(
        &mut self,
        typed: &str,
        user: User,
    ) -> Result<NameTypedOutcome, NameTypedError> {
        if self.state() != SessionState::Identifying {
            return Err(NameTypedError::WrongState(self.state()));
        }
        // The freshly loaded user IS the stored state: baseline the
        // persist diff here so `pending_user_patch` carries only
        // changes made after the bind.
        self.persist_baseline = Some(Box::new(user.to_persisted()));
        self.phase = SessionPhase::Authenticating {
            attempt: AuthenticatingAttempt {
                typed_name: typed.to_string(),
                user,
                password_retry_count: 0,
            },
        };
        Ok(NameTypedOutcome::Authenticated)
    }

    /// Applies the unknown-handle branch of `session.allium:NameTyped`.
    ///
    /// Increments [`Self::name_retry_count`]. After five strikes, the
    /// session ends with [`LogoffReason::NewUserRejected`].
    ///
    /// # Errors
    /// Returns [`NameTypedError::WrongState`] if the session is not in
    /// [`SessionState::Identifying`].
    pub fn record_unknown_name(
        &mut self,
        now: SystemTime,
    ) -> Result<NameTypedOutcome, NameTypedError> {
        let SessionPhase::Identifying { name_retry_count } = &mut self.phase else {
            return Err(NameTypedError::WrongState(self.state()));
        };
        *name_retry_count += 1;
        if *name_retry_count >= MAX_NAME_RETRIES {
            self.phase = SessionPhase::Ended {
                call: CallSalvage::Unidentified,
                reason: Some(LogoffReason::NewUserRejected),
                logoff_at: Some(now),
            };
            Ok(NameTypedOutcome::SessionEnded)
        } else {
            Ok(NameTypedOutcome::NotFound)
        }
    }
}
