//! [`Session`] entity (spec: `session.allium:Session`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop reads.
//! Presentation booleans, time accounting, temp access, reserved-for
//! and the `new_user_registering` branch arrive in their owning
//! slices.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::domain::caller_log::CallerLog;
use crate::domain::password::{PasswordError, PasswordHashKind};
use crate::domain::user::User;

/// Maximum number of unknown handle entries before a session is ended.
const MAX_NAME_RETRIES: u32 = 5;

/// Default consecutive bad-password attempts before a session ends or
/// account lockout applies.
const DEFAULT_MAX_PASSWORD_FAILURES: u32 = 3;

/// Default offset past midnight UTC used by
/// [`SessionPolicy::new`] when no explicit value is supplied. Mirrors
/// the legacy AmiExpress constant `21600` seconds (six hours) at
/// `amiexpress/express.e:529`.
const DEFAULT_DAILY_RESET_OFFSET: Duration = Duration::from_secs(6 * 3_600);

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
    /// The session burned through `time_remaining` while in
    /// `onboarded` or `menu`. Set by
    /// `session.allium:TimeExpired` (Slice 14).
    OutOfTime,
}

/// Policy decision after a password failure has been recorded on a
/// session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordFailureDecision {
    /// Keep the session in password authentication.
    Continue,
    /// End the session because the session-level failure limit was
    /// reached.
    EndSession,
    /// Lock the user's account because the user-level failure limit
    /// was reached.
    LockAccount,
}

/// Domain policy values that influence a session's behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    max_password_failures: u32,
    daily_reset_offset: Duration,
    password_expiry_days: u32,
    min_password_length: u32,
    min_password_categories: u32,
}

impl SessionPolicy {
    /// Constructs a session policy with the spec-default
    /// [`Self::daily_reset_offset`] of six hours, expiry disabled and
    /// no password-strength enforcement.
    ///
    /// # Parameters
    /// - `max_password_failures`: the number of consecutive bad
    ///   password attempts that ends the session or locks the account.
    ///
    /// # Returns
    /// A [`SessionPolicy`] carrying the supplied password-failure
    /// limit, a six-hour daily reset offset, and zeros for the
    /// password-expiry / strength knobs (Slice 15 disables them by
    /// default).
    pub fn new(max_password_failures: u32) -> Self {
        Self {
            max_password_failures,
            daily_reset_offset: DEFAULT_DAILY_RESET_OFFSET,
            password_expiry_days: 0,
            min_password_length: 0,
            min_password_categories: 0,
        }
    }

    /// Returns a copy of `self` with [`Self::daily_reset_offset`]
    /// replaced by `offset`.
    pub fn with_daily_reset_offset(mut self, offset: Duration) -> Self {
        self.daily_reset_offset = offset;
        self
    }

    /// Returns the configured daily reset offset (Slice 14).
    pub fn daily_reset_offset(&self) -> Duration {
        self.daily_reset_offset
    }

    /// Returns a copy of `self` with [`Self::password_expiry_days`]
    /// replaced by `days`. `0` disables expiry.
    pub fn with_password_expiry_days(mut self, days: u32) -> Self {
        self.password_expiry_days = days;
        self
    }

    /// Returns the configured password expiry, in days (Slice 15).
    /// `0` disables expiry.
    pub fn password_expiry_days(&self) -> u32 {
        self.password_expiry_days
    }

    /// Returns a copy of `self` with [`Self::min_password_length`]
    /// replaced by `length`. `0` disables the length check.
    pub fn with_min_password_length(mut self, length: u32) -> Self {
        self.min_password_length = length;
        self
    }

    /// Returns the configured minimum password length (Slice 15).
    /// `0` disables the length check.
    pub fn min_password_length(&self) -> u32 {
        self.min_password_length
    }

    /// Returns a copy of `self` with [`Self::min_password_categories`]
    /// replaced by `categories`. `0` disables the category check;
    /// values above `4` are treated as `4`.
    pub fn with_min_password_categories(mut self, categories: u32) -> Self {
        self.min_password_categories = categories;
        self
    }

    /// Returns the configured minimum password categories (Slice 15).
    /// `0` disables the category check.
    pub fn min_password_categories(&self) -> u32 {
        self.min_password_categories
    }

    /// Decides what should happen after a password failure has been
    /// recorded on `session`.
    ///
    /// The account-level lockout decision wins over the session-level
    /// end decision when both counters have reached the configured
    /// limit.
    ///
    /// # Parameters
    /// - `session`: the session whose password-failure counters should
    ///   be assessed.
    ///
    /// # Returns
    /// A [`PasswordFailureDecision`] describing whether the session
    /// may continue, should end, or should lock the bound account.
    pub fn password_failure_decision(&self, session: &Session) -> PasswordFailureDecision {
        let user_failures = session
            .user()
            .map(|user| user.invalid_attempts())
            .unwrap_or_default();
        if user_failures >= self.max_password_failures {
            PasswordFailureDecision::LockAccount
        } else if session.password_retry_count() >= self.max_password_failures {
            PasswordFailureDecision::EndSession
        } else {
            PasswordFailureDecision::Continue
        }
    }
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PASSWORD_FAILURES)
    }
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
    time_remaining: Duration,
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
            time_remaining: Duration::ZERO,
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

    /// Returns how much per-call time the session has left.
    ///
    /// Set on the `authenticating -> onboarded` transition by
    /// [`Session::initialise_daily_budget`] and decremented each minute
    /// by [`Session::tick_minute`]. Slice 14.
    pub fn time_remaining(&self) -> Duration {
        self.time_remaining
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
    /// [`SessionState::Onboarded`],
    /// [`EnterMenuError::UserMissing`] when no user is bound, or
    /// [`EnterMenuError::PasswordResetPending`] when the bound
    /// user has `force_password_reset` set (Slice 15).
    pub fn enter_menu(&mut self, now: SystemTime) -> Result<CallerLog, EnterMenuError> {
        if self.state != SessionState::Onboarded {
            return Err(EnterMenuError::WrongState(self.state));
        }
        let user = self.user.as_mut().ok_or(EnterMenuError::UserMissing)?;
        if user.force_password_reset() {
            return Err(EnterMenuError::PasswordResetPending);
        }
        user.bump_times_called();
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
    /// transitions to [`SessionState::Onboarded`], then fires the
    /// `state becomes onboarded` rule cluster via
    /// [`Session::on_enter_onboarded`].
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
    /// not in [`SessionState::Authenticating`], or
    /// [`VerifyPasswordError::UserMissing`] if no user is bound.
    pub fn apply_password_match(
        &mut self,
        policy: SessionPolicy,
        now: SystemTime,
    ) -> Result<(VerifyPasswordOutcome, Option<CallerLog>), VerifyPasswordError> {
        if self.state != SessionState::Authenticating {
            return Err(VerifyPasswordError::WrongState(self.state));
        }
        let user_mut = self.user.as_mut().ok_or(VerifyPasswordError::UserMissing)?;
        user_mut.clear_invalid_attempts();
        self.authenticated_at = Some(now);
        self.transition_to(SessionState::Onboarded)
            .expect("authenticating -> onboarded is permitted");
        let rejection = self.on_enter_onboarded(policy, now);
        let outcome = if rejection.is_some() {
            VerifyPasswordOutcome::LogonRejected
        } else {
            VerifyPasswordOutcome::Authenticated
        };
        Ok((outcome, rejection))
    }

    /// Fires every spec rule whose `when` clause is the transition
    /// into [`SessionState::Onboarded`].
    ///
    /// Called by every code path that drives a session into
    /// `Onboarded`: [`Session::apply_password_match`] today; later,
    /// new-user registration (Slice 20), sysop direct logon (Slice 22)
    /// and local logon (Slice 23). Rules fire in spec order:
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
    fn on_enter_onboarded(&mut self, policy: SessionPolicy, now: SystemTime) -> Option<CallerLog> {
        assert_eq!(
            self.state,
            SessionState::Onboarded,
            "on_enter_onboarded called outside Onboarded state"
        );
        assert!(
            self.user.is_some(),
            "on_enter_onboarded called without a bound user"
        );
        if let Some(entry) = self.reject_locked_or_insufficient_access(now) {
            return Some(entry);
        }
        self.initialise_daily_budget(now, policy.daily_reset_offset())
            .expect("guards hold immediately after transition to Onboarded");
        self.force_password_reset_if_due(policy.password_expiry_days(), now)
            .expect("guards hold immediately after transition to Onboarded");
        None
    }

    /// `session.allium:RejectLockedOrInsufficientAccess` rule
    /// (Slice 16).
    ///
    /// When the bound user is locked out (account_locked or
    /// access_level <= 1), transitions the session to
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
            self.state,
            SessionState::Onboarded,
            "reject_locked_or_insufficient_access called outside Onboarded"
        );
        let user = self
            .user
            .as_ref()
            .expect("reject_locked_or_insufficient_access without bound user");
        if !user.is_locked_out() {
            return None;
        }
        let reason = if user.is_account_locked() {
            LogoffReason::LockedAccount
        } else {
            LogoffReason::NewUserRejected
        };
        self.transition_to(SessionState::LoggingOff)
            .expect("onboarded -> logging_off is permitted");
        self.logoff_reason = Some(reason);
        Some(CallerLog {
            session_node: self.node_number,
            at: now,
            text: "Logon rejected: account locked or below access threshold".to_string(),
            is_password_failure: false,
        })
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
    /// session is not in [`SessionState::Onboarded`], or
    /// [`ForcePasswordResetError::UserMissing`] when no user is
    /// bound.
    pub fn force_password_reset_if_due(
        &mut self,
        password_expiry_days: u32,
        now: SystemTime,
    ) -> Result<(), ForcePasswordResetError> {
        if self.state != SessionState::Onboarded {
            return Err(ForcePasswordResetError::WrongState(self.state));
        }
        let user = self
            .user
            .as_mut()
            .ok_or(ForcePasswordResetError::UserMissing)?;
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
    /// Replaces the user's stored credentials with the freshly
    /// computed `(hash, salt, kind)` triple, sets
    /// `password_last_updated = now`, and clears
    /// `force_password_reset`. The strength check and
    /// "differs-from-old" check are the caller's responsibility (see
    /// `app::session_flow::complete_password_reset`).
    ///
    /// # Errors
    /// Returns [`CompletePasswordResetError::WrongState`] when the
    /// session is not in [`SessionState::Onboarded`],
    /// [`CompletePasswordResetError::UserMissing`] when no user is
    /// bound, or [`CompletePasswordResetError::ResetNotPending`]
    /// when the bound user does not have `force_password_reset`
    /// set.
    pub fn apply_password_change(
        &mut self,
        hash: String,
        salt: Option<String>,
        kind: PasswordHashKind,
        now: SystemTime,
    ) -> Result<(), CompletePasswordResetError> {
        if self.state != SessionState::Onboarded {
            return Err(CompletePasswordResetError::WrongState(self.state));
        }
        let user = self
            .user
            .as_mut()
            .ok_or(CompletePasswordResetError::UserMissing)?;
        if !user.force_password_reset() {
            return Err(CompletePasswordResetError::ResetNotPending);
        }
        user.record_password_change(hash, salt, kind, now);
        Ok(())
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
    /// not in [`SessionState::Authenticating`], or
    /// [`VerifyPasswordError::UserMissing`] if no user is bound.
    pub fn apply_password_mismatch(
        &mut self,
        policy: SessionPolicy,
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

        let outcome = match policy.password_failure_decision(self) {
            PasswordFailureDecision::LockAccount => {
                self.user.as_mut().expect("user present").lock_account();
                self.transition_to(SessionState::LoggingOff)
                    .expect("authenticating -> logging_off is permitted");
                self.logoff_reason = Some(LogoffReason::LockedAccount);
                VerifyPasswordOutcome::AccountLocked
            }
            PasswordFailureDecision::EndSession => {
                self.transition_to(SessionState::LoggingOff)
                    .expect("authenticating -> logging_off is permitted");
                self.logoff_reason = Some(LogoffReason::ExcessivePasswordFails);
                VerifyPasswordOutcome::TooManyFailures
            }
            PasswordFailureDecision::Continue => VerifyPasswordOutcome::NotMatching,
        };
        Ok((outcome, entry))
    }

    /// `session.allium:InitialiseDailyBudget` rule (Slice 14).
    ///
    /// Fires once the session has reached
    /// [`SessionState::Onboarded`]. If `now` falls in a different
    /// accounting day from the user's previous `last_call`, the daily
    /// counters reset; otherwise `times_called_today` increments.
    /// `time_remaining` is then set to `user.time_limit_per_call`.
    ///
    /// The accounting day boundary is `daily_reset_offset` past
    /// midnight UTC (the legacy AmiExpress default is six hours, so
    /// the day rolls over at 06:00 UTC).
    ///
    /// # Errors
    /// Returns [`InitialiseDailyBudgetError::WrongState`] when the
    /// session is not in [`SessionState::Onboarded`], or
    /// [`InitialiseDailyBudgetError::UserMissing`] when no user is
    /// bound.
    pub fn initialise_daily_budget(
        &mut self,
        now: SystemTime,
        daily_reset_offset: Duration,
    ) -> Result<(), InitialiseDailyBudgetError> {
        if self.state != SessionState::Onboarded {
            return Err(InitialiseDailyBudgetError::WrongState(self.state));
        }
        let user = self
            .user
            .as_mut()
            .ok_or(InitialiseDailyBudgetError::UserMissing)?;

        let today = floor_to_day(now, daily_reset_offset);
        let last_call_day = user
            .last_call()
            .map(|t| floor_to_day(t, daily_reset_offset));
        let is_new_day = last_call_day.is_none_or(|d| d != today);

        if is_new_day {
            user.reset_daily_counters();
        } else {
            user.bump_times_called_today();
        }
        self.time_remaining = user.time_limit_per_call();
        Ok(())
    }

    /// `session.allium:UpdateTimeUsed` + `TimeExpired` rules (Slice 14).
    ///
    /// Decrements [`Self::time_remaining`] by one minute (saturating at
    /// zero) and accumulates the same minute against
    /// `user.time_used_today`. If `time_remaining` reaches zero the
    /// session transitions to [`SessionState::LoggingOff`] with
    /// [`LogoffReason::OutOfTime`].
    ///
    /// # Errors
    /// Returns [`TickMinuteError::WrongState`] when the session is not
    /// in [`SessionState::Onboarded`] or [`SessionState::Menu`], or
    /// [`TickMinuteError::UserMissing`] when no user is bound.
    pub fn tick_minute(&mut self) -> Result<TickMinuteOutcome, TickMinuteError> {
        if !matches!(self.state, SessionState::Onboarded | SessionState::Menu) {
            return Err(TickMinuteError::WrongState(self.state));
        }
        let user = self.user.as_mut().ok_or(TickMinuteError::UserMissing)?;
        user.add_time_used_today(Duration::from_secs(60));
        self.time_remaining = self.time_remaining.saturating_sub(Duration::from_secs(60));
        if self.time_remaining.is_zero() {
            self.transition_to(SessionState::LoggingOff)
                .expect("onboarded/menu -> logging_off is permitted");
            self.logoff_reason = Some(LogoffReason::OutOfTime);
            Ok(TickMinuteOutcome::TimeExpired)
        } else {
            Ok(TickMinuteOutcome::Continued)
        }
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
    /// Credentials matched, but
    /// `session.allium:RejectLockedOrInsufficientAccess` (Slice 16)
    /// short-circuited the post-auth rule cluster: the user's
    /// account was already locked or below the minimum access tier.
    /// The session has moved to [`SessionState::LoggingOff`] with
    /// [`LogoffReason::LockedAccount`] or
    /// [`LogoffReason::NewUserRejected`].
    LogonRejected,
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
    /// The bound user has `force_password_reset` set; the listener
    /// must run the password-change sub-flow before retrying
    /// (`session.allium:CompletePasswordReset`, Slice 15).
    PasswordResetPending,
}

impl std::fmt::Display for EnterMenuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "enter_menu in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "enter_menu called without a bound user"),
            Self::PasswordResetPending => write!(
                f,
                "enter_menu blocked: user must complete a forced password reset"
            ),
        }
    }
}

impl std::error::Error for EnterMenuError {}

/// Errors returned by [`Session::initialise_daily_budget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialiseDailyBudgetError {
    /// The session is not in [`SessionState::Onboarded`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
}

impl std::fmt::Display for InitialiseDailyBudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "initialise_daily_budget in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "initialise_daily_budget called without a bound user"),
        }
    }
}

impl std::error::Error for InitialiseDailyBudgetError {}

/// Errors returned by [`Session::force_password_reset_if_due`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcePasswordResetError {
    /// The session is not in [`SessionState::Onboarded`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
}

impl std::fmt::Display for ForcePasswordResetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => {
                write!(f, "force_password_reset_if_due in unexpected state: {s:?}")
            }
            Self::UserMissing => {
                write!(f, "force_password_reset_if_due called without a bound user")
            }
        }
    }
}

impl std::error::Error for ForcePasswordResetError {}

/// Errors returned by [`Session::apply_password_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletePasswordResetError {
    /// The session is not in [`SessionState::Onboarded`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
    /// The bound user does not have `force_password_reset` set, so
    /// `CompletePasswordReset` doesn't apply.
    ResetNotPending,
}

impl std::fmt::Display for CompletePasswordResetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "apply_password_change in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "apply_password_change called without a bound user"),
            Self::ResetNotPending => write!(
                f,
                "apply_password_change called when force_password_reset is not set"
            ),
        }
    }
}

impl std::error::Error for CompletePasswordResetError {}

/// Outcome of [`Session::tick_minute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickMinuteOutcome {
    /// The session has time left and remains in `onboarded` or `menu`.
    Continued,
    /// `time_remaining` has reached zero. The session has moved to
    /// [`SessionState::LoggingOff`] with [`LogoffReason::OutOfTime`].
    TimeExpired,
}

/// Errors returned by [`Session::tick_minute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickMinuteError {
    /// The session is not in [`SessionState::Onboarded`] or
    /// [`SessionState::Menu`].
    WrongState(SessionState),
    /// No user is bound to the session.
    UserMissing,
}

impl std::fmt::Display for TickMinuteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongState(s) => write!(f, "tick_minute in unexpected state: {s:?}"),
            Self::UserMissing => write!(f, "tick_minute called without a bound user"),
        }
    }
}

impl std::error::Error for TickMinuteError {}

/// `session.allium:floor_to_day` black-box helper.
///
/// Buckets `at` into a day index where the boundary sits
/// `offset` past midnight UTC. The legacy AmiExpress equivalent is
/// `Div(currTime - 21600, 86400)` (six-hour offset) — see
/// `amiexpress/express.e:529`.
fn floor_to_day(at: SystemTime, offset: Duration) -> i64 {
    let secs = match at.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let offset_secs = offset.as_secs() as i64;
    (secs - offset_secs).div_euclid(86_400)
}

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
        Some(LogoffReason::OutOfTime) => "out_of_time",
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
            .apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
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
        let (outcome, rejection) = s
            .apply_password_match(SessionPolicy::default(), now)
            .unwrap();
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
        s.user.as_mut().unwrap().bump_invalid_attempts();
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_match_fires_initialise_daily_budget() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        s.record_identified_user("alice", user).unwrap();
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
        // InitialiseDailyBudget consequent of the transition.
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn verify_password_mismatch_bumps_counters() {
        let mut s = authenticated_session();
        let (outcome, entry) = s
            .apply_password_mismatch(SessionPolicy::new(3), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        s.password_retry_count = 1;
        s.user.as_mut().unwrap().bump_invalid_attempts();

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::Continue
        );
    }

    #[test]
    fn session_policy_locks_account_when_user_failures_reach_limit() {
        let mut s = authenticated_session();
        s.password_retry_count = 3;
        for _ in 0..3 {
            s.user.as_mut().unwrap().bump_invalid_attempts();
        }

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::LockAccount
        );
    }

    #[test]
    fn session_policy_ends_session_when_session_failures_reach_limit() {
        let mut s = authenticated_session();
        s.password_retry_count = 3;

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::EndSession
        );
    }

    #[test]
    fn verify_password_locks_account_when_user_attempts_reach_max() {
        let mut s = authenticated_session();
        let (outcome, _entry) = s
            .apply_password_mismatch(SessionPolicy::new(1), SystemTime::UNIX_EPOCH)
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
        s.apply_password_mismatch(SessionPolicy::new(5), SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_password_mismatch(SessionPolicy::new(5), SystemTime::UNIX_EPOCH)
            .unwrap();
        // Simulate an out-of-band reset of the user-level counter.
        s.user.as_mut().unwrap().clear_invalid_attempts();
        let (outcome, _entry) = s
            .apply_password_mismatch(SessionPolicy::new(3), SystemTime::UNIX_EPOCH)
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
            .apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
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
        s.transition_to(SessionState::Identifying).unwrap();
        s.user = Some(user);
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
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
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
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
        s.initialise_daily_budget(later_today, DAILY_RESET_OFFSET)
            .unwrap();
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
        s.initialise_daily_budget(today, DAILY_RESET_OFFSET)
            .unwrap();
        assert_eq!(s.user().unwrap().times_called_today(), 0);
        assert_eq!(s.user().unwrap().time_used_today(), Duration::ZERO);
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .expect_err("must be onboarded");
        assert!(matches!(err, InitialiseDailyBudgetError::WrongState(_)));
    }

    #[test]
    fn initialise_daily_budget_without_user_errors() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
        let err = s
            .initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .expect_err("user missing");
        assert!(matches!(err, InitialiseDailyBudgetError::UserMissing));
    }

    #[test]
    fn tick_minute_decrements_remaining_and_accumulates_used() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(5 * 60),
            Duration::from_secs(60 * 60),
        ));
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
        let outcome = s.tick_minute().unwrap();
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
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        let outcome = s.tick_minute().unwrap();
        assert_eq!(outcome, TickMinuteOutcome::Continued);
        assert_eq!(s.state(), SessionState::Menu);
    }

    #[test]
    fn tick_minute_at_zero_logs_off_with_out_of_time() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
        let outcome = s.tick_minute().unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::OutOfTime));
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn tick_minute_outside_onboarded_or_menu_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s.tick_minute().expect_err("must be onboarded/menu");
        assert!(matches!(err, TickMinuteError::WrongState(_)));
    }

    #[test]
    fn tick_minute_without_user_errors() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
        let err = s.tick_minute().expect_err("user missing");
        assert!(matches!(err, TickMinuteError::UserMissing));
    }

    #[test]
    fn tick_minute_saturates_does_not_underflow() {
        // A user with zero per-call limit immediately expires on the
        // first tick.
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::ZERO,
            Duration::from_secs(60 * 60),
        ));
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
        let outcome = s.tick_minute().unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn force_password_reset_sets_flag_when_expiry_elapsed() {
        let user = alice();
        // alice's password_last_updated is UNIX_EPOCH.
        let mut s = session_at_onboarded_with(user.clone());
        let now = UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        s.force_password_reset_if_due(7, now).unwrap();
        assert!(s.user().unwrap().force_password_reset());
        // The bound clone of alice that we still hold isn't mutated.
        assert!(!user.force_password_reset());
    }

    #[test]
    fn force_password_reset_keeps_flag_when_expiry_not_elapsed() {
        let mut s = session_at_onboarded_with(alice());
        let now = UNIX_EPOCH + Duration::from_secs(3 * 86_400);
        s.force_password_reset_if_due(7, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_disabled_at_zero_days() {
        let mut s = session_at_onboarded_with(alice());
        // Even far in the future, expiry=0 means "disabled".
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        s.force_password_reset_if_due(0, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_preserves_flag_already_set_by_sysop() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        // Even with expiry disabled, a pre-set flag survives.
        s.force_password_reset_if_due(0, UNIX_EPOCH).unwrap();
        assert!(s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_no_op_for_locked_account() {
        let mut user = alice();
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        s.force_password_reset_if_due(7, now).unwrap();
        // Spec: requires not user.account_locked.
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .force_password_reset_if_due(7, SystemTime::UNIX_EPOCH)
            .expect_err("must be onboarded");
        assert!(matches!(err, ForcePasswordResetError::WrongState(_)));
    }

    #[test]
    fn force_password_reset_without_user_errors() {
        let mut s = new_session(LogonChannel::Remote);
        s.transition_to(SessionState::Identifying).unwrap();
        s.transition_to(SessionState::Authenticating).unwrap();
        s.transition_to(SessionState::Onboarded).unwrap();
        let err = s
            .force_password_reset_if_due(7, SystemTime::UNIX_EPOCH)
            .expect_err("user missing");
        assert!(matches!(err, ForcePasswordResetError::UserMissing));
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
        s.apply_password_match(policy, now).unwrap();
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
        s.apply_password_change(
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
        s.apply_password_change(
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
        let err = s
            .apply_password_change(
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
        let err = s
            .apply_password_change(
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
        let (outcome, rejection) = s
            .apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_ne!(s.state(), SessionState::Menu);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("LoggingOff cannot enter Menu");
        assert!(matches!(err, EnterMenuError::WrongState(_)));
    }

    #[test]
    fn finalise_logoff_after_out_of_time_logs_reason() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        s.initialise_daily_budget(SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .unwrap();
        s.tick_minute().unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("out_of_time"),
            "expected out_of_time in logoff line, got {entry:?}"
        );
    }
}
