//! Time / ratio budget rules applied to a [`Session`].
//!
//! Free functions that read and mutate the time-accounting fields
//! the spec attaches to onboarded users (`time_remaining`,
//! `times_called_today`, `time_used_today`). [`tick_minute`] is
//! lifecycle-adjacent because exhausting the time budget transitions
//! the session into `LoggingOff` — it kept here for symmetry with
//! [`initialise_daily_budget`], but readers should be aware of the
//! dual concern.

use std::time::{Duration, SystemTime};

use crate::domain::user::DailyBudgetOutcome;

use super::log_format::floor_to_day;
use super::{
    InitialiseDailyBudgetError, LogoffReason, Session, SessionPhase, TickMinuteError,
    TickMinuteOutcome,
};

/// Decides whether a logon at `now` falls in a new accounting day
/// relative to the user's previous `last_call`.
///
/// This is the single day-boundary decision shared by
/// [`initialise_daily_budget`] (which mutates the in-session user) and
/// the `record_auth_outcome` persistence command (which must carry the
/// same decision to storage) — one function so the two cannot drift.
///
/// # Parameters
/// - `last_call`: the user's previous completed-logon timestamp, if
///   any (`None` counts as a new day).
/// - `now`: the current logon time.
/// - `daily_reset_offset`: how far past midnight UTC the accounting
///   day rolls over (legacy default: six hours).
///
/// # Returns
/// [`DailyBudgetOutcome::NewDay`] when the boundary was crossed,
/// otherwise [`DailyBudgetOutcome::SameDay`].
#[must_use]
pub fn daily_budget_outcome(
    last_call: Option<SystemTime>,
    now: SystemTime,
    daily_reset_offset: Duration,
) -> DailyBudgetOutcome {
    let today = floor_to_day(now, daily_reset_offset);
    let last_call_day = last_call.map(|t| floor_to_day(t, daily_reset_offset));
    if last_call_day.is_none_or(|d| d != today) {
        DailyBudgetOutcome::NewDay
    } else {
        DailyBudgetOutcome::SameDay
    }
}

/// `session.allium:InitialiseDailyBudget` rule (Slice 14).
///
/// Fires once the session has reached [`super::SessionState::Onboarded`].
/// If `now` falls in a different accounting day from the user's
/// previous `last_call`, the daily counters reset; otherwise
/// `times_called_today` increments. `time_remaining` is then set to
/// `user.time_limit_per_call`.
///
/// The accounting day boundary is `daily_reset_offset` past midnight
/// UTC (the legacy `AmiExpress` default is six hours, so the day
/// rolls over at 06:00 UTC).
///
/// # Errors
/// Returns [`InitialiseDailyBudgetError::WrongState`] when the
/// session is not in [`super::SessionState::Onboarded`].
pub fn initialise_daily_budget(
    session: &mut Session,
    now: SystemTime,
    daily_reset_offset: Duration,
) -> Result<(), InitialiseDailyBudgetError> {
    let SessionPhase::Onboarded { call } = &mut session.phase else {
        return Err(InitialiseDailyBudgetError::WrongState(session.state()));
    };

    match daily_budget_outcome(call.user.last_call(), now, daily_reset_offset) {
        DailyBudgetOutcome::NewDay => call.user.reset_daily_counters(),
        DailyBudgetOutcome::SameDay => call.user.bump_times_called_today(),
    }
    call.time_remaining = call.user.time_limit_per_call();
    Ok(())
}

/// `session.allium:UpdateTimeUsed` + `TimeExpired` rules (Slice 14).
///
/// Decrements `time_remaining` by one minute (saturating at zero)
/// and accumulates the same minute against `user.time_used_today`.
/// If `time_remaining` reaches zero the session transitions to
/// [`super::SessionState::LoggingOff`] with
/// [`LogoffReason::OutOfTime`] — this method is lifecycle-adjacent.
///
/// # Errors
/// Returns [`TickMinuteError::WrongState`] when the session is not
/// in [`super::SessionState::Onboarded`] or [`super::SessionState::Menu`].
pub fn tick_minute(session: &mut Session) -> Result<TickMinuteOutcome, TickMinuteError> {
    let expired = match &mut session.phase {
        SessionPhase::Onboarded { call } | SessionPhase::Menu { call } => {
            call.user.add_time_used_today(Duration::from_mins(1));
            call.time_remaining = call.time_remaining.saturating_sub(Duration::from_mins(1));
            call.time_remaining.is_zero()
        }
        _ => return Err(TickMinuteError::WrongState(session.state())),
    };
    if expired {
        session.move_to_logging_off(Some(LogoffReason::OutOfTime));
        Ok(TickMinuteOutcome::TimeExpired)
    } else {
        Ok(TickMinuteOutcome::Continued)
    }
}
