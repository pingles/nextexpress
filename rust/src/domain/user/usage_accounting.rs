//! [`UsageAccounting`] value object — logon counters and time-budget
//! bookkeeping for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module. The [`User`] aggregate
//! delegates to these methods through its public accessors.

use std::time::{Duration, SystemTime};

/// Logon-counter and time-budget bookkeeping for a
/// [`crate::domain::user::User`].
///
/// Bundles the six fields the time-budget rules
/// (`session.allium:EnterMenu`, `InitialiseDailyBudget`,
/// `UpdateTimeUsed`, `FinaliseLogoff`) read or mutate together.
/// Owned privately by [`crate::domain::user::User`]; the user's public
/// surface delegates to these accessors so callers don't change.
#[derive(Debug, Clone)]
pub(super) struct UsageAccounting {
    /// Number of completed logons recorded for this user.
    times_called: u32,
    /// Timestamp of the most recently completed logon, if any.
    last_call: Option<SystemTime>,
    /// Per-call wall-clock allowance.
    time_limit_per_call: Duration,
    /// Combined per-day allowance across all visits in one accounting day.
    time_limit_per_day: Duration,
    /// Wall-clock time burned through today.
    time_used_today: Duration,
    /// Number of completed logons recorded for this user in the current
    /// accounting day.
    times_called_today: u32,
}

impl UsageAccounting {
    /// Constructs a freshly-zeroed accounting record.
    pub(super) fn new() -> Self {
        Self {
            times_called: 0,
            last_call: None,
            time_limit_per_call: Duration::ZERO,
            time_limit_per_day: Duration::ZERO,
            time_used_today: Duration::ZERO,
            times_called_today: 0,
        }
    }

    /// Rebuilds an accounting record from a persisted snapshot. Used
    /// by [`crate::domain::user::User::from_persisted`] to thread every
    /// counter and allowance verbatim from durable storage back into the
    /// aggregate.
    pub(super) fn from_persisted(
        times_called: u32,
        times_called_today: u32,
        last_call: Option<SystemTime>,
        time_limit_per_call: Duration,
        time_limit_per_day: Duration,
        time_used_today: Duration,
    ) -> Self {
        Self {
            times_called,
            last_call,
            time_limit_per_call,
            time_limit_per_day,
            time_used_today,
            times_called_today,
        }
    }

    /// Constructs an accounting record with the spec's
    /// `CompleteNewUserRegistration` defaults: 30-minute per-call /
    /// 1-hour per-day allowances, `last_call = now`, counters zeroed.
    pub(super) fn for_fresh_registration(now: SystemTime) -> Self {
        Self {
            times_called: 0,
            last_call: Some(now),
            time_limit_per_call: Duration::from_mins(30),
            time_limit_per_day: Duration::from_hours(1),
            time_used_today: Duration::ZERO,
            times_called_today: 0,
        }
    }

    pub(super) fn times_called(&self) -> u32 {
        self.times_called
    }
    pub(super) fn last_call(&self) -> Option<SystemTime> {
        self.last_call
    }
    pub(super) fn time_limit_per_call(&self) -> Duration {
        self.time_limit_per_call
    }
    pub(super) fn time_limit_per_day(&self) -> Duration {
        self.time_limit_per_day
    }
    pub(super) fn time_used_today(&self) -> Duration {
        self.time_used_today
    }
    pub(super) fn times_called_today(&self) -> u32 {
        self.times_called_today
    }

    pub(super) fn bump_times_called(&mut self) {
        self.times_called = self.times_called.saturating_add(1);
    }

    pub(super) fn record_last_call(&mut self, at: SystemTime) {
        self.last_call = Some(at);
    }

    pub(super) fn set_time_limits(&mut self, per_call: Duration, per_day: Duration) {
        self.time_limit_per_call = per_call;
        self.time_limit_per_day = per_day;
    }

    /// Resets the daily counters at the start of a new accounting day.
    pub(super) fn reset_daily_counters(&mut self) {
        self.times_called_today = 0;
        self.time_used_today = Duration::ZERO;
    }

    pub(super) fn bump_times_called_today(&mut self) {
        self.times_called_today = self.times_called_today.saturating_add(1);
    }

    pub(super) fn add_time_used_today(&mut self, elapsed: Duration) {
        self.time_used_today = self.time_used_today.saturating_add(elapsed);
    }
}
