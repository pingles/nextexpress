//! [`Session`] entity (spec: `session.allium:Session`).
//!
//! This module keeps the aggregate state shape and public re-exports.
//! Capability-specific transition rules live in sibling modules such
//! as `identity`, `registration`, `lifecycle`, `conferencing`,
//! `budget`, and `lockout`.

use std::time::{Duration, SystemTime};

use crate::domain::conference::NameType;
use crate::domain::files::flagged::FlaggedFiles;
use crate::domain::user::{PersistedUser, User, UserPatch};

mod budget;
mod call;
mod conference_activity;
mod conferencing;
mod errors;
mod identity;
mod lifecycle;
mod lockout;
mod log_format;
mod outcomes;
mod registration;
mod transitions;
pub(crate) mod typed;

#[cfg(test)]
mod tests;

use call::{AuthenticatedCall, CallSalvage};
use conference_activity::ConferenceActivity;

pub use crate::domain::session_policy::{PasswordFailureDecision, SessionPolicy};
pub use budget::{daily_budget_outcome, initialise_daily_budget, tick_minute};
pub use call::CallId;
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
    /// Per-session flagged-file set (slice D2f); held outside
    /// [`SessionPhase`] so it survives transitions. Slice D5 will
    /// persist it.
    flagged_files: FlaggedFiles,
    /// Bound user's state as last persisted: set when a user binds,
    /// refreshed by [`Session::rebaseline_persisted`] after every
    /// repository command-write. [`Session::pending_user_patch`] diffs
    /// the live user against it. Held outside [`SessionPhase`] so it
    /// survives transitions.
    persist_baseline: Option<Box<PersistedUser>>,
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
    /// `session.allium:Session.quiet_mode` (Tier A quickwin A9).
    /// When `true` the session suppresses inter-node OLM broadcasts
    /// and join announcements; the Q menu command toggles it.
    /// Seeded `false` by `AcceptConnection` and `SysopDirectLogon`
    /// per the spec.
    quiet_mode: bool,
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
        call: AuthenticatedCall,
    },
    Menu {
        call: AuthenticatedCall,
    },
    LoggingOff {
        call: CallSalvage,
        reason: Option<LogoffReason>,
    },
    Ended {
        call: CallSalvage,
        reason: Option<LogoffReason>,
        logoff_at: Option<SystemTime>,
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
            Self::Authenticating { user, .. } => Some(user),
            Self::Onboarded { call } | Self::Menu { call } => Some(&call.user),
            Self::LoggingOff { call, .. } | Self::Ended { call, .. } => call.user(),
            Self::Connecting | Self::Identifying { .. } | Self::NewUserRegistering { .. } => None,
        }
    }

    fn user_mut(&mut self) -> Option<&mut User> {
        match self {
            Self::Authenticating { user, .. } => Some(user),
            Self::Onboarded { call } | Self::Menu { call } => Some(&mut call.user),
            Self::LoggingOff { call, .. } | Self::Ended { call, .. } => call.user_mut(),
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
            Self::Onboarded { call } | Self::Menu { call } => Some(call.authenticated_at),
            Self::LoggingOff { call, .. } | Self::Ended { call, .. } => call.authenticated_at(),
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

    fn call_id(&self) -> Option<CallId> {
        match self {
            Self::Onboarded { call } | Self::Menu { call } => Some(call.call_id),
            Self::LoggingOff { call, .. } | Self::Ended { call, .. } => call.call_id(),
            Self::Connecting
            | Self::Identifying { .. }
            | Self::Authenticating { .. }
            | Self::NewUserRegistering { .. } => None,
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
            Self::Onboarded { call } | Self::Menu { call } => call.time_remaining,
            Self::LoggingOff { call, .. } | Self::Ended { call, .. } => call.time_remaining(),
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
                quiet_mode: false,
                display_name_type: NameType::Handle,
            },
            phase: SessionPhase::Connecting,
            activity: ConferenceActivity::new(),
            flagged_files: FlaggedFiles::default(),
            persist_baseline: None,
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

    /// Returns whether the session is in quiet mode, suppressing
    /// inter-node OLM broadcasts and join announcements. Mirrors
    /// `session.allium:Session.quiet_mode` (Tier A quickwin A9).
    #[must_use]
    pub fn quiet_mode(&self) -> bool {
        self.shared.quiet_mode
    }

    /// Sets [`Self::quiet_mode`]. The `Q` menu command toggles it
    /// through [`crate::domain::session::typed::MenuSession::toggle_quiet_mode`].
    pub fn set_quiet_mode(&mut self, quiet: bool) {
        self.shared.quiet_mode = quiet;
    }

    /// The session's flagged-file set (slice D2f; D5 persists). The
    /// `F`/`R` pager verbs flag listed files into it, and the lister
    /// reborrows it immutably to mark flagged rows.
    pub(crate) fn flagged_files_mut(&mut self) -> &mut FlaggedFiles {
        &mut self.flagged_files
    }

    /// The session's flagged-file set, read-only — the `A` listing
    /// (slice D6a) reads it.
    pub(crate) fn flagged_files(&self) -> &FlaggedFiles {
        &self.flagged_files
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

    /// Refreshes the persisted-state baseline to the bound user's
    /// current state. The application layer calls this after every
    /// successful repository write so [`Session::pending_user_patch`]
    /// yields only not-yet-persisted changes. No-op when no user is
    /// bound.
    pub fn rebaseline_persisted(&mut self) {
        self.persist_baseline = self.phase.user().map(|u| Box::new(u.to_persisted()));
    }

    /// Returns the bound user's slot number and the [`UserPatch`] of
    /// changes since the last (re)baseline, or `None` when no user is
    /// bound.
    ///
    /// Binding a user (identify or registration) establishes the
    /// baseline, so a bound user always has one on production paths —
    /// a bound user without a baseline is a programmer error (caught
    /// by a debug assertion).
    #[must_use]
    pub fn pending_user_patch(&self) -> Option<(u32, UserPatch)> {
        let user = self.phase.user()?;
        debug_assert!(
            self.persist_baseline.is_some(),
            "user bound without a persist baseline"
        );
        let baseline = self.persist_baseline.as_ref()?;
        let current = user.to_persisted();
        Some((current.slot_number, UserPatch::between(baseline, &current)))
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

    /// Returns the opaque identity of this call, stamped when
    /// authentication completed. `None` before authentication and for
    /// sessions that never authenticated; survives teardown so the
    /// logoff path can persist it (the D-T1 transfer ledger's
    /// `call_id` column).
    #[must_use]
    pub fn call_id(&self) -> Option<CallId> {
        self.phase.call_id()
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
        self.phase = SessionPhase::LoggingOff {
            call: CallSalvage::from_phase(previous),
            reason,
        };
    }

    fn move_to_ended(&mut self, logoff_at: Option<SystemTime>) {
        let previous = std::mem::replace(&mut self.phase, SessionPhase::Connecting);
        // Teardown phases keep their recorded reason; ending straight
        // from an active phase records none.
        let (call, reason) = match previous {
            SessionPhase::LoggingOff { call, reason }
            | SessionPhase::Ended { call, reason, .. } => (call, reason),
            other => (CallSalvage::from_phase(other), None),
        };
        self.phase = SessionPhase::Ended {
            call,
            reason,
            logoff_at,
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
}
