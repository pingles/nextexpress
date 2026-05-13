//! [`Session`] entity (spec: `session.allium:Session`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop reads.
//! Presentation booleans, time accounting, temp access and reserved-for
//! arrive in their owning slices.

use std::time::{Duration, SystemTime};

use crate::domain::caller_log::CallerLog;
use crate::domain::conference::{first_accessible_conference, Conference, NameType};
use crate::domain::conference_visit::{
    next_accessible_conference_after, primary_msgbase_of, resolve_auto_rejoin,
    resolve_explicit_join, ConferenceScan, ConferenceVisit, JoinResolution,
};
use crate::domain::user::User;

mod budget;
mod conference_activity;
mod errors;
mod lockout;
mod log_format;
mod outcomes;
mod transitions;
pub(crate) mod typed;

#[cfg(test)]
mod tests;

use conference_activity::ConferenceActivity;

use log_format::{format_logoff_line, format_logon_line};

/// Maximum number of unknown handle entries before a session is ended.
const MAX_NAME_RETRIES: u32 = 5;

pub use crate::domain::session_policy::{PasswordFailureDecision, SessionPolicy};
pub use budget::{initialise_daily_budget, tick_minute};
pub use errors::{
    AcceptConnectionError, AutoRejoinError, CarrierLostError, CompleteNewUserRegistrationError,
    CompletePasswordResetError, EnterMenuError, ForcePasswordResetError, IdleTimeoutError,
    InitialiseDailyBudgetError, NameTypedError, TickMinuteError, VerifyNewUserPasswordError,
    VerifyPasswordError,
};
pub use lockout::{
    apply_password_change, apply_password_match, apply_password_mismatch,
    force_password_reset_if_due,
};
pub use outcomes::{
    AutoRejoinOutcome, ConferenceScanOutcome, ExplicitJoinOutcome, NameTypedOutcome,
    NewUserPasswordOutcome, NewUserRequestOutcome, TickMinuteOutcome, VerifyPasswordOutcome,
};
pub use transitions::SessionTransitionError;

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
    /// `JoinConference` could not resolve any conference for the
    /// user, so the session terminates immediately
    /// (`conferences.allium:JoinConference`'s
    /// `resolved_conference = null` branch, Slice 30).
    NoConferenceAccess,
    /// The session burned through `time_remaining` while in
    /// `onboarded` or `menu`. Set by
    /// `session.allium:TimeExpired` (Slice 14).
    OutOfTime,
    /// The session received no input for longer than
    /// `core/config.input_timeout` and
    /// `treat_timeout_as_logoff` is `true`
    /// (`session.allium:IdleTimeout`, Slice 17).
    InputTimeout,
    /// Either the transport reported the connection had gone away
    /// (`session.allium:CarrierLost`, Slice 18), or the idle
    /// timeout fired with `treat_timeout_as_logoff = false`
    /// (Slice 17).
    CarrierLoss,
}

/// Lifecycle state of a [`Session`] (spec: `session.allium:Session.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Connection accepted, banner not yet displayed.
    Connecting,
    /// Prompting the user for their handle.
    Identifying,
    /// Verifying a typed password.
    Authenticating,
    /// User typed `NEW`; the registration sub-flow is in progress
    /// (spec: `session.allium:Session.state` `new_user_registering`).
    NewUserRegistering,
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
pub struct Session {
    shared: SessionShared,
    phase: SessionPhase,
    /// Per-session conference state: visit history plus any
    /// in-progress `CS` scan. Held outside [`SessionPhase`] so it
    /// survives `Onboarded -> Menu` transitions.
    activity: ConferenceActivity,
}

/// Session fields that are valid for every lifecycle phase.
#[derive(Debug, Clone)]
struct SessionShared {
    node_number: u32,
    channel: LogonChannel,
    connected_at: SystemTime,
    last_input_at: SystemTime,
    online_baud: u32,
    /// `session.allium:Session.quick_logon` (first read in Slice 31).
    /// When `true` the listener skips on-logon screens that are
    /// considered chrome — currently the post-join conference
    /// bulletin (`conferences.allium:ShowConferenceBulletin`). The
    /// full toggle UI lands in Slice 65.
    quick_logon: bool,
    /// `session.allium:Session.display_name_type` (first read in
    /// Slice 34). Set on every successful conference join to the
    /// joined conference's `accepted_name_type`; controls how the
    /// user's identity is rendered in messages going forward.
    display_name_type: NameType,
}

/// Lifecycle-specific session data.
///
/// This keeps state payloads next to the state that makes them valid:
/// authenticated phases always carry a [`User`], password retry counts
/// exist only while authenticating, and new-user gate counters exist
/// only during registration.
#[derive(Debug, Clone)]
enum SessionPhase {
    Connecting,
    Identifying {
        name_retry_count: u32,
    },
    Authenticating {
        typed_name: String,
        user: User,
        password_retry_count: u32,
    },
    NewUserRegistering {
        password_verified: bool,
        password_attempts: u32,
    },
    Onboarded {
        user: User,
        authenticated_at: SystemTime,
        time_remaining: Duration,
    },
    Menu {
        user: User,
        authenticated_at: SystemTime,
        time_remaining: Duration,
    },
    LoggingOff {
        user: Option<User>,
        authenticated_at: Option<SystemTime>,
        reason: Option<LogoffReason>,
        time_remaining: Duration,
    },
    Ended {
        user: Option<User>,
        authenticated_at: Option<SystemTime>,
        reason: Option<LogoffReason>,
        logoff_at: Option<SystemTime>,
        time_remaining: Duration,
    },
}

impl SessionPhase {
    fn state(&self) -> SessionState {
        match self {
            Self::Connecting => SessionState::Connecting,
            Self::Identifying { .. } => SessionState::Identifying,
            Self::Authenticating { .. } => SessionState::Authenticating,
            Self::NewUserRegistering { .. } => SessionState::NewUserRegistering,
            Self::Onboarded { .. } => SessionState::Onboarded,
            Self::Menu { .. } => SessionState::Menu,
            Self::LoggingOff { .. } => SessionState::LoggingOff,
            Self::Ended { .. } => SessionState::Ended,
        }
    }

    fn user(&self) -> Option<&User> {
        match self {
            Self::Authenticating { user, .. }
            | Self::Onboarded { user, .. }
            | Self::Menu { user, .. } => Some(user),
            Self::LoggingOff { user, .. } | Self::Ended { user, .. } => user.as_ref(),
            Self::Connecting | Self::Identifying { .. } | Self::NewUserRegistering { .. } => None,
        }
    }

    fn user_mut(&mut self) -> Option<&mut User> {
        match self {
            Self::Authenticating { user, .. }
            | Self::Onboarded { user, .. }
            | Self::Menu { user, .. } => Some(user),
            Self::LoggingOff { user, .. } | Self::Ended { user, .. } => user.as_mut(),
            Self::Connecting | Self::Identifying { .. } | Self::NewUserRegistering { .. } => None,
        }
    }

    fn typed_name(&self) -> Option<&str> {
        match self {
            Self::Authenticating { typed_name, .. } => Some(typed_name),
            _ => None,
        }
    }

    fn name_retry_count(&self) -> u32 {
        match self {
            Self::Identifying { name_retry_count } => *name_retry_count,
            _ => 0,
        }
    }

    fn password_retry_count(&self) -> u32 {
        match self {
            Self::Authenticating {
                password_retry_count,
                ..
            } => *password_retry_count,
            _ => 0,
        }
    }

    fn authenticated_at(&self) -> Option<SystemTime> {
        match self {
            Self::Onboarded {
                authenticated_at, ..
            }
            | Self::Menu {
                authenticated_at, ..
            } => Some(*authenticated_at),
            Self::LoggingOff {
                authenticated_at, ..
            }
            | Self::Ended {
                authenticated_at, ..
            } => *authenticated_at,
            Self::Connecting
            | Self::Identifying { .. }
            | Self::Authenticating { .. }
            | Self::NewUserRegistering { .. } => None,
        }
    }

    fn logoff_at(&self) -> Option<SystemTime> {
        match self {
            Self::Ended { logoff_at, .. } => *logoff_at,
            _ => None,
        }
    }

    fn logoff_reason(&self) -> Option<LogoffReason> {
        match self {
            Self::LoggingOff { reason, .. } | Self::Ended { reason, .. } => *reason,
            _ => None,
        }
    }

    fn time_remaining(&self) -> Duration {
        match self {
            Self::Onboarded { time_remaining, .. }
            | Self::Menu { time_remaining, .. }
            | Self::LoggingOff { time_remaining, .. }
            | Self::Ended { time_remaining, .. } => *time_remaining,
            Self::Connecting
            | Self::Identifying { .. }
            | Self::Authenticating { .. }
            | Self::NewUserRegistering { .. } => Duration::ZERO,
        }
    }

    fn new_user_password_verified(&self) -> bool {
        match self {
            Self::NewUserRegistering {
                password_verified, ..
            } => *password_verified,
            _ => false,
        }
    }

    fn new_user_password_attempts(&self) -> u32 {
        match self {
            Self::NewUserRegistering {
                password_attempts, ..
            } => *password_attempts,
            _ => 0,
        }
    }
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
    #[must_use]
    pub fn new(
        node_number: u32,
        channel: LogonChannel,
        online_baud: u32,
        connected_at: SystemTime,
    ) -> Self {
        Self {
            shared: SessionShared {
                node_number,
                channel,
                connected_at,
                last_input_at: connected_at,
                online_baud,
                quick_logon: false,
                display_name_type: NameType::Handle,
            },
            phase: SessionPhase::Connecting,
            activity: ConferenceActivity::new(),
        }
    }

    /// Returns whether the session is in quick-logon mode, suppressing
    /// chrome-y on-logon screens (currently the post-join conference
    /// bulletin). Mirrors `session.allium:Session.quick_logon`.
    #[must_use]
    pub fn quick_logon(&self) -> bool {
        self.shared.quick_logon
    }

    /// Sets [`Self::quick_logon`]. Tests and the future Slice-65
    /// presentation-toggles flow drive this directly.
    pub fn set_quick_logon(&mut self, quick: bool) {
        self.shared.quick_logon = quick;
    }

    /// Returns the [`NameType`] the session is currently rendering
    /// the user's identity as
    /// (`session.allium:Session.display_name_type`, Slice 34).
    /// Updated on every successful conference join via the spec's
    /// `JoinedConferenceForNameType` rule.
    #[must_use]
    pub fn display_name_type(&self) -> NameType {
        self.shared.display_name_type
    }

    /// Returns this session's node number.
    #[must_use]
    pub fn node_number(&self) -> u32 {
        self.shared.node_number
    }

    /// Returns the channel the session was opened on.
    #[must_use]
    pub fn channel(&self) -> LogonChannel {
        self.shared.channel
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub fn state(&self) -> SessionState {
        self.phase.state()
    }

    /// Returns the user this session has identified as, if any.
    #[must_use]
    pub fn user(&self) -> Option<&User> {
        self.phase.user()
    }

    /// Returns the handle the user typed at the identify prompt, if any.
    #[must_use]
    pub fn typed_name(&self) -> Option<&str> {
        self.phase.typed_name()
    }

    /// Returns the number of name-not-found strikes accumulated on this
    /// session.
    #[must_use]
    pub fn name_retry_count(&self) -> u32 {
        self.phase.name_retry_count()
    }

    /// Returns the number of bad-password strikes accumulated on this
    /// session.
    #[must_use]
    pub fn password_retry_count(&self) -> u32 {
        self.phase.password_retry_count()
    }

    /// Returns the timestamp the connection was accepted.
    #[must_use]
    pub fn connected_at(&self) -> SystemTime {
        self.shared.connected_at
    }

    /// Returns the timestamp of the last input received from the user.
    #[must_use]
    pub fn last_input_at(&self) -> SystemTime {
        self.shared.last_input_at
    }

    /// Returns the connection baud rate (0 for local sessions).
    #[must_use]
    pub fn online_baud(&self) -> u32 {
        self.shared.online_baud
    }

    /// Returns the timestamp at which authentication completed, if it
    /// has.
    #[must_use]
    pub fn authenticated_at(&self) -> Option<SystemTime> {
        self.phase.authenticated_at()
    }

    /// Returns the timestamp the session ended, if it has.
    #[must_use]
    pub fn logoff_at(&self) -> Option<SystemTime> {
        self.phase.logoff_at()
    }

    /// Returns the reason recorded for the session ending, if any.
    #[must_use]
    pub fn logoff_reason(&self) -> Option<LogoffReason> {
        self.phase.logoff_reason()
    }

    /// Returns how much per-call time the session has left.
    ///
    /// Set on the `authenticating -> onboarded` transition by
    /// [`Session::initialise_daily_budget`] and decremented each minute
    /// by [`Session::tick_minute`]. Slice 14.
    #[must_use]
    pub fn time_remaining(&self) -> Duration {
        self.phase.time_remaining()
    }

    /// Whether the new-user password gate
    /// (`session.allium:VerifyNewUserPassword`, Slice 20a) has been
    /// satisfied for this session. Always `true` when no gate is
    /// configured. Read by `CompleteNewUserRegistration` as a
    /// precondition.
    #[must_use]
    pub fn new_user_password_verified(&self) -> bool {
        self.phase.new_user_password_verified()
    }

    /// Number of incorrect new-user password attempts recorded against
    /// this session. Bounded by
    /// `core/config.max_new_user_password_attempts` per the
    /// `SessionRetriesBounded` invariant.
    #[must_use]
    pub fn new_user_password_attempts(&self) -> u32 {
        self.phase.new_user_password_attempts()
    }

    /// Spec-derived predicate: `channel in {remote, ftp}`.
    #[must_use]
    pub fn is_remote(&self) -> bool {
        matches!(
            self.shared.channel,
            LogonChannel::Remote | LogonChannel::Ftp
        )
    }

    /// Spec-derived predicate:
    /// `state in {onboarded, menu, logging_off, ended} and user != null`.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user().is_some()
            && matches!(
                self.state(),
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
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state() != SessionState::Ended
    }

    fn move_to_logging_off(&mut self, reason: Option<LogoffReason>) {
        let previous = std::mem::replace(&mut self.phase, SessionPhase::Connecting);
        let (user, authenticated_at, time_remaining) = match previous {
            SessionPhase::Connecting
            | SessionPhase::Identifying { .. }
            | SessionPhase::NewUserRegistering { .. } => (None, None, Duration::ZERO),
            SessionPhase::Authenticating { user, .. } => (Some(user), None, Duration::ZERO),
            SessionPhase::Onboarded {
                user,
                authenticated_at,
                time_remaining,
            }
            | SessionPhase::Menu {
                user,
                authenticated_at,
                time_remaining,
            } => (Some(user), Some(authenticated_at), time_remaining),
            SessionPhase::LoggingOff {
                user,
                authenticated_at,
                time_remaining,
                ..
            }
            | SessionPhase::Ended {
                user,
                authenticated_at,
                time_remaining,
                ..
            } => (user, authenticated_at, time_remaining),
        };
        self.phase = SessionPhase::LoggingOff {
            user,
            authenticated_at,
            reason,
            time_remaining,
        };
    }

    fn move_to_ended(&mut self, logoff_at: Option<SystemTime>) {
        let previous = std::mem::replace(&mut self.phase, SessionPhase::Connecting);
        let (user, authenticated_at, reason, time_remaining) = match previous {
            SessionPhase::Connecting
            | SessionPhase::Identifying { .. }
            | SessionPhase::NewUserRegistering { .. } => (None, None, None, Duration::ZERO),
            SessionPhase::Authenticating { user, .. } => (Some(user), None, None, Duration::ZERO),
            SessionPhase::Onboarded {
                user,
                authenticated_at,
                time_remaining,
            }
            | SessionPhase::Menu {
                user,
                authenticated_at,
                time_remaining,
            } => (Some(user), Some(authenticated_at), None, time_remaining),
            SessionPhase::LoggingOff {
                user,
                authenticated_at,
                reason,
                time_remaining,
            }
            | SessionPhase::Ended {
                user,
                authenticated_at,
                reason,
                time_remaining,
                ..
            } => (user, authenticated_at, reason, time_remaining),
        };
        self.phase = SessionPhase::Ended {
            user,
            authenticated_at,
            reason,
            logoff_at,
            time_remaining,
        };
    }

    /// `session.allium:AcceptConnection` rule.
    ///
    /// Creates a fresh [`Session`] for `node_number`. Rejects when
    /// `existing_session_for_node` already holds an active session for
    /// that node — the spec's `OneActiveSessionPerNode` invariant. The
    /// caller (typically the application supervisor that owns the
    /// node pool) is responsible for ensuring the underlying node is in
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
        if existing_session_for_node.is_some_and(Session::is_active) {
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
        self.phase = SessionPhase::Authenticating {
            typed_name: typed.to_string(),
            user,
            password_retry_count: 0,
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
                user: None,
                authenticated_at: None,
                reason: Some(LogoffReason::NewUserRejected),
                logoff_at: Some(now),
                time_remaining: Duration::ZERO,
            };
            Ok(NameTypedOutcome::SessionEnded)
        } else {
            Ok(NameTypedOutcome::NotFound)
        }
    }

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

    /// Returns this session's conference-visit history (spec:
    /// `conferences.allium:ConferenceVisit`). At most one entry has
    /// `left_at == None` thanks to
    /// [`Session::auto_rejoin_conference`] closing prior visits on
    /// every join — that's the
    /// `SessionsHaveAtMostOneOpenVisit` invariant.
    #[must_use]
    pub fn visits(&self) -> &[ConferenceVisit] {
        self.activity.visits()
    }

    /// Returns the visit currently open for this session, if any.
    /// Phase 4's join workflow (Slice 30) keeps this in lock-step
    /// with the bound user's `last_joined`.
    #[must_use]
    pub fn current_visit(&self) -> Option<&ConferenceVisit> {
        self.activity.current_visit()
    }

    /// Resolves the auto-rejoin path of
    /// `conferences.allium:JoinConference` (Slice 30).
    ///
    /// On a successful resolution the session attaches a fresh
    /// [`ConferenceVisit`] and updates the bound user's
    /// `last_joined`. When the user has no granted membership for
    /// any catalogued conference the session moves to
    /// [`SessionState::LoggingOff`] with
    /// [`LogoffReason::NoConferenceAccess`].
    ///
    /// # Parameters
    /// - `conferences`: catalogue loaded by the
    ///   [`crate::domain::conference_repository::ConferenceRepository`],
    ///   in ascending `number` order.
    /// - `now`: timestamp recorded as `joined_at` on the new visit
    ///   (and `left_at` on any prior open visit).
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn auto_rejoin_conference(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<AutoRejoinOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let resolution = resolve_auto_rejoin(user, conferences);
        match resolution {
            JoinResolution::NoAccess => {
                self.move_to_logging_off(Some(LogoffReason::NoConferenceAccess));
                Ok(AutoRejoinOutcome::NoAccess)
            }
            JoinResolution::Resolved {
                conference,
                msgbase,
                matched_request: _,
            } => {
                let conference_number = conference.number();
                let msgbase_number = msgbase.number();
                let conference_name_type = conference.accepted_name_type();
                user.record_join(conference, msgbase);
                let show_bulletin = !self.shared.quick_logon && !self.activity.is_scanning();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                self.activity.attach(conference_number, msgbase_number, now);
                Ok(AutoRejoinOutcome::Joined {
                    conference_number,
                    msgbase_number,
                    show_bulletin,
                    name_type_promoted_to,
                })
            }
        }
    }

    /// Updates `display_name_type` to `target` (spec:
    /// `conferences.allium:JoinedConferenceForNameType`, Slice 34).
    /// Returns `Some(target)` if the value changed and `None` if the
    /// session was already rendering that name-type, so callers can
    /// surface the change without keeping their own before/after
    /// state.
    fn promote_display_name_type(&mut self, target: NameType) -> Option<NameType> {
        if self.shared.display_name_type == target {
            None
        } else {
            self.shared.display_name_type = target;
            Some(target)
        }
    }

    /// Resolves the explicit-join path of
    /// `conferences.allium:JoinConference`
    /// (`reason = explicit_join`, Slice 32).
    ///
    /// Models the user typing `J <number>` from the menu. When the
    /// user has access to `target_conference_number` the session
    /// attaches there directly; otherwise the resolver falls
    /// through to `first_accessible_conference` and signals
    /// `matched_request = false` so the listener can render the
    /// legacy "You do not have access to the requested conference"
    /// notice (`amiexpress/express.e:25157`) before the JOIN /
    /// JOINED screens. With no granted memberships at all the
    /// session moves to [`SessionState::LoggingOff`] with
    /// [`LogoffReason::NoConferenceAccess`].
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn explicit_join_conference(
        &mut self,
        target_conference_number: u32,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ExplicitJoinOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let resolution = resolve_explicit_join(target_conference_number, user, conferences);
        match resolution {
            JoinResolution::NoAccess => {
                self.move_to_logging_off(Some(LogoffReason::NoConferenceAccess));
                Ok(ExplicitJoinOutcome::NoAccess)
            }
            JoinResolution::Resolved {
                conference,
                msgbase,
                matched_request,
            } => {
                let conference_number = conference.number();
                let msgbase_number = msgbase.number();
                let conference_name_type = conference.accepted_name_type();
                user.record_join(conference, msgbase);
                let show_bulletin = !self.shared.quick_logon && !self.activity.is_scanning();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                self.activity.attach(conference_number, msgbase_number, now);
                Ok(ExplicitJoinOutcome::Joined {
                    conference_number,
                    msgbase_number,
                    show_bulletin,
                    matched_request,
                    name_type_promoted_to,
                })
            }
        }
    }

    /// Returns the in-progress conference-scan, if any
    /// (`conferences.allium:ConferenceScan`, Slice 33).
    #[must_use]
    pub fn conference_scan(&self) -> Option<&ConferenceScan> {
        self.activity.scan()
    }

    /// Starts a `CS` conference scan
    /// (`conferences.allium:StartConferenceScan`, Slice 33).
    ///
    /// Initialises a [`ConferenceScan`] with `next_conference`
    /// pointing at the first conference the user has access to,
    /// and runs the first scan step so the listener has a join
    /// outcome to display. When the user has no granted membership
    /// the session terminates with
    /// [`LogoffReason::NoConferenceAccess`].
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn start_conference_scan(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ConferenceScanOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let first = first_accessible_conference(user.memberships(), conferences);
        let Some(first_conference) = first else {
            self.move_to_logging_off(Some(LogoffReason::NoConferenceAccess));
            return Ok(ConferenceScanOutcome::NoAccess);
        };

        let first_number = first_conference.number();
        let mb = primary_msgbase_of(first_conference);
        let msgbase_number = mb.number();
        let conference_name_type = first_conference.accepted_name_type();
        user.record_join(first_conference, mb);
        let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
        // The next call to step_conference_scan will resume from the
        // conference *after* this one.
        self.activity
            .set_scan(Some(ConferenceScan::new(Some(first_number), now)));
        self.activity.attach(first_number, msgbase_number, now);
        Ok(ConferenceScanOutcome::Stepped {
            conference_number: first_number,
            msgbase_number,
            name_type_promoted_to,
        })
    }

    /// Advances the in-progress conference scan
    /// (`conferences.allium:StepConferenceScan` /
    /// `FinishConferenceScan`, Slice 33).
    ///
    /// Joins the scan's `next_conference`. When no more conferences
    /// remain, the scan finishes: `in_progress` is cleared and the
    /// session re-attaches to `User.last_joined` per the spec's
    /// "re-join the user's last conference at the end of the scan".
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when no scan is
    /// in progress on this session (the listener should call
    /// [`Self::start_conference_scan`] first), or
    /// [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn step_conference_scan(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ConferenceScanOutcome, AutoRejoinError> {
        let Some(current_number) = self
            .activity
            .scan()
            .and_then(ConferenceScan::next_conference_number)
        else {
            return Err(AutoRejoinError::WrongState(self.state()));
        };
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        if let Some(next) = next_accessible_conference_after(user, conferences, current_number) {
            let next_number = next.number();
            let mb = primary_msgbase_of(next);
            let msgbase_number = mb.number();
            let conference_name_type = next.accepted_name_type();
            user.record_join(next, mb);
            let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
            self.activity
                .set_scan(Some(ConferenceScan::new(Some(next_number), now)));
            self.activity.attach(next_number, msgbase_number, now);
            Ok(ConferenceScanOutcome::Stepped {
                conference_number: next_number,
                msgbase_number,
                name_type_promoted_to,
            })
        } else {
            // FinishConferenceScan: clear the scan and re-attach to
            // the user's last_joined (which during the scan was
            // updated to the last visited conference).
            self.activity.set_scan(None);
            let last = user.last_joined();
            Ok(ConferenceScanOutcome::Finished {
                rejoined_conference: last.map(|r| r.conference_number()),
            })
        }
    }

    /// Applies `session.allium:CompleteNewUserRegistration`
    /// (Slice 20).
    ///
    /// Binds the freshly built `user`, sets `authenticated_at`,
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
        self.phase = SessionPhase::Onboarded {
            user,
            authenticated_at: now,
            time_remaining: Duration::ZERO,
        };
        Ok(self.on_enter_onboarded(policy, now))
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
    fn reject_locked_or_insufficient_access(&mut self, now: SystemTime) -> Option<CallerLog> {
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
