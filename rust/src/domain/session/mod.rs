//! [`Session`] entity (spec: `session.allium:Session`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop reads.
//! Presentation booleans, time accounting, temp access and reserved-for
//! arrive in their owning slices.

use std::time::{Duration, SystemTime};

use crate::domain::caller_log::CallerLog;
use crate::domain::conference::Conference;
use crate::domain::conference::{first_accessible_conference, NameType};
use crate::domain::conference_visit::{
    next_accessible_conference_after, primary_msgbase_of, resolve_auto_rejoin,
    resolve_explicit_join, ConferenceScan, ConferenceVisit, JoinResolution,
};
use crate::domain::user::User;

mod budget;
mod errors;
mod lockout;
mod log_format;
mod outcomes;
mod transitions;

#[cfg(test)]
use log_format::floor_to_day;
use log_format::{format_logoff_line, format_logon_line};
use transitions::is_session_transition_allowed;

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
    /// Per-session conference visits (spec:
    /// `conferences.allium:ConferenceVisit`). The collection is held
    /// at the session level rather than inside `SessionPhase` so it
    /// survives `Onboarded -> Menu` transitions and the
    /// `SessionsHaveAtMostOneOpenVisit` invariant remains visible to
    /// future-phase rules.
    visits: Vec<ConferenceVisit>,
    /// In-progress conference-scan, when the user has typed `CS`
    /// (`conferences.allium:ConferenceScan`, Slice 33). While set
    /// the `ShowConferenceBulletin` rule (Slice 31) suppresses
    /// bulletins on per-step joins.
    scan: Option<ConferenceScan>,
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
            visits: Vec::new(),
            scan: None,
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

    /// Attempts to transition the session to `target`.
    ///
    /// # Errors
    /// Returns [`SessionTransitionError`] if the spec does not permit
    /// the transition (Phase 1 subset of `session.allium:Session.state`).
    fn transition_to(&mut self, target: SessionState) -> Result<(), SessionTransitionError> {
        let from = self.state();
        if !is_session_transition_allowed(from, target) {
            return Err(SessionTransitionError { from, to: target });
        }
        self.transition_after_guard(target);
        Ok(())
    }

    fn transition_after_guard(&mut self, target: SessionState) {
        let from = self.state();
        debug_assert!(
            is_session_transition_allowed(from, target),
            "guarded transition should be permitted"
        );
        match target {
            SessionState::Connecting => self.phase = SessionPhase::Connecting,
            SessionState::Identifying => {
                self.phase = SessionPhase::Identifying {
                    name_retry_count: 0,
                };
            }
            SessionState::LoggingOff => self.move_to_logging_off(None),
            SessionState::Ended => self.move_to_ended(None),
            SessionState::Authenticating
            | SessionState::NewUserRegistering
            | SessionState::Onboarded
            | SessionState::Menu => {
                panic!("transition to {target:?} requires phase-specific payload");
            }
        }
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
        &self.visits
    }

    /// Returns the visit currently open for this session, if any.
    /// Phase 4's join workflow (Slice 30) keeps this in lock-step
    /// with the bound user's `last_joined`.
    #[must_use]
    pub fn current_visit(&self) -> Option<&ConferenceVisit> {
        self.visits.iter().find(|v| v.is_open())
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
                let show_bulletin = !self.shared.quick_logon && self.scan.is_none();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                for visit in &mut self.visits {
                    visit.close(now);
                }
                self.visits
                    .push(ConferenceVisit::new(conference_number, msgbase_number, now));
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
                let show_bulletin = !self.shared.quick_logon && self.scan.is_none();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                for visit in &mut self.visits {
                    visit.close(now);
                }
                self.visits
                    .push(ConferenceVisit::new(conference_number, msgbase_number, now));
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
        self.scan.as_ref()
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
        self.scan = Some(ConferenceScan::new(Some(first_number), now));
        for visit in &mut self.visits {
            visit.close(now);
        }
        self.visits
            .push(ConferenceVisit::new(first_number, msgbase_number, now));
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
            .scan
            .as_ref()
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
            self.scan = Some(ConferenceScan::new(Some(next_number), now));
            for visit in &mut self.visits {
                visit.close(now);
            }
            self.visits
                .push(ConferenceVisit::new(next_number, msgbase_number, now));
            Ok(ConferenceScanOutcome::Stepped {
                conference_number: next_number,
                msgbase_number,
                name_type_promoted_to,
            })
        } else {
            // FinishConferenceScan: clear the scan and re-attach to
            // the user's last_joined (which during the scan was
            // updated to the last visited conference).
            self.scan = None;
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

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::domain::password::PasswordHashKind;

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
        assert!(is_session_transition_allowed(
            SessionState::Connecting,
            SessionState::Identifying
        ));
        assert!(is_session_transition_allowed(
            SessionState::Identifying,
            SessionState::Authenticating
        ));
        assert!(is_session_transition_allowed(
            SessionState::Authenticating,
            SessionState::Onboarded
        ));
        assert!(is_session_transition_allowed(
            SessionState::Onboarded,
            SessionState::Menu
        ));
        assert!(is_session_transition_allowed(
            SessionState::Menu,
            SessionState::LoggingOff
        ));
        assert!(is_session_transition_allowed(
            SessionState::LoggingOff,
            SessionState::Ended
        ));
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
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s.transition_to(SessionState::Ended).expect("allowed");
    }

    #[test]
    fn onboarded_can_short_circuit_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
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
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
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
        assert_eq!(
            s.user().map(super::super::user::User::handle),
            Some("alice")
        );
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
    fn name_typed_new_keyword_transitions_to_new_user_registering() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            outcome,
            NewUserRequestOutcome::Initialised {
                password_required: false
            }
        );
        assert_eq!(s.state(), SessionState::NewUserRegistering);
        assert!(
            s.new_user_password_verified(),
            "no gate required => verified"
        );
        assert_eq!(s.new_user_password_attempts(), 0);
        // Retry counter is unrelated to the new-user branch.
        assert_eq!(s.name_retry_count(), 0);
    }

    #[test]
    fn record_new_user_request_with_gate_arms_unverified() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            outcome,
            NewUserRequestOutcome::Initialised {
                password_required: true
            }
        );
        assert!(!s.new_user_password_verified());
        assert_eq!(s.new_user_password_attempts(), 0);
    }

    #[test]
    fn record_new_user_request_with_disallowed_registration_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(false, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserRequestOutcome::Rejected);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
    }

    #[test]
    fn record_new_user_request_outside_identifying_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .expect_err("must be in identifying");
        assert!(matches!(err, NameTypedError::WrongState(_)));
    }

    #[test]
    fn carrier_loss_from_new_user_registering_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_carrier_loss().expect("permitted");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn idle_timeout_from_new_user_registering_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_idle_timeout(true).expect("permitted");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::InputTimeout));
    }

    #[test]
    fn apply_new_user_password_attempt_match_marks_verified() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let (outcome, entry) = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Verified);
        assert!(entry.is_none());
        assert!(s.new_user_password_verified());
    }

    #[test]
    fn apply_new_user_password_attempt_mismatch_increments_and_logs() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let (outcome, entry) = s
            .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Mismatch);
        let entry = entry.expect("caller-log entry");
        assert!(entry.text.contains("New-user password failure"));
        assert!(entry.is_password_failure);
        assert_eq!(s.new_user_password_attempts(), 1);
        assert!(!s.new_user_password_verified());
        assert_eq!(s.state(), SessionState::NewUserRegistering);
    }

    #[test]
    fn apply_new_user_password_attempt_max_failures_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        for _ in 0..2 {
            let (outcome, _) = s
                .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
                .unwrap();
            assert_eq!(outcome, NewUserPasswordOutcome::Mismatch);
        }
        let (outcome, entry) = s
            .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::TooManyFailures);
        assert!(entry.is_some());
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
    }

    #[test]
    fn apply_new_user_password_attempt_already_verified_errors() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        // No gate required => already verified.
        let err = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .expect_err("already verified should error");
        assert_eq!(err, VerifyNewUserPasswordError::AlreadyVerified);
    }

    #[test]
    fn apply_new_user_password_attempt_outside_new_user_registering_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .expect_err("must be in new_user_registering");
        assert!(matches!(err, VerifyNewUserPasswordError::WrongState(_)));
    }

    fn fresh_new_user(now: SystemTime) -> User {
        User::register_new(crate::domain::user::NewUserRegistration {
            slot_number: 7,
            handle: "newbie".to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555".to_string()),
            email: Some("n@example.com".to_string()),
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: crate::domain::password::PasswordHashKind::Pbkdf210000,
            line_length: 80,
            ansi_colour: true,
            flags: std::collections::BTreeSet::new(),
            ratio_mode: crate::domain::user::RatioMode::ByFiles,
            ratio_value: 3,
            now,
        })
        .expect("valid registration")
    }

    #[test]
    fn complete_new_user_registration_binds_user_and_onboards() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        // No gate configured; verified is set to true on entry.
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let rejection = s
            .complete_new_user_registration(fresh_new_user(now), SessionPolicy::default(), now)
            .expect("valid");
        assert!(rejection.is_none(), "fresh new user should not be rejected");
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.authenticated_at(), Some(now));
        assert_eq!(
            s.user().map(super::super::user::User::handle),
            Some("newbie")
        );
        // InitialiseDailyBudget consequent ran via on_enter_onboarded.
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn complete_new_user_registration_blocked_by_unverified_gate() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        // Gate armed but not yet satisfied.
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let err = s
            .complete_new_user_registration(
                fresh_new_user(SystemTime::UNIX_EPOCH),
                SessionPolicy::default(),
                SystemTime::UNIX_EPOCH,
            )
            .expect_err("gate not verified should error");
        assert_eq!(err, CompleteNewUserRegistrationError::GateNotVerified);
        assert_eq!(s.state(), SessionState::NewUserRegistering);
    }

    #[test]
    fn complete_new_user_registration_succeeds_after_gate_passes() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        // Gate-pass step.
        s.apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        s.complete_new_user_registration(fresh_new_user(now), SessionPolicy::default(), now)
            .expect("valid after gate passes");
        assert_eq!(s.state(), SessionState::Onboarded);
    }

    #[test]
    fn complete_new_user_registration_outside_new_user_registering_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .complete_new_user_registration(
                fresh_new_user(SystemTime::UNIX_EPOCH),
                SessionPolicy::default(),
                SystemTime::UNIX_EPOCH,
            )
            .expect_err("must be in new_user_registering");
        assert!(matches!(
            err,
            CompleteNewUserRegistrationError::WrongState(_)
        ));
    }

    fn authenticated_session() -> Session {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s
    }

    fn authenticating_user_mut(session: &mut Session) -> &mut User {
        match &mut session.phase {
            SessionPhase::Authenticating { user, .. } => user,
            other => panic!("expected authenticating phase, got {:?}", other.state()),
        }
    }

    fn set_authenticating_password_retry_count(session: &mut Session, count: u32) {
        match &mut session.phase {
            SessionPhase::Authenticating {
                password_retry_count,
                ..
            } => *password_retry_count = count,
            other => panic!("expected authenticating phase, got {:?}", other.state()),
        }
    }

    #[test]
    fn verify_password_match_advances_to_onboarded() {
        let mut s = authenticated_session();
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60);
        let (outcome, rejection) =
            apply_password_match(&mut s, SessionPolicy::default(), now).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::Authenticated);
        assert!(rejection.is_none());
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.authenticated_at(), Some(now));
        assert!(s.is_authenticated());
    }

    #[test]
    fn verify_password_match_clears_user_attempts() {
        let mut s = authenticated_session();
        // Pre-existing attempts on the user (e.g. from a prior failed
        // session) should be cleared on success.
        authenticating_user_mut(&mut s).bump_invalid_attempts();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_match_fires_initialise_daily_budget() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        // InitialiseDailyBudget consequent of the transition.
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn verify_password_mismatch_bumps_counters() {
        let mut s = authenticated_session();
        let (outcome, entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(3), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::NotMatching);
        assert_eq!(s.state(), SessionState::Authenticating);
        assert_eq!(s.password_retry_count(), 1);
        assert_eq!(s.user().unwrap().invalid_attempts(), 1);
        assert_eq!(entry.text, "Password failure");
        assert!(entry.is_password_failure);
    }

    #[test]
    fn session_policy_continues_below_password_failure_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 1);
        authenticating_user_mut(&mut s).bump_invalid_attempts();

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::Continue
        );
    }

    #[test]
    fn session_policy_locks_account_when_user_failures_reach_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 3);
        for _ in 0..3 {
            authenticating_user_mut(&mut s).bump_invalid_attempts();
        }

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::LockAccount
        );
    }

    #[test]
    fn session_policy_ends_session_when_session_failures_reach_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 3);

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::EndSession
        );
    }

    #[test]
    fn verify_password_locks_account_when_user_attempts_reach_max() {
        let mut s = authenticated_session();
        let (outcome, _entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(1), SystemTime::UNIX_EPOCH).unwrap();
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
        apply_password_mismatch(&mut s, SessionPolicy::new(5), SystemTime::UNIX_EPOCH).unwrap();
        apply_password_mismatch(&mut s, SessionPolicy::new(5), SystemTime::UNIX_EPOCH).unwrap();
        // Simulate an out-of-band reset of the user-level counter.
        authenticating_user_mut(&mut s).clear_invalid_attempts();
        let (outcome, _entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(3), SystemTime::UNIX_EPOCH).unwrap();
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
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
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
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
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
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
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
        let err = apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH)
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

    const DAILY_RESET_OFFSET: Duration = Duration::from_secs(6 * 3_600);

    fn user_with_time_limits(per_call: Duration, per_day: Duration) -> User {
        let mut u = alice();
        u.set_time_limits(per_call, per_day);
        u
    }

    /// Drives a session into [`SessionState::Onboarded`] via raw state
    /// transitions, deliberately bypassing the rules
    /// [`Session::apply_password_match`] fires on entry. The Slice 14
    /// rule tests use this so they can drive
    /// [`Session::initialise_daily_budget`] under controlled inputs.
    fn session_at_onboarded_with(user: User) -> Session {
        let mut s = new_session(LogonChannel::Remote);
        s.phase = SessionPhase::Onboarded {
            user,
            authenticated_at: SystemTime::UNIX_EPOCH,
            time_remaining: Duration::ZERO,
        };
        s
    }

    #[test]
    fn floor_to_day_buckets_into_24h_groups_offset_by_six_hours() {
        // Six hours past UNIX_EPOCH is the start of "day 0".
        let day_zero = UNIX_EPOCH + Duration::from_secs(6 * 3_600);
        assert_eq!(floor_to_day(day_zero, DAILY_RESET_OFFSET), 0);
        let just_before = day_zero - Duration::from_secs(1);
        assert_eq!(floor_to_day(just_before, DAILY_RESET_OFFSET), -1);
        let later_same_day = day_zero + Duration::from_secs(20 * 3_600);
        assert_eq!(floor_to_day(later_same_day, DAILY_RESET_OFFSET), 0);
        let next_day = day_zero + Duration::from_secs(24 * 3_600);
        assert_eq!(floor_to_day(next_day, DAILY_RESET_OFFSET), 1);
    }

    #[test]
    fn initialise_daily_budget_first_call_treats_as_new_day() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(30 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        // Spec: new-day branch sets times_called_today = 0.
        assert_eq!(s.user().unwrap().times_called_today(), 0);
        assert_eq!(s.user().unwrap().time_used_today(), Duration::ZERO);
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_same_day_bumps_times_called_today() {
        let mut user =
            user_with_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        // Pretend the user logged on earlier today.
        let earlier_today = UNIX_EPOCH + Duration::from_secs(7 * 3_600);
        user.record_last_call(earlier_today);
        user.add_time_used_today(Duration::from_secs(120));
        user.bump_times_called_today();
        let mut s = session_at_onboarded_with(user);

        let later_today = UNIX_EPOCH + Duration::from_secs(20 * 3_600);
        initialise_daily_budget(&mut s, later_today, DAILY_RESET_OFFSET).unwrap();
        // Same-day branch: times_called_today increments, time_used preserved.
        assert_eq!(s.user().unwrap().times_called_today(), 2);
        assert_eq!(
            s.user().unwrap().time_used_today(),
            Duration::from_secs(120)
        );
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_new_day_after_previous_day_resets() {
        let mut user =
            user_with_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        // Yesterday: 06:00 UTC of day 0 (the "start of day 0" in our offset).
        let yesterday = UNIX_EPOCH + Duration::from_secs(10 * 3_600);
        user.record_last_call(yesterday);
        user.add_time_used_today(Duration::from_secs(900));
        user.bump_times_called_today();
        user.bump_times_called_today();
        let mut s = session_at_onboarded_with(user);

        let today = UNIX_EPOCH + Duration::from_secs(36 * 3_600);
        initialise_daily_budget(&mut s, today, DAILY_RESET_OFFSET).unwrap();
        assert_eq!(s.user().unwrap().times_called_today(), 0);
        assert_eq!(s.user().unwrap().time_used_today(), Duration::ZERO);
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .expect_err("must be onboarded");
        assert!(matches!(err, InitialiseDailyBudgetError::WrongState(_)));
    }

    #[test]
    fn tick_minute_decrements_remaining_and_accumulates_used() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(5 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::Continued);
        assert_eq!(s.time_remaining(), Duration::from_secs(4 * 60));
        assert_eq!(s.user().unwrap().time_used_today(), Duration::from_secs(60));
    }

    #[test]
    fn tick_minute_in_menu_state_works_too() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(5 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::Continued);
        assert_eq!(s.state(), SessionState::Menu);
    }

    #[test]
    fn tick_minute_at_zero_logs_off_with_out_of_time() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::OutOfTime));
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn tick_minute_outside_onboarded_or_menu_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = tick_minute(&mut s).expect_err("must be onboarded/menu");
        assert!(matches!(err, TickMinuteError::WrongState(_)));
    }

    #[test]
    fn tick_minute_saturates_does_not_underflow() {
        // A user with zero per-call limit immediately expires on the
        // first tick.
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::ZERO,
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn force_password_reset_sets_flag_when_expiry_elapsed() {
        let user = alice();
        // alice's password_last_updated is UNIX_EPOCH.
        let mut s = session_at_onboarded_with(user.clone());
        let now = UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        assert!(s.user().unwrap().force_password_reset());
        // The bound clone of alice that we still hold isn't mutated.
        assert!(!user.force_password_reset());
    }

    #[test]
    fn force_password_reset_keeps_flag_when_expiry_not_elapsed() {
        let mut s = session_at_onboarded_with(alice());
        let now = UNIX_EPOCH + Duration::from_secs(3 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_disabled_at_zero_days() {
        let mut s = session_at_onboarded_with(alice());
        // Even far in the future, expiry=0 means "disabled".
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        force_password_reset_if_due(&mut s, 0, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_preserves_flag_already_set_by_sysop() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        // Even with expiry disabled, a pre-set flag survives.
        force_password_reset_if_due(&mut s, 0, UNIX_EPOCH).unwrap();
        assert!(s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_no_op_for_locked_account() {
        let mut user = alice();
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        // Spec: requires not user.account_locked.
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = force_password_reset_if_due(&mut s, 7, SystemTime::UNIX_EPOCH)
            .expect_err("must be onboarded");
        assert!(matches!(err, ForcePasswordResetError::WrongState(_)));
    }

    #[test]
    fn apply_password_match_fires_force_password_reset_when_expired() {
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(60), Duration::from_secs(60));
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        let policy = SessionPolicy::default().with_password_expiry_days(1);
        let now = UNIX_EPOCH + Duration::from_secs(7 * 86_400);
        apply_password_match(&mut s, policy, now).unwrap();
        assert!(s.user().unwrap().force_password_reset());
    }

    #[test]
    fn enter_menu_blocked_when_force_password_reset_set() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("flag should block enter_menu");
        assert!(matches!(err, EnterMenuError::PasswordResetPending));
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.user().unwrap().times_called(), 0);
    }

    #[test]
    fn apply_password_change_replaces_credentials_and_clears_flag() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        let later = UNIX_EPOCH + Duration::from_secs(5_000);
        apply_password_change(
            &mut s,
            "fresh".to_string(),
            Some("freshsalt".to_string()),
            PasswordHashKind::Pbkdf210000,
            later,
        )
        .unwrap();
        let saved = s.user().unwrap();
        assert_eq!(saved.password_hash(), "fresh");
        assert_eq!(saved.password_salt(), Some("freshsalt"));
        assert_eq!(saved.password_last_updated(), later);
        assert!(!saved.force_password_reset());
    }

    #[test]
    fn apply_password_change_unblocks_enter_menu() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        apply_password_change(
            &mut s,
            "fresh".to_string(),
            Some("freshsalt".to_string()),
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.state(), SessionState::Menu);
    }

    #[test]
    fn apply_password_change_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = apply_password_change(
            &mut s,
            "fresh".to_string(),
            None,
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("must be onboarded");
        assert!(matches!(err, CompletePasswordResetError::WrongState(_)));
    }

    #[test]
    fn apply_password_change_without_pending_reset_errors() {
        let mut s = session_at_onboarded_with(alice()); // flag NOT set.
        let err = apply_password_change(
            &mut s,
            "fresh".to_string(),
            None,
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("flag not set");
        assert!(matches!(err, CompletePasswordResetError::ResetNotPending));
    }

    fn user_with_access_level(level: u8) -> User {
        User::new(
            2,
            "alice".to_string(),
            crate::domain::password::PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            level,
        )
        .expect("valid user")
    }

    #[test]
    fn reject_locked_account_transitions_to_logging_off() {
        let mut user = alice();
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        let outcome = s
            .reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("locked user should be rejected");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
        assert_eq!(
            outcome.text,
            "Logon rejected: account locked or below access threshold"
        );
        assert!(!outcome.is_password_failure);
    }

    #[test]
    fn reject_low_access_uses_new_user_rejected_reason() {
        // access_level <= 1 with account_locked == false.
        let mut s = session_at_onboarded_with(user_with_access_level(1));
        let outcome = s
            .reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("low-access user should be rejected");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
        assert!(outcome.text.contains("Logon rejected"));
    }

    #[test]
    fn reject_account_locked_with_low_access_still_uses_locked_account() {
        // Both branches of `is_locked_out`. Spec: account_locked
        // takes precedence in the logoff_reason selector.
        let mut user = user_with_access_level(0);
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        s.reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("locked user should be rejected");
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
    }

    #[test]
    fn reject_no_op_for_normal_user() {
        let mut s = session_at_onboarded_with(alice()); // access 100, not locked.
        let outcome = s.reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH);
        assert!(outcome.is_none());
        assert_eq!(s.state(), SessionState::Onboarded);
        assert!(s.logoff_reason().is_none());
    }

    #[test]
    fn apply_password_match_returns_logon_rejected_for_locked_user() {
        let mut user = alice();
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        let (outcome, rejection) =
            apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::LogonRejected);
        assert!(rejection.is_some());
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
    }

    #[test]
    fn apply_password_match_short_circuits_other_rules_when_rejected() {
        // ForcePasswordReset should not run after a rejection: if it
        // did, the locked user would arrive at finalise with a
        // residual time_remaining set. Confirm time_remaining is
        // still zero (InitialiseDailyBudget didn't run).
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn locked_user_cannot_reach_menu() {
        // LockedAccountsCannotEnterMenu invariant. A locked user
        // who authenticates is bounced into LoggingOff before
        // enter_menu can fire.
        let mut user = alice();
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_ne!(s.state(), SessionState::Menu);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("LoggingOff cannot enter Menu");
        assert!(matches!(err, EnterMenuError::WrongState(_)));
    }

    #[test]
    fn record_input_updates_last_input_at() {
        let mut s = new_session(LogonChannel::Remote);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        s.record_input(later);
        assert_eq!(s.last_input_at(), later);
    }

    #[test]
    fn idle_timeout_from_identifying_uses_carrier_loss_by_default() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn idle_timeout_treat_as_logoff_uses_input_timeout_reason() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_idle_timeout(true).unwrap();
        assert_eq!(s.logoff_reason(), Some(LogoffReason::InputTimeout));
    }

    #[test]
    fn idle_timeout_from_authenticating_allowed() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s.apply_idle_timeout(true).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_onboarded_allowed() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_menu_allowed() {
        let mut s = session_at_onboarded_with(alice());
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_connecting_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .apply_idle_timeout(false)
            .expect_err("connecting not allowed");
        assert!(matches!(
            err,
            IdleTimeoutError::WrongState(SessionState::Connecting)
        ));
    }

    #[test]
    fn idle_timeout_from_logging_off_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        let err = s
            .apply_idle_timeout(false)
            .expect_err("logging_off not allowed");
        assert!(matches!(
            err,
            IdleTimeoutError::WrongState(SessionState::LoggingOff)
        ));
    }

    #[test]
    fn finalise_logoff_after_idle_timeout_writes_reason_to_log_line() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(true).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("input_timeout"),
            "expected input_timeout in goodbye line, got {entry:?}"
        );
    }

    #[test]
    fn carrier_loss_from_connecting_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_identifying_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_authenticating_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_onboarded_transitions_to_logging_off() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_menu_transitions_to_logging_off() {
        let mut s = session_at_onboarded_with(alice());
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_logging_off_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        let err = s
            .apply_carrier_loss()
            .expect_err("logging_off cannot fire CarrierLost again");
        assert!(matches!(
            err,
            CarrierLostError::WrongState(SessionState::LoggingOff)
        ));
    }

    #[test]
    fn carrier_loss_from_ended_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        let err = s
            .apply_carrier_loss()
            .expect_err("ended cannot fire CarrierLost");
        assert!(matches!(
            err,
            CarrierLostError::WrongState(SessionState::Ended)
        ));
    }

    #[test]
    fn finalise_logoff_after_carrier_loss_writes_reason() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_carrier_loss().unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("carrier_loss"),
            "expected carrier_loss in goodbye line, got {entry:?}"
        );
    }

    #[test]
    fn finalise_logoff_after_carrier_loss_treatment_writes_reason_to_log_line() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(false).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("carrier_loss"),
            "expected carrier_loss in goodbye line, got {entry:?}"
        );
    }

    #[test]
    fn finalise_logoff_after_out_of_time_logs_reason() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        tick_minute(&mut s).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("out_of_time"),
            "expected out_of_time in logoff line, got {entry:?}"
        );
    }

    fn make_conf(number: u32) -> Conference {
        use crate::domain::conference::MessageBase;
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    fn make_conf_with_name_type(number: u32, name_type: NameType) -> Conference {
        use crate::domain::conference::MessageBase;
        Conference::with_name_type(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
            name_type,
        )
        .expect("valid")
    }

    fn user_with_grants(grants: &[u32]) -> User {
        let mut user = alice();
        for g in grants {
            user.upsert_membership(crate::domain::conference::ConferenceMembership::new(
                *g, true,
            ));
        }
        user
    }

    #[test]
    fn new_session_has_no_visits() {
        let s = new_session(LogonChannel::Remote);
        assert!(s.visits().is_empty());
        assert!(s.current_visit().is_none());
    }

    #[test]
    fn auto_rejoin_attaches_session_to_first_accessible_conference() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[2]));
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
        let outcome = s.auto_rejoin_conference(&confs, now).expect("ok");
        assert_eq!(
            outcome,
            AutoRejoinOutcome::Joined {
                conference_number: 2,
                msgbase_number: 1,
                show_bulletin: true,
                name_type_promoted_to: None,
            }
        );
        let visit = s.current_visit().expect("open visit");
        assert_eq!(visit.conference_number(), 2);
        assert_eq!(visit.msgbase_number(), 1);
        assert_eq!(visit.joined_at(), now);
        assert_eq!(
            s.user().unwrap().last_joined().unwrap().conference_number(),
            2
        );
    }

    #[test]
    fn auto_rejoin_prefers_users_last_joined_when_still_accessible() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut user = user_with_grants(&[1, 2, 3]);
        user.record_join(&confs[2], &confs[2].msgbases()[0]);
        let mut s = session_at_onboarded_with(user);
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert!(matches!(
            outcome,
            AutoRejoinOutcome::Joined {
                conference_number: 3,
                ..
            }
        ));
    }

    #[test]
    fn auto_rejoin_with_no_grants_moves_to_logging_off_with_no_conference_access() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, AutoRejoinOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
        assert!(s.current_visit().is_none());
    }

    #[test]
    fn auto_rejoin_closes_prior_open_visit_before_attaching_new_one() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t2 = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        s.auto_rejoin_conference(&confs[..1], t1).expect("ok");
        // Force the user to next prefer conf 2 by directly recording a join
        s.phase
            .user_mut()
            .unwrap()
            .record_join(&confs[1], &confs[1].msgbases()[0]);
        s.auto_rejoin_conference(&confs, t2).expect("ok");

        // SessionsHaveAtMostOneOpenVisit: exactly one open visit, the new one.
        let open: Vec<_> = s.visits().iter().filter(|v| v.is_open()).collect();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].conference_number(), 2);

        // The previous visit is closed at t2.
        let closed: Vec<_> = s.visits().iter().filter(|v| !v.is_open()).collect();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].conference_number(), 1);
        assert_eq!(closed[0].left_at(), Some(t2));
    }

    #[test]
    fn auto_rejoin_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn auto_rejoin_clears_show_bulletin_when_session_is_in_quick_logon_mode() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        s.set_quick_logon(true);
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined { show_bulletin, .. } => {
                assert!(
                    !show_bulletin,
                    "quick_logon should suppress the conference bulletin"
                );
            }
            AutoRejoinOutcome::NoAccess => panic!("expected Joined, got NoAccess"),
        }
    }

    #[test]
    fn quick_logon_round_trips_via_setter() {
        let mut s = new_session(LogonChannel::Remote);
        assert!(!s.quick_logon());
        s.set_quick_logon(true);
        assert!(s.quick_logon());
        s.set_quick_logon(false);
        assert!(!s.quick_logon());
    }

    #[test]
    fn explicit_join_attaches_directly_when_user_has_access_to_target() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2, 3]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                msgbase_number,
                matched_request,
                show_bulletin,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(msgbase_number, 1);
                assert!(matched_request);
                assert!(show_bulletin);
            }
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn explicit_join_falls_through_with_matched_request_false_when_no_access_to_target() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                matched_request,
                ..
            } => {
                assert_eq!(conference_number, 1);
                assert!(!matched_request);
            }
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined fallback"),
        }
    }

    #[test]
    fn explicit_join_with_no_grants_anywhere_terminates_session_with_no_conference_access() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, ExplicitJoinOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
    }

    #[test]
    fn explicit_join_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn explicit_join_from_menu_state_is_allowed() {
        // `J` is typed at the menu, not during onboarding. Verify
        // the method accepts the menu state.
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.enter_menu(SystemTime::UNIX_EPOCH).expect("enter menu");
        assert_eq!(s.state(), SessionState::Menu);
        s.explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn explicit_join_clears_show_bulletin_under_quick_logon() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        s.set_quick_logon(true);
        let outcome = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined { show_bulletin, .. } => assert!(!show_bulletin),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn start_conference_scan_attaches_to_first_accessible_conference_and_marks_in_progress() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut s = session_at_onboarded_with(user_with_grants(&[2, 3]));
        let outcome = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ConferenceScanOutcome::Stepped {
                conference_number,
                msgbase_number,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(msgbase_number, 1);
            }
            other => panic!("expected Stepped, got {other:?}"),
        }
        let scan = s.conference_scan().expect("scan in progress");
        // Scan started, with the next-conference pointer parked at the
        // first joined conference (so step picks up after it).
        assert_eq!(scan.next_conference_number(), Some(2));
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn step_conference_scan_advances_through_each_accessible_conference() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3), make_conf(5)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 3, 5]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        // First step jumps from 1 -> 3
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Stepped {
                conference_number, ..
            } => assert_eq!(conference_number, 3),
            other => panic!("expected Stepped, got {other:?}"),
        }
        // Second step jumps to 5
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Stepped {
                conference_number, ..
            } => assert_eq!(conference_number, 5),
            other => panic!("expected Stepped, got {other:?}"),
        }
        // Third step has no more — finishes.
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Finished {
                rejoined_conference,
            } => assert_eq!(rejoined_conference, Some(5)),
            other => panic!("expected Finished, got {other:?}"),
        }
        // Scan slot cleared.
        assert!(s.conference_scan().is_none());
    }

    #[test]
    fn start_conference_scan_with_no_grants_terminates_session() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, ConferenceScanOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
    }

    #[test]
    fn step_conference_scan_without_a_started_scan_errors() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let err = s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("no scan in progress");
        assert!(matches!(err, AutoRejoinError::WrongState(_)));
    }

    #[test]
    fn auto_rejoin_during_active_scan_suppresses_bulletin() {
        // While a scan is in progress, ShowConferenceBulletin is
        // suppressed (`conferences.allium:ShowConferenceBulletin`).
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        // A subsequent auto_rejoin while scan is still in progress
        // must not flag show_bulletin.
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined { show_bulletin, .. } => {
                assert!(
                    !show_bulletin,
                    "scan-in-progress should suppress conference bulletin"
                );
            }
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn start_conference_scan_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn new_session_starts_with_handle_display_name_type() {
        let s = new_session(LogonChannel::Remote);
        assert_eq!(s.display_name_type(), NameType::Handle);
    }

    #[test]
    fn auto_rejoin_into_real_name_conference_promotes_display_name_type() {
        let confs = vec![make_conf_with_name_type(1, NameType::RealName)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, Some(NameType::RealName)),
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::RealName);
    }

    #[test]
    fn auto_rejoin_into_handle_conference_does_not_signal_promotion() {
        // The session already renders as Handle by default; joining
        // a Handle conference is a no-op for display_name_type.
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, None),
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::Handle);
    }

    #[test]
    fn explicit_join_promotes_display_name_type_when_target_uses_internet_names() {
        let confs = vec![
            make_conf(1),
            make_conf_with_name_type(2, NameType::InternetName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, Some(NameType::InternetName)),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::InternetName);
    }

    #[test]
    fn conference_scan_step_promotes_display_name_type_when_visiting_real_name_conf() {
        let confs = vec![
            make_conf(1),
            make_conf_with_name_type(2, NameType::RealName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let stepped = s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        match stepped {
            ConferenceScanOutcome::Stepped {
                conference_number,
                name_type_promoted_to,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(name_type_promoted_to, Some(NameType::RealName));
            }
            other => panic!("expected Stepped, got {other:?}"),
        }
        assert_eq!(s.display_name_type(), NameType::RealName);
    }

    #[test]
    fn rejoining_same_name_type_conference_signals_no_promotion() {
        // After moving to RealName once, rejoining another RealName
        // conference must not flag promotion (the session is already
        // rendering as RealName).
        let confs = vec![
            make_conf_with_name_type(1, NameType::RealName),
            make_conf_with_name_type(2, NameType::RealName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        match outcome {
            ExplicitJoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, None),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn auto_rejoin_finalise_logoff_log_line_includes_no_conference_access_reason() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        s.auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("no_conference_access"),
            "expected no_conference_access in goodbye line, got {entry:?}"
        );
    }
}
