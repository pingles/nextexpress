//! [`User`] entity (spec: `core.allium:User`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop actually
//! reads. Lockout, time accounting, ratios and conference state arrive
//! in later slices that introduce the rules reading them.

use std::time::{Duration, SystemTime};

use crate::domain::password::PasswordHashKind;

/// A registered BBS user.
///
/// Construct via [`User::new`], which enforces the
/// `SaltMatchesAlgorithm` invariant from the spec. The lockout state
/// (`invalid_attempts`, `account_locked`) starts cleared and is mutated
/// by the `VerifyPassword` rule.
//
// `dead_code` is allowed at the struct level: every field except those
// exposed via accessors is stored for use by later slices.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct User {
    slot_number: u32,
    handle: String,
    password_hash_kind: PasswordHashKind,
    password_hash: String,
    password_salt: Option<String>,
    password_last_updated: SystemTime,
    access_level: u8,
    invalid_attempts: u32,
    account_locked: bool,
    times_called: u32,
    last_call: Option<SystemTime>,
    time_limit_per_call: Duration,
    time_limit_per_day: Duration,
    time_used_today: Duration,
    times_called_today: u32,
}

impl User {
    /// Constructs a new [`User`].
    ///
    /// # Parameters
    /// - `slot_number`: stable account id; `1` is the sysop.
    /// - `handle`: unique login name.
    /// - `password_hash_kind`, `password_hash`, `password_salt`: the
    ///   opaque credential triple verified by the password adapter.
    /// - `password_last_updated`: when the credential triple was last
    ///   rotated.
    /// - `access_level`: `0..=255` access tier (`0` = locked out).
    ///
    /// # Errors
    /// Returns [`UserError::SaltRequired`] when `password_hash_kind` is
    /// a PBKDF2 variant and `password_salt` is `None`. This enforces
    /// the spec's `SaltMatchesAlgorithm` invariant.
    pub fn new(
        slot_number: u32,
        handle: String,
        password_hash_kind: PasswordHashKind,
        password_hash: String,
        password_salt: Option<String>,
        password_last_updated: SystemTime,
        access_level: u8,
    ) -> Result<Self, UserError> {
        if requires_salt(password_hash_kind) && password_salt.is_none() {
            return Err(UserError::SaltRequired);
        }
        Ok(Self {
            slot_number,
            handle,
            password_hash_kind,
            password_hash,
            password_salt,
            password_last_updated,
            access_level,
            invalid_attempts: 0,
            account_locked: false,
            times_called: 0,
            last_call: None,
            time_limit_per_call: Duration::ZERO,
            time_limit_per_day: Duration::ZERO,
            time_used_today: Duration::ZERO,
            times_called_today: 0,
        })
    }

    /// Returns `true` when this user is the sysop (slot `1`).
    pub fn is_sysop(&self) -> bool {
        self.slot_number == 1
    }

    /// Returns the user's handle (login name).
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Returns the algorithm used to verify the stored password hash.
    pub fn password_hash_kind(&self) -> PasswordHashKind {
        self.password_hash_kind
    }

    /// Returns the opaque stored password hash.
    pub fn password_hash(&self) -> &str {
        &self.password_hash
    }

    /// Returns the salt the stored password hash was bound to, if the
    /// algorithm uses one.
    pub fn password_salt(&self) -> Option<&str> {
        self.password_salt.as_deref()
    }

    /// Returns the number of recent invalid password attempts. Cleared
    /// to zero when the account is locked or a successful login lands.
    pub fn invalid_attempts(&self) -> u32 {
        self.invalid_attempts
    }

    /// Returns whether the account is currently locked out.
    pub fn is_account_locked(&self) -> bool {
        self.account_locked
    }

    /// Increments [`Self::invalid_attempts`] by one. Used by
    /// `session.allium:VerifyPassword` (Slice 11) when a candidate
    /// fails to match.
    pub fn bump_invalid_attempts(&mut self) {
        self.invalid_attempts = self.invalid_attempts.saturating_add(1);
    }

    /// Resets [`Self::invalid_attempts`] to zero.
    pub fn clear_invalid_attempts(&mut self) {
        self.invalid_attempts = 0;
    }

    /// Marks the account as locked and resets `invalid_attempts` to
    /// preserve the spec's `LockoutClearsAttempts` invariant.
    pub fn lock_account(&mut self) {
        self.account_locked = true;
        self.invalid_attempts = 0;
    }

    /// Returns the number of completed logons recorded for this user.
    pub fn times_called(&self) -> u32 {
        self.times_called
    }

    /// Returns the timestamp of the most recently completed logon, if
    /// any.
    pub fn last_call(&self) -> Option<SystemTime> {
        self.last_call
    }

    /// Increments [`Self::times_called`] by one. Used by
    /// `session.allium:EnterMenu` (Slice 12).
    pub fn bump_times_called(&mut self) {
        self.times_called = self.times_called.saturating_add(1);
    }

    /// Updates [`Self::last_call`] to `at`. Used by
    /// `session.allium:FinaliseLogoff` (Slice 13).
    pub fn record_last_call(&mut self, at: SystemTime) {
        self.last_call = Some(at);
    }

    /// Returns the per-call time allowance configured for this user.
    pub fn time_limit_per_call(&self) -> Duration {
        self.time_limit_per_call
    }

    /// Returns the per-day combined time allowance configured for this
    /// user.
    pub fn time_limit_per_day(&self) -> Duration {
        self.time_limit_per_day
    }

    /// Returns how much wall-clock time the user has burned through
    /// today, accumulated across calls in the current accounting day.
    pub fn time_used_today(&self) -> Duration {
        self.time_used_today
    }

    /// Returns the number of completed logons recorded for this user
    /// in the current accounting day.
    pub fn times_called_today(&self) -> u32 {
        self.times_called_today
    }

    /// Sets the per-call and per-day time allowances. Used by the
    /// new-user registration flow and admin tooling.
    ///
    /// # Parameters
    /// - `per_call`: how much time a single visit may consume.
    /// - `per_day`: combined allowance across all visits in one
    ///   accounting day.
    pub fn set_time_limits(&mut self, per_call: Duration, per_day: Duration) {
        self.time_limit_per_call = per_call;
        self.time_limit_per_day = per_day;
    }

    /// Resets the daily counters at the start of a new accounting day.
    ///
    /// Mirrors the new-day branch of `session.allium:InitialiseDailyBudget`
    /// (Slice 14): `times_called_today` and `time_used_today` are
    /// cleared. Daily byte counters and chat-minute accounting land
    /// with the slices that introduce them.
    pub fn reset_daily_counters(&mut self) {
        self.times_called_today = 0;
        self.time_used_today = Duration::ZERO;
    }

    /// Increments [`Self::times_called_today`] by one. Used by the
    /// same-day branch of `session.allium:InitialiseDailyBudget`.
    pub fn bump_times_called_today(&mut self) {
        self.times_called_today = self.times_called_today.saturating_add(1);
    }

    /// Adds `elapsed` to [`Self::time_used_today`]. Used by
    /// `session.allium:UpdateTimeUsed` (Slice 14).
    pub fn add_time_used_today(&mut self, elapsed: Duration) {
        self.time_used_today = self.time_used_today.saturating_add(elapsed);
    }
}

/// Errors returned by [`User::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserError {
    /// The chosen [`PasswordHashKind`] requires a non-null salt
    /// (spec invariant `SaltMatchesAlgorithm`).
    SaltRequired,
}

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SaltRequired => write!(f, "password hash kind requires a salt"),
        }
    }
}

impl std::error::Error for UserError {}

/// Whether the spec's `SaltMatchesAlgorithm` invariant requires a non-null
/// salt for `kind`.
fn requires_salt(kind: PasswordHashKind) -> bool {
    match kind {
        PasswordHashKind::Pbkdf210000 => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(slot: u32, salt: Option<String>) -> Result<User, UserError> {
        User::new(
            slot,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
    }

    #[test]
    fn slot_one_is_sysop() {
        let user = make_user(1, Some("salt".to_string())).expect("valid user");
        assert!(user.is_sysop());
    }

    #[test]
    fn other_slots_are_not_sysop() {
        let user = make_user(2, Some("salt".to_string())).expect("valid user");
        assert!(!user.is_sysop());
    }

    #[test]
    fn pbkdf2_without_salt_is_rejected() {
        let err = make_user(1, None).expect_err("missing salt should error");
        assert_eq!(err, UserError::SaltRequired);
    }

    #[test]
    fn pbkdf2_with_salt_is_accepted() {
        assert!(make_user(1, Some("salt".to_string())).is_ok());
    }

    #[test]
    fn new_user_has_clean_lockout_state() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.invalid_attempts(), 0);
        assert!(!user.is_account_locked());
    }

    #[test]
    fn bump_invalid_attempts_increments() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.bump_invalid_attempts();
        assert_eq!(user.invalid_attempts(), 2);
    }

    #[test]
    fn clear_invalid_attempts_resets() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.clear_invalid_attempts();
        assert_eq!(user.invalid_attempts(), 0);
    }

    #[test]
    fn lock_account_clears_attempts() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.bump_invalid_attempts();
        user.lock_account();
        assert!(user.is_account_locked());
        // LockoutClearsAttempts invariant.
        assert_eq!(user.invalid_attempts(), 0);
    }

    #[test]
    fn new_user_has_zero_time_accounting() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.time_limit_per_call(), Duration::ZERO);
        assert_eq!(user.time_limit_per_day(), Duration::ZERO);
        assert_eq!(user.time_used_today(), Duration::ZERO);
        assert_eq!(user.times_called_today(), 0);
    }

    #[test]
    fn set_time_limits_updates_both_caps() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.set_time_limits(Duration::from_secs(60), Duration::from_secs(3_600));
        assert_eq!(user.time_limit_per_call(), Duration::from_secs(60));
        assert_eq!(user.time_limit_per_day(), Duration::from_secs(3_600));
    }

    #[test]
    fn reset_daily_counters_clears_today_counters() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_times_called_today();
        user.add_time_used_today(Duration::from_secs(120));
        user.reset_daily_counters();
        assert_eq!(user.times_called_today(), 0);
        assert_eq!(user.time_used_today(), Duration::ZERO);
    }

    #[test]
    fn bump_times_called_today_increments() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_times_called_today();
        user.bump_times_called_today();
        assert_eq!(user.times_called_today(), 2);
    }

    #[test]
    fn add_time_used_today_accumulates() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.add_time_used_today(Duration::from_secs(30));
        user.add_time_used_today(Duration::from_secs(45));
        assert_eq!(user.time_used_today(), Duration::from_secs(75));
    }
}
