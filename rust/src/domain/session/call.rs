//! Phase payload types for the `SessionPhase` variants.
//!
//! [`AuthenticatedCall`] groups the state that exists from successful
//! authentication until teardown. The authenticated phases carry it
//! whole and [`CallSalvage`] preserves it through `LoggingOff`/`Ended`,
//! so adding a per-call field is a single-site change instead of an
//! edit to every `SessionPhase` variant and salvage match.
//! [`AuthenticatingAttempt`] carries the pre-authentication payload
//! the same way (item 23a's pattern applied to `Authenticating`).

use std::fmt;
use std::time::{Duration, SystemTime};

use crate::domain::user::{DailyBudgetOutcome, User};

use super::{NewUserPasswordOutcome, SessionPhase};

/// Opaque, durable identity of one authenticated call.
///
/// Created by the application layer after successful authentication
/// (`designs/FILES.md`: the transfer ledger persists this value; node
/// numbers are never durable call identity). Like `now`, the value is
/// an input to the domain rules — entropy comes from the caller so the
/// rules stay deterministic under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallId(u128);

impl CallId {
    /// Wraps a caller-supplied 128-bit value (production callers pass
    /// freshly generated random bits).
    ///
    /// # Parameters
    /// - `value`: the identity bits; the domain never inspects them.
    ///
    /// # Returns
    /// The wrapped identifier.
    #[must_use]
    pub fn new(value: u128) -> Self {
        Self(value)
    }
}

impl fmt::Display for CallId {
    /// Renders the persistence TEXT form: 32 lowercase hex digits,
    /// zero-padded.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// State that exists from successful authentication to teardown.
///
/// Carried whole by `SessionPhase::Onboarded` and `SessionPhase::Menu`,
/// and salvaged into [`CallSalvage::Authenticated`] when the session
/// begins logging off — every transition moves the struct as one value.
#[derive(Debug, Clone)]
pub(super) struct AuthenticatedCall {
    /// The call's durable identity, stamped at authentication.
    pub(super) call_id: CallId,
    /// The authenticated user bound to this call.
    pub(super) user: User,
    /// When authentication completed.
    pub(super) authenticated_at: SystemTime,
    /// Per-call time budget: set by `initialise_daily_budget`,
    /// decremented by [`Self::accrue_elapsed`].
    pub(super) time_remaining: Duration,
    /// The instant the per-call budget was last accounted against.
    /// Wall-clock elapsed since here is what [`Self::accrue_elapsed`]
    /// converts into consumed minutes; it advances by whole minutes so
    /// a sub-minute remainder carries to the next accrual.
    pub(super) last_tick_at: SystemTime,
}

impl AuthenticatedCall {
    /// Binds `user` to a freshly authenticated call.
    ///
    /// # Parameters
    /// - `call_id`: the call's durable identity, stamped at
    ///   authentication.
    /// - `user`: the authenticated user the call carries.
    /// - `authenticated_at`: when authentication completed.
    ///
    /// # Returns
    /// A call whose `time_remaining` starts at [`Duration::ZERO`] —
    /// the budget is not an authentication concern, so it stays zero
    /// until `session.allium:InitialiseDailyBudget` resets it via
    /// [`Self::reset_time_budget`].
    #[must_use]
    pub(super) fn new(call_id: CallId, user: User, authenticated_at: SystemTime) -> Self {
        Self {
            call_id,
            user,
            authenticated_at,
            time_remaining: Duration::ZERO,
            last_tick_at: authenticated_at,
        }
    }

    /// `session.allium:InitialiseDailyBudget` (Slice 14): rolls the
    /// bound user's daily counters across the accounting-day boundary
    /// and resets the per-call time budget.
    ///
    /// Delegates the day-boundary decision to
    /// [`super::budget::daily_budget_outcome`] — the same function the
    /// `record_auth_outcome` persistence command uses — so the
    /// in-session mutation and the persisted command cannot drift.
    /// Note the legacy quirk the outcome documents: a new day *resets*
    /// the counters without bumping `times_called_today`.
    ///
    /// # Parameters
    /// - `now`: the current logon time.
    /// - `daily_reset_offset`: how far past midnight UTC the
    ///   accounting day rolls over (legacy default: six hours).
    ///
    /// # Returns
    /// The [`DailyBudgetOutcome`] the rule applied — threaded back to
    /// the caller so the `record_auth_outcome` persistence command
    /// carries the very decision that mutated the counters.
    pub(super) fn begin_daily_budget(
        &mut self,
        now: SystemTime,
        daily_reset_offset: Duration,
    ) -> DailyBudgetOutcome {
        let outcome =
            super::budget::daily_budget_outcome(self.user.last_call(), now, daily_reset_offset);
        match outcome {
            DailyBudgetOutcome::NewDay => self.user.reset_daily_counters(),
            DailyBudgetOutcome::SameDay => self.user.bump_times_called_today(),
        }
        self.reset_time_budget();
        // Accounting starts now: elapsed time is measured from the
        // moment the fresh budget was granted.
        self.last_tick_at = now;
        outcome
    }

    /// Resets the per-call time budget to the user's configured
    /// per-call limit.
    ///
    /// Each call starts with a fresh allowance; invoked by
    /// [`Self::begin_daily_budget`] on the `authenticating ->
    /// onboarded` transition (`session.allium:InitialiseDailyBudget`).
    pub(super) fn reset_time_budget(&mut self) {
        self.time_remaining = self.user.time_limit_per_call();
    }

    /// Applies one elapsed minute to the call: accumulates it against
    /// the user's daily total and decrements the per-call budget,
    /// saturating at zero.
    ///
    /// # Returns
    /// `true` when the per-call budget is now exhausted, so the caller
    /// must begin logging off (`session.allium:UpdateTimeUsed` +
    /// `TimeExpired`).
    pub(super) fn consume_minute(&mut self) -> bool {
        let minute = Duration::from_mins(1);
        self.user.add_time_used_today(minute);
        self.time_remaining = self.time_remaining.saturating_sub(minute);
        self.time_remaining.is_zero()
    }

    /// Accrues wall-clock time since [`Self::last_tick_at`] against the
    /// per-call budget (`session.allium:UpdateTimeUsed`): decrements
    /// `time_remaining` (saturating) and adds to the user's daily total,
    /// one whole minute at a time. The tick anchor advances by the
    /// minutes consumed, so a sub-minute remainder carries forward and a
    /// clock that appears to move backwards is a no-op.
    ///
    /// This does **not** transition the session — the expiry-logoff
    /// decision is the caller's (item 27b), which consults the returned
    /// flag together with the `OverrideTimeLimit` right.
    ///
    /// # Parameters
    /// - `now`: the current instant from the application clock.
    ///
    /// # Returns
    /// `true` when the per-call budget is now exhausted.
    pub(super) fn accrue_elapsed(&mut self, now: SystemTime) -> bool {
        let elapsed = now
            .duration_since(self.last_tick_at)
            .unwrap_or(Duration::ZERO);
        // Whole elapsed minutes only; the sub-minute remainder is left
        // on the anchor by advancing it by exactly what we consume, so
        // it carries into the next accrual. A `< 1 min` elapse consumes
        // zero and leaves every field untouched.
        let consumed = Duration::from_secs(elapsed.as_secs() / 60 * 60);
        self.user.add_time_used_today(consumed);
        self.time_remaining = self.time_remaining.saturating_sub(consumed);
        self.last_tick_at += consumed;
        self.time_remaining.is_zero()
    }
}

/// Marker error returned by [`NewUserGate::record_attempt`] when the
/// gate has already passed — the caller should stop prompting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GateAlreadyVerified;

/// Payload of `SessionPhase::NewUserRegistering`: the new-user
/// password gate's per-session progress
/// (`session.allium:VerifyNewUserPassword`, Slice 20a).
///
/// Deliberately close in name to the app-layer `NewUserGateConfig`
/// (`app::session_flow`): the config carries the gate's *policy
/// inputs* (secret, attempt cap), while this value object owns the
/// *per-session state* those inputs gate. The cap stays a per-call
/// parameter of [`Self::record_attempt`] — duplicating config into
/// gate state would create a second source of truth.
#[derive(Debug, Clone)]
pub(super) struct NewUserGate {
    /// Whether the gate has passed (or was never armed).
    verified: bool,
    /// Incorrect password attempts recorded against this session.
    attempts: u32,
}

impl NewUserGate {
    /// `session.allium:InitialiseNewUserGate` (Slice 20a): arms the
    /// gate for a fresh registration sub-flow.
    ///
    /// # Parameters
    /// - `password_required`: mirrors `core/config.new_user_password
    ///   != null`. When `false` no gate runs, so the flag starts
    ///   verified.
    ///
    /// # Returns
    /// A gate with zero attempts, already verified when no password
    /// is required.
    #[must_use]
    pub(super) fn new(password_required: bool) -> Self {
        Self {
            verified: !password_required,
            attempts: 0,
        }
    }

    /// `session.allium:VerifyNewUserPassword` (Slice 20a): records one
    /// gate attempt, owning the verify / bump / threshold rules.
    ///
    /// # Parameters
    /// - `matches`: whether the typed candidate matched
    ///   `core/config.new_user_password` — the comparison is the
    ///   application layer's, keeping presentation and hash-storage
    ///   decisions out of the gate.
    /// - `max_attempts`: `core/config.max_new_user_password_attempts`
    ///   (the `SessionRetriesBounded` invariant's bound).
    ///
    /// # Returns
    /// [`NewUserPasswordOutcome::Verified`] on a match. On a mismatch
    /// the attempt counter climbs (saturating) and the outcome is
    /// [`NewUserPasswordOutcome::TooManyFailures`] once the counter
    /// reaches `max_attempts`, otherwise
    /// [`NewUserPasswordOutcome::Mismatch`].
    ///
    /// # Errors
    /// [`GateAlreadyVerified`] when the gate has already passed.
    pub(super) fn record_attempt(
        &mut self,
        matches: bool,
        max_attempts: u32,
    ) -> Result<NewUserPasswordOutcome, GateAlreadyVerified> {
        if self.verified {
            return Err(GateAlreadyVerified);
        }
        if matches {
            self.verified = true;
            return Ok(NewUserPasswordOutcome::Verified);
        }
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= max_attempts {
            Ok(NewUserPasswordOutcome::TooManyFailures)
        } else {
            Ok(NewUserPasswordOutcome::Mismatch)
        }
    }

    /// Whether the gate has passed — `CompleteNewUserRegistration`'s
    /// `GateNotVerified` precondition and the
    /// `Session::new_user_password_verified` accessor read this.
    #[must_use]
    pub(super) fn verified(&self) -> bool {
        self.verified
    }

    /// Incorrect attempts recorded so far — the
    /// `Session::new_user_password_attempts` accessor reads this.
    #[must_use]
    pub(super) fn attempts(&self) -> u32 {
        self.attempts
    }
}

/// Payload of `SessionPhase::Authenticating`: the identified user and
/// the per-session password bookkeeping that exists only while a
/// password is being verified.
#[derive(Debug, Clone)]
pub(super) struct AuthenticatingAttempt {
    /// The handle exactly as the user typed it at the identify prompt.
    pub(super) typed_name: String,
    /// The user the typed handle resolved to.
    pub(super) user: User,
    /// Bad-password strikes accumulated on this session.
    pub(super) password_retry_count: u32,
}

impl AuthenticatingAttempt {
    /// Records one failed password attempt (the non-matching branch of
    /// `session.allium:VerifyPassword`, Slice 11): bumps the user's
    /// persistent `invalid_attempts` and the per-session
    /// `password_retry_count` together. The two counters feed
    /// `SessionPolicy::password_failure_decision` and must advance in
    /// lockstep — that pairing is the invariant this method owns.
    pub(super) fn record_password_failure(&mut self) {
        self.user.bump_invalid_attempts();
        self.password_retry_count = self.password_retry_count.saturating_add(1);
    }
}

/// What the session still knows about its caller once teardown begins —
/// the payload `LoggingOff` and `Ended` retain.
///
/// Replaces the independently nullable `user`/`authenticated_at` option
/// pairs those phases used to carry, which could represent impossible
/// combinations (an authentication timestamp with no user). Only the
/// three reachable shapes are representable.
#[derive(Debug, Clone)]
pub(super) enum CallSalvage {
    /// Teardown before any user was identified.
    Unidentified,
    /// A user was identified (handle accepted) but the call never
    /// authenticated — e.g. lockout during password verification.
    Identified(User),
    /// Teardown of an authenticated call.
    Authenticated(AuthenticatedCall),
}

impl CallSalvage {
    /// Salvages whatever caller state `phase` carried.
    pub(super) fn from_phase(phase: SessionPhase) -> Self {
        match phase {
            SessionPhase::Connecting
            | SessionPhase::Identifying { .. }
            | SessionPhase::NewUserRegistering { .. } => Self::Unidentified,
            SessionPhase::Authenticating { attempt } => Self::Identified(attempt.user),
            SessionPhase::Onboarded { call } | SessionPhase::Menu { call } => {
                Self::Authenticated(call)
            }
            SessionPhase::LoggingOff { call, .. } | SessionPhase::Ended { call, .. } => call,
        }
    }

    pub(super) fn user(&self) -> Option<&User> {
        match self {
            Self::Unidentified => None,
            Self::Identified(user) => Some(user),
            Self::Authenticated(call) => Some(&call.user),
        }
    }

    pub(super) fn user_mut(&mut self) -> Option<&mut User> {
        match self {
            Self::Unidentified => None,
            Self::Identified(user) => Some(user),
            Self::Authenticated(call) => Some(&mut call.user),
        }
    }

    /// When authentication completed, if this call ever authenticated.
    pub(super) fn authenticated_at(&self) -> Option<SystemTime> {
        match self {
            Self::Authenticated(call) => Some(call.authenticated_at),
            Self::Unidentified | Self::Identified(_) => None,
        }
    }

    /// Remaining per-call time; zero when the call never authenticated.
    pub(super) fn time_remaining(&self) -> Duration {
        match self {
            Self::Authenticated(call) => call.time_remaining,
            Self::Unidentified | Self::Identified(_) => Duration::ZERO,
        }
    }

    /// The call identity, if this call ever authenticated.
    pub(super) fn call_id(&self) -> Option<CallId> {
        match self {
            Self::Authenticated(call) => Some(call.call_id),
            Self::Unidentified | Self::Identified(_) => None,
        }
    }
}
