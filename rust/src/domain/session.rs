//! [`Session`] entity (spec: `session.allium:Session`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop reads.
//! Presentation booleans, time accounting, temp access, reserved-for
//! and the `new_user_registering` branch arrive in their owning
//! slices.

use std::time::SystemTime;

use crate::domain::caller_log::CallerLog;
use crate::domain::password::PasswordError;
use crate::domain::user::User;

/// Maximum number of unknown handle entries before a session is ended.
const MAX_NAME_RETRIES: u32 = 5;

/// How the user reached the BBS (spec: `session.allium:LogonChannel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogonChannel {
    /// Sysop pressing F1/F2 at the BBS console.
    SysopConsole,
    /// Local logon key, no carrier.
    Local,
    /// Normal user, via telnet or modem.
    Remote,
    /// File-transfer-only logon over FTP.
    Ftp,
}

/// Why a session is logging off (spec: `session.allium:LogoffReason`).
///
/// Phase 1 introduces the variants its slices need: `NewUserRejected`
/// in Slice 9, `ExcessivePasswordFails` and `LockedAccount` in
/// Slice 11, `NormalLogoff` in Slice 13. The remaining variants land
/// with their owning slices in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogoffReason {
    /// Five name-not-found strikes in a row, or the new-user
    /// registration was refused.
    NewUserRejected,
    /// Too many bad passwords on this session.
    ExcessivePasswordFails,
    /// The user's account has been locked (too many bad passwords
    /// across sessions).
    LockedAccount,
    /// User typed `G` (or the configured logoff command).
    NormalLogoff,
}

/// Lifecycle state of a [`Session`] (spec: `session.allium:Session.state`).
///
/// Phase 1 omits `new_user_registering`; that branch lands in Slice 19.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Connection accepted, banner not yet displayed.
    Connecting,
    /// Prompting the user for their handle.
    Identifying,
    /// Verifying a typed password.
    Authenticating,
    /// Authenticated; on-logon screens running.
    Onboarded,
    /// At the conference menu.
    Menu,
    /// Tearing down; about to write the goodbye line.
    LoggingOff,
    /// Terminal state; the node is being released.
    Ended,
}

/// A single in-progress or completed visit to the BBS.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    node_number: u32,
    channel: LogonChannel,
    state: SessionState,
    user: Option<User>,
    typed_name: Option<String>,
    name_retry_count: u32,
    password_retry_count: u32,
    connected_at: SystemTime,
    last_input_at: SystemTime,
    online_baud: u32,
    authenticated_at: Option<SystemTime>,
    logoff_at: Option<SystemTime>,
    logoff_reason: Option<LogoffReason>,
}

impl Session {
    /// Constructs a new session in [`SessionState::Connecting`].
    ///
    /// # Parameters
    /// - `node_number`: the [`crate::domain::node::Node`] this session
    ///   is bound to (1-based).
    /// - `channel`: how the user reached the BBS.
    /// - `online_baud`: connection baud (0 for local sessions).
    /// - `connected_at`: timestamp the transport accepted the
    ///   connection. Also used as the initial `last_input_at`.
    pub fn new(
        node_number: u32,
        channel: LogonChannel,
        online_baud: u32,
        connected_at: SystemTime,
    ) -> Self {
        Self {
            node_number,
            channel,
            state: SessionState::Connecting,
            user: None,
            typed_name: None,
            name_retry_count: 0,
            password_retry_count: 0,
            connected_at,
            last_input_at: connected_at,
            online_baud,
            authenticated_at: None,
            logoff_at: None,
            logoff_reason: None,
        }
    }

    /// Returns this session's node number.
    pub fn node_number(&self) -> u32 {
        self.node_number
    }

    /// Returns the channel the session was opened on.
    pub fn channel(&self) -> LogonChannel {
        self.channel
    }

    /// Returns the current lifecycle state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Returns the user this session has identified as, if any.
    pub fn user(&self) -> Option<&User> {
        self.user.as_ref()
    }

    /// Returns the handle the user typed at the identify prompt, if any.
    pub fn typed_name(&self) -> Option<&str> {
        self.typed_name.as_deref()
    }

    /// Returns the number of name-not-found strikes accumulated on this
    /// session.
    pub fn name_retry_count(&self) -> u32 {
        self.name_retry_count
    }

    /// Returns the number of bad-password strikes accumulated on this
    /// session.
    pub fn password_retry_count(&self) -> u32 {
        self.password_retry_count
    }

    /// Returns the timestamp the connection was accepted.
    pub fn connected_at(&self) -> SystemTime {
        self.connected_at
    }

    /// Returns the timestamp of the last input received from the user.
    pub fn last_input_at(&self) -> SystemTime {
        self.last_input_at
    }

    /// Returns the connection baud rate (0 for local sessions).
    pub fn online_baud(&self) -> u32 {
        self.online_baud
    }

    /// Returns the timestamp at which authentication completed, if it
    /// has.
    pub fn authenticated_at(&self) -> Option<SystemTime> {
        self.authenticated_at
    }

    /// Returns the timestamp the session ended, if it has.
    pub fn logoff_at(&self) -> Option<SystemTime> {
        self.logoff_at
    }

    /// Returns the reason recorded for the session ending, if any.
    pub fn logoff_reason(&self) -> Option<LogoffReason> {
        self.logoff_reason
    }

    /// Spec-derived predicate: `channel in {remote, ftp}`.
    pub fn is_remote(&self) -> bool {
        matches!(self.channel, LogonChannel::Remote | LogonChannel::Ftp)
    }

    /// Spec-derived predicate:
    /// `state in {onboarded, menu, logging_off, ended} and user != null`.
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
            && matches!(
                self.state,
                SessionState::Onboarded
                    | SessionState::Menu
                    | SessionState::LoggingOff
                    | SessionState::Ended
            )
    }

    /// Returns `true` when the session has not yet ended (i.e. its
    /// state is anything except [`SessionState::Ended`]). Helper for
    /// the `OneActiveSessionPerNode` invariant: an active session is
    /// one whose state is not the terminal `Ended`.
    pub fn is_active(&self) -> bool {
        self.state != SessionState::Ended
    }

    /// Attempts to transition the session to `target`.
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the spec does not permit
    /// the transition (Phase 1 subset of `session.allium:Session.state`).
    fn transition_to(&mut self, target: SessionState) -> Result<(), SessionTransitionError> {
        if !is_session_transition_allowed(self.state, target) {
            return Err(SessionTransitionError {
                from: self.state,
                to: target,
            });
        }
        self.state = target;
        Ok(())
    }

    /// `session.allium:AcceptConnection` rule.
    ///
    /// Creates a fresh [`Session`] for `node_number`. Rejects when
    /// `existing_session_for_node` already holds an active session for
    /// that node — the spec's `OneActiveSessionPerNode` invariant. The
    /// caller (typically the supervisor on top of
    /// [`crate::app::node_pool::NodePool`]) is responsible for
    /// ensuring the underlying node is in
    /// [`crate::domain::node::NodeStatus::Connecting`] before
    /// invoking this rule (the pool's `allocate` does that
    /// atomically).
    ///
    /// # Errors
    /// Returns [`AcceptConnectionError::AlreadyActiveSession`] if
    /// `existing_session_for_node` is `Some` and that session has not
    /// reached [`SessionState::Ended`].
    pub fn accept_connection(
        node_number: u32,
        channel: LogonChannel,
        online_baud: u32,
        connected_at: SystemTime,
        existing_session_for_node: Option<&Session>,
    ) -> Result<Self, AcceptConnectionError> {
        if existing_session_for_node.is_some_and(|s| s.is_active()) {
            return Err(AcceptConnectionError::AlreadyActiveSession);
        }
        Ok(Self::new(node_number, channel, online_baud, connected_at))
    }

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
        self.transition_to(SessionState::Identifying)
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
        if self.state != SessionState::Identifying {
            return Err(NameTypedError::WrongState(self.state));
        }
        self.typed_name = Some(typed.to_string());
        self.user = Some(user);
        self.transition_to(SessionState::Authenticating)
            .expect("identifying -> authenticating is permitted");
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
        if self.state != SessionState::Identifying {
            return Err(NameTypedError::WrongState(self.state));
        }
        self.name_retry_count += 1;
        if self.name_retry_count >= MAX_NAME_RETRIES {
            self.transition_to(SessionState::Ended)
                .expect("identifying -> ended is permitted");
            self.logoff_reason = Some(LogoffReason::NewUserRejected);
            self.logoff_at = Some(now);
            Ok(NameTypedOutcome::SessionEnded)
        } else {
            Ok(NameTypedOutcome::NotFound)
        }
    }

    /// Applies the Phase 1 `NEW` branch of `session.allium:NameTyped`.
    ///
    /// Slice 19 wires this up to the registration flow. Until then, the
    /// session stays in [`SessionState::Identifying`] and the caller can
    /// present a rejection/retry prompt.
    ///
    /// # Errors
    /// Returns [`NameTypedError::WrongState`] if the session is not in
    /// [`SessionState::Identifying`].
    pub fn reject_new_user_request(&self) -> Result<NameTypedOutcome, NameTypedError> {
        if self.state != SessionState::Identifying {
            return Err(NameTypedError::WrongState(self.state));
        }
        Ok(NameTypedOutcome::NewUserRejected)
    }

    /// `session.allium:UserRequestsLogoff` rule.
    ///
    /// Transitions [`SessionState::Onboarded`] or
    /// [`SessionState::Menu`] to [`SessionState::LoggingOff`] and
    /// records [`LogoffReason::NormalLogoff`].
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the session is not in
    /// `onboarded` or `menu`.
    pub fn user_requests_logoff(&mut self) -> Result<(), SessionTransitionError> {
        self.transition_to(SessionState::LoggingOff)?;
        self.logoff_reason = Some(LogoffReason::NormalLogoff);
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
        if self.state != SessionState::LoggingOff {
            return Err(SessionTransitionError {
                from: self.state,
                to: SessionState::Ended,
            });
        }
        if let Some(user) = self.user.as_mut() {
            user.record_last_call(now);
        }
        let line = format_logoff_line(self);
        let entry = CallerLog {
            session_node: self.node_number,
            at: now,
            text: line,
            is_password_failure: false,
        };
        self.transition_to(SessionState::Ended)
            .expect("logging_off -> ended is permitted");
        self.logoff_at = Some(now);
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
    /// [`SessionState::Onboarded`] or
    /// [`EnterMenuError::UserMissing`] when no user is bound.
    pub fn enter_menu(&mut self, now: SystemTime) -> Result<CallerLog, EnterMenuError> {
        if self.state != SessionState::Onboarded {
            return Err(EnterMenuError::WrongState(self.state));
        }
        if self.user.is_none() {
            return Err(EnterMenuError::UserMissing);
        }
        self.user
            .as_mut()
            .expect("user present")
            .bump_times_called();
        self.transition_to(SessionState::Menu)
            .expect("onboarded -> menu is permitted");
        let line = format_logon_line(self);
        Ok(CallerLog {
            session_node: self.node_number,
            at: now,
            text: line,
            is_password_failure: false,
        })
    }

    /// Applies the matching branch of `session.allium:VerifyPassword`.
    ///
    /// Clears `user.invalid_attempts`, sets `authenticated_at`, and
    /// transitions to [`SessionState::Onboarded`].
    ///
    /// # Errors
    /// Returns [`VerifyPasswordError::WrongState`] if the session is
    /// not in [`SessionState::Authenticating`], or
    /// [`VerifyPasswordError::UserMissing`] if no user is bound.
    pub fn apply_password_match(
        &mut self,
        now: SystemTime,
    ) -> Result<VerifyPasswordOutcome, VerifyPasswordError> {
        if self.state != SessionState::Authenticating {
            return Err(VerifyPasswordError::WrongState(self.state));
        }
        let user_mut = self.user.as_mut().ok_or(VerifyPasswordError::UserMissing)?;
        user_mut.clear_invalid_attempts();
        self.authenticated_at = Some(now);
        self.transition_to(SessionState::Onboarded)
            .expect("authenticating -> onboarded is permitted");
        Ok(VerifyPasswordOutcome::Authenticated)
    }

    /// Applies the non-matching branch of `session.allium:VerifyPassword`.
    ///
    /// Increments `user.invalid_attempts` and `password_retry_count`,
    /// returns the caller-log "Password failure" entry, and may move the
    /// session to [`SessionState::LoggingOff`] when either failure limit
    /// is reached.
    ///
    /// # Errors
    /// Returns [`VerifyPasswordError::WrongState`] if the session is
    /// not in [`SessionState::Authenticating`], or
    /// [`VerifyPasswordError::UserMissing`] if no user is bound.
    pub fn apply_password_mismatch(
        &mut self,
        max_password_failures: u32,
        now: SystemTime,
    ) -> Result<(VerifyPasswordOutcome, CallerLog), VerifyPasswordError> {
        if self.state != SessionState::Authenticating {
            return Err(VerifyPasswordError::WrongState(self.state));
        }
        let user_mut = self.user.as_mut().ok_or(VerifyPasswordError::UserMissing)?;
        user_mut.bump_invalid_attempts();
        self.password_retry_count = self.password_retry_count.saturating_add(1);

        let entry = CallerLog {
            session_node: self.node_number,
            at: now,
            text: "Password failure".to_string(),
            is_password_failure: true,
        };

        let user_attempts = self.user.as_ref().expect("user present").invalid_attempts();
        let outcome = if user_attempts >= max_password_failures {
            self.user.as_mut().expect("user present").lock_account();
            self.transition_to(SessionState::LoggingOff)
                .expect("authenticating -> logging_off is permitted");
            self.logoff_reason = Some(LogoffReason::LockedAccount);
            VerifyPasswordOutcome::AccountLocked
        } else if self.password_retry_count >= max_password_failures {
            self.transition_to(SessionState::LoggingOff)
                .expect("authenticating -> logging_off is permitted");
            self.logoff_reason = Some(LogoffReason::ExcessivePasswordFails);
            VerifyPasswordOutcome::TooManyFailures
        } else {
            VerifyPasswordOutcome::NotMatching
        };
        Ok((outcome, entry))
    }
}

/// Errors returned by [`Session::accept_connection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptConnectionError {
    /// The node already has a non-ended session bound to it.
    AlreadyActiveSession,
}

impl std::fmt::Display for AcceptConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActiveSession => {
                write!(f, "node already has an active session")
            }
        }
    }
}

impl std::error::Error for AcceptConnectionError {}

/// Outcome of [`Session::name_typed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTypedOutcome {
    /// User found; session has moved to authenticating and is ready
    /// for [`Session::user`] to drive the password prompt.
    Authenticated,
    /// Handle did not match any user. The retry counter has been
    /// incremented. The listener should re-prompt.
    NotFound,
    /// Five not-found strikes in a row. The session has ended with
    /// [`LogoffReason::NewUserRejected`].
    SessionEnded,
    /// The literal `NEW` was typed. Slice 9 does not implement the
    /// registration branch; Slice 19 wires it up.
    NewUserRejected,
}

/// Errors returned by [`Session::name_typed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTypedError {
    /// The session is not in [`SessionState::Identifying`].
    WrongState(SessionState),
}

impl std::fmt::Display for NameTypedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "name typed in unexpected state: {s:?}"),
        }
    }
}

impl std::error::Error for NameTypedError {}

/// Outcome of [`Session::verify_password`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyPasswordOutcome {
    /// Credentials match. The session has moved to
    /// [`SessionState::Onboarded`], `authenticated_at` is set, and
    /// `user.invalid_attempts` is cleared.
    Authenticated,
    /// Credentials do not match. The session stays in
    /// [`SessionState::Authenticating`]; the listener should re-prompt.
    NotMatching,
    /// `user.invalid_attempts` reached `max_password_failures`. The
    /// account is now locked, the session has moved to
    /// [`SessionState::LoggingOff`] with
    /// [`LogoffReason::LockedAccount`].
    AccountLocked,
    /// `password_retry_count` reached `max_password_failures` for
    /// this session. The session has moved to
    /// [`SessionState::LoggingOff`] with
    /// [`LogoffReason::ExcessivePasswordFails`].
    TooManyFailures,
}

/// Errors returned by [`Session::verify_password`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyPasswordError {
    /// The session is not in [`SessionState::Authenticating`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
    /// The hasher rejected the user's stored hash kind.
    HashKindUnsupported(PasswordError),
}

impl std::fmt::Display for VerifyPasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "verify_password in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "verify_password called without a bound user"),
            Self::HashKindUnsupported(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VerifyPasswordError {}

/// Errors returned by [`Session::enter_menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterMenuError {
    /// The session is not in [`SessionState::Onboarded`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
}

impl std::fmt::Display for EnterMenuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "enter_menu in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "enter_menu called without a bound user"),
        }
    }
}

impl std::error::Error for EnterMenuError {}

/// `session.allium:format_logon_line` black-box helper.
///
/// Produces the line written to the caller log when a session reaches
/// the menu. The legacy AmiExpress format is something like
/// `Logon: alice (node 1, 9600 baud, remote)`; we match that shape.
fn format_logon_line(session: &Session) -> String {
    let handle = session.user.as_ref().map(|u| u.handle()).unwrap_or("?");
    let channel = match session.channel {
        LogonChannel::SysopConsole => "sysop_console",
        LogonChannel::Local => "local",
        LogonChannel::Remote => "remote",
        LogonChannel::Ftp => "ftp",
    };
    format!(
        "Logon: {handle} (node {}, {} baud, {channel})",
        session.node_number, session.online_baud
    )
}

/// `session.allium:format_logoff_line` black-box helper.
///
/// Phase 1 emits a minimal line. Slice 53 onward extends it with
/// transfer accounting (`bytes_uploaded`, `bytes_downloaded`).
fn format_logoff_line(session: &Session) -> String {
    let handle = session.user.as_ref().map(|u| u.handle()).unwrap_or("?");
    let reason = match session.logoff_reason {
        Some(LogoffReason::NormalLogoff) => "normal_logoff",
        Some(LogoffReason::NewUserRejected) => "new_user_rejected",
        Some(LogoffReason::ExcessivePasswordFails) => "excessive_password_fails",
        Some(LogoffReason::LockedAccount) => "locked_account",
        None => "unknown",
    };
    format!(
        "Logoff: {handle} (node {}, reason {reason})",
        session.node_number
    )
}

/// Returned when the requested transition is not in the spec's
/// transition table for the Phase 1 subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTransitionError {
    /// State the session was in when the transition was attempted.
    pub from: SessionState,
    /// State the caller asked to move into.
    pub to: SessionState,
}

impl std::fmt::Display for SessionTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid session transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for SessionTransitionError {}

/// Returns whether the spec's Phase 1 transition table permits
/// `from -> to`. The `new_user_registering` branch is omitted (Slice 19
/// adds it).
///
/// `Authenticating -> LoggingOff` is included to let
/// `session.allium:VerifyPassword` end the session via its
/// FinaliseLogoff hand-off. The Allium transition list omits this
/// transition explicitly, but the rule's body implies it; the Rust
/// port follows the rule.
fn is_session_transition_allowed(from: SessionState, to: SessionState) -> bool {
    use SessionState::*;
    matches!(
        (from, to),
        (Connecting, Identifying)
            | (Connecting, Ended)
            | (Identifying, Authenticating)
            | (Identifying, Ended)
            | (Authenticating, Onboarded)
            | (Authenticating, Ended)
            | (Authenticating, LoggingOff)
            | (Onboarded, Menu)
            | (Onboarded, LoggingOff)
            | (Menu, LoggingOff)
            | (LoggingOff, Ended)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_session(channel: LogonChannel) -> Session {
        Session::new(1, channel, 9_600, SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn new_session_is_connecting() {
        let session = new_session(LogonChannel::Remote);
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.channel(), LogonChannel::Remote);
        assert_eq!(session.node_number(), 1);
        assert_eq!(session.online_baud(), 9_600);
        assert_eq!(session.connected_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.last_input_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.name_retry_count(), 0);
        assert_eq!(session.password_retry_count(), 0);
        assert!(session.user().is_none());
        assert!(session.typed_name().is_none());
        assert!(session.authenticated_at().is_none());
        assert!(session.logoff_at().is_none());
        assert!(session.logoff_reason().is_none());
    }

    #[test]
    fn is_remote_true_for_remote_and_ftp_only() {
        assert!(new_session(LogonChannel::Remote).is_remote());
        assert!(new_session(LogonChannel::Ftp).is_remote());
        assert!(!new_session(LogonChannel::Local).is_remote());
        assert!(!new_session(LogonChannel::SysopConsole).is_remote());
    }

    #[test]
    fn full_phase1_state_path_is_allowed() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
        s.transition_to(SessionState::Menu).unwrap();
        s.transition_to(SessionState::LoggingOff).unwrap();
        s.transition_to(SessionState::Ended).unwrap();
        assert_eq!(s.state(), SessionState::Ended);
    }

    #[test]
    fn carrier_drop_from_connecting_ends_session() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Ended).expect("allowed");
    }

    #[test]
    fn carrier_drop_from_identifying_ends_session() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Ended).expect("allowed");
    }

    #[test]
    fn carrier_drop_from_authenticating_ends_session() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Ended).expect("allowed");
    }

    #[test]
    fn onboarded_can_short_circuit_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
        s.transition_to(SessionState::LoggingOff)
            .expect("onboarded -> logging_off allowed");
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .transition_to(SessionState::Onboarded)
            .expect_err("connecting -> onboarded not allowed");
        assert_eq!(err.from, SessionState::Connecting);
        assert_eq!(err.to, SessionState::Onboarded);
        assert_eq!(s.state(), SessionState::Connecting);
    }

    fn alice() -> User {
        User::new(
            2,
            "alice".to_string(),
            crate::domain::password::PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    #[test]
    fn unauthenticated_session_is_not_authenticated() {
        let session = new_session(LogonChannel::Remote);
        assert!(!session.is_authenticated());
    }

    #[test]
    fn onboarded_session_with_user_is_authenticated() {
        let mut session = new_session(LogonChannel::Remote);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", alice()).unwrap();
        session
            .apply_password_match(SystemTime::UNIX_EPOCH)
            .unwrap();
        assert!(session.is_authenticated());
    }

    #[test]
    fn authenticating_with_user_is_not_yet_authenticated() {
        let mut session = new_session(LogonChannel::Remote);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", alice()).unwrap();
        assert!(!session.is_authenticated());
    }

    #[test]
    fn onboarded_without_user_is_not_authenticated() {
        let mut session = new_session(LogonChannel::Remote);
        session.transition_to(SessionState::Identifying).unwrap();
        session.transition_to(SessionState::Authenticating).unwrap();
        session.transition_to(SessionState::Onboarded).unwrap();
        assert!(!session.is_authenticated());
    }

    #[test]
    fn accept_connection_creates_session_with_zero_retries() {
        let session = Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            None,
        )
        .expect("should accept");
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.node_number(), 3);
        assert_eq!(session.channel(), LogonChannel::Remote);
        assert_eq!(session.online_baud(), 9_600);
        assert_eq!(session.connected_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.last_input_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.name_retry_count(), 0);
        assert_eq!(session.password_retry_count(), 0);
    }

    #[test]
    fn accept_connection_rejects_when_active_session_exists() {
        let existing = Session::new(3, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        let err = Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            Some(&existing),
        )
        .expect_err("active session should block accept");
        assert_eq!(err, AcceptConnectionError::AlreadyActiveSession);
    }

    #[test]
    fn accept_connection_allows_when_existing_session_ended() {
        let mut existing = Session::new(3, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        existing.transition_to(SessionState::Ended).unwrap();
        Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            Some(&existing),
        )
        .expect("ended session should not block accept");
    }

    #[test]
    fn prompt_for_name_moves_to_identifying() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().expect("connecting -> identifying");
        assert_eq!(s.state(), SessionState::Identifying);
    }

    #[test]
    fn prompt_for_name_rejects_outside_connecting() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        let err = s
            .prompt_for_name()
            .expect_err("identifying -> identifying not allowed");
        assert_eq!(err.from, SessionState::Identifying);
    }

    #[test]
    fn name_typed_found_advances_to_authenticating() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_identified_user("alice", alice())
            .expect("name_typed");
        assert_eq!(outcome, NameTypedOutcome::Authenticated);
        assert_eq!(s.state(), SessionState::Authenticating);
        assert_eq!(s.typed_name(), Some("alice"));
        assert_eq!(s.user().map(|u| u.handle()), Some("alice"));
    }

    #[test]
    fn name_typed_not_found_increments_retry() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, NameTypedOutcome::NotFound);
        assert_eq!(s.state(), SessionState::Identifying);
        assert_eq!(s.name_retry_count(), 1);
    }

    #[test]
    fn name_typed_five_strikes_ends_session() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        for _ in 0..4 {
            assert_eq!(
                s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap(),
                NameTypedOutcome::NotFound
            );
        }
        assert_eq!(s.name_retry_count(), 4);
        let outcome = s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, NameTypedOutcome::SessionEnded);
        assert_eq!(s.state(), SessionState::Ended);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
        assert_eq!(s.logoff_at(), Some(SystemTime::UNIX_EPOCH));
    }

    #[test]
    fn name_typed_new_keyword_returns_new_user_rejected() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s.reject_new_user_request().unwrap();
        assert_eq!(outcome, NameTypedOutcome::NewUserRejected);
        // No state change, no retry bump.
        assert_eq!(s.state(), SessionState::Identifying);
        assert_eq!(s.name_retry_count(), 0);
    }

    fn authenticated_session() -> Session {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s
    }

    #[test]
    fn verify_password_match_advances_to_onboarded() {
        let mut s = authenticated_session();
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60);
        let outcome = s.apply_password_match(now).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::Authenticated);
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.authenticated_at(), Some(now));
        assert!(s.is_authenticated());
    }

    #[test]
    fn verify_password_match_clears_user_attempts() {
        let mut s = authenticated_session();
        // Pre-existing attempts on the user (e.g. from a prior failed
        // session) should be cleared on success.
        s.user.as_mut().unwrap().bump_invalid_attempts();
        s.apply_password_match(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_mismatch_bumps_counters() {
        let mut s = authenticated_session();
        let (outcome, entry) = s
            .apply_password_mismatch(3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::NotMatching);
        assert_eq!(s.state(), SessionState::Authenticating);
        assert_eq!(s.password_retry_count(), 1);
        assert_eq!(s.user().unwrap().invalid_attempts(), 1);
        assert_eq!(entry.text, "Password failure");
        assert!(entry.is_password_failure);
    }

    #[test]
    fn verify_password_locks_account_when_user_attempts_reach_max() {
        let mut s = authenticated_session();
        let (outcome, _entry) = s
            .apply_password_mismatch(1, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::AccountLocked);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
        assert!(s.user().unwrap().is_account_locked());
        // LockoutClearsAttempts: attempts cleared on lock.
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_session_level_trip_fires_when_user_counter_reset() {
        // The session-level check (`password_retry_count >= max`)
        // only fires when the user-level counter happens to be below
        // max. In normal operation both counters track 1:1, so the
        // user-level check wins. This test manually clears the user
        // counter mid-session to exercise the session-level branch.
        let mut s = authenticated_session();
        s.apply_password_mismatch(5, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_password_mismatch(5, SystemTime::UNIX_EPOCH)
            .unwrap();
        // Simulate an out-of-band reset of the user-level counter.
        s.user.as_mut().unwrap().clear_invalid_attempts();
        let (outcome, _entry) = s
            .apply_password_mismatch(3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::TooManyFailures);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(
            s.logoff_reason(),
            Some(LogoffReason::ExcessivePasswordFails)
        );
        assert!(!s.user().unwrap().is_account_locked());
    }

    #[test]
    fn enter_menu_advances_state_and_logs() {
        let mut s = authenticated_session();
        // Get to onboarded via successful verify.
        s.apply_password_match(SystemTime::UNIX_EPOCH).unwrap();
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(120);
        let entry = s.enter_menu(now).unwrap();
        assert_eq!(s.state(), SessionState::Menu);
        assert_eq!(s.user().unwrap().times_called(), 1);
        assert!(
            entry.text.contains("Logon:")
                && entry.text.contains("alice")
                && !entry.is_password_failure,
            "expected logon caller-log entry, got {entry:?}"
        );
    }

    #[test]
    fn enter_menu_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("must be onboarded");
        assert!(matches!(err, EnterMenuError::WrongState(_)));
    }

    /// Drives a session from connecting to menu via the rule chain.
    fn session_at_menu() -> Session {
        let mut s = authenticated_session();
        s.apply_password_match(SystemTime::UNIX_EPOCH).unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s
    }

    #[test]
    fn user_requests_logoff_from_menu_records_normal_logoff() {
        let mut s = session_at_menu();
        s.user_requests_logoff().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NormalLogoff));
    }

    #[test]
    fn user_requests_logoff_from_onboarded_is_allowed() {
        let mut s = authenticated_session();
        s.apply_password_match(SystemTime::UNIX_EPOCH).unwrap();
        s.user_requests_logoff().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn user_requests_logoff_outside_menu_or_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .user_requests_logoff()
            .expect_err("connecting cannot log off");
        assert_eq!(err.from, SessionState::Connecting);
    }

    #[test]
    fn finalise_logoff_updates_user_and_logs_goodbye() {
        let mut s = session_at_menu();
        s.user_requests_logoff().unwrap();
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(300);
        let entry = s.finalise_logoff(now).unwrap();
        assert_eq!(s.state(), SessionState::Ended);
        assert_eq!(s.logoff_at(), Some(now));
        assert_eq!(s.user().unwrap().last_call(), Some(now));
        assert!(
            entry.text.contains("Logoff:") && entry.text.contains("alice"),
            "expected logoff caller-log entry, got {entry:?}"
        );
    }

    #[test]
    fn finalise_logoff_outside_logging_off_errors() {
        let mut s = session_at_menu();
        let err = s
            .finalise_logoff(SystemTime::UNIX_EPOCH)
            .expect_err("must be logging_off");
        assert_eq!(err.from, SessionState::Menu);
    }

    #[test]
    fn verify_password_outside_authenticating_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .apply_password_match(SystemTime::UNIX_EPOCH)
            .expect_err("must be authenticating");
        assert!(matches!(err, VerifyPasswordError::WrongState(_)));
    }

    #[test]
    fn name_typed_outside_identifying_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .record_identified_user("alice", alice())
            .expect_err("must be in identifying");
        assert!(matches!(err, NameTypedError::WrongState(_)));
    }
}
