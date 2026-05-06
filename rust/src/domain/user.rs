//! [`User`] entity (spec: `core.allium:User`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop actually
//! reads. Lockout, time accounting, ratios and conference state arrive
//! in later slices that introduce the rules reading them.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use crate::domain::password::PasswordHashKind;

/// Ratio enforcement mode for a user (spec: `core.allium:RatioMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatioMode {
    /// Ratio enforcement is off.
    Disabled,
    /// Enforce uploads:downloads file count.
    ByFiles,
    /// Enforce uploads:downloads byte count.
    ByBytes,
}

/// Bit-flag preferences persisted on a user record
/// (spec: `core.allium:UserFlag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UserFlag {
    /// Show the "new user" greeting once.
    ShowNewUserMessage,
    /// Auto-join the first conference on logon.
    AutoJoinFirstConf,
    /// Show one-time messages.
    ShowOneTimeMessages,
    /// Clear the screen after each message.
    ScreenClearAfterMessage,
    /// User has paid; affects screens, not access.
    IsDonor,
    /// Use the full-screen editor.
    EditorFullScreen,
    /// Show editor prompts.
    EditorPrompts,
    /// Check uploads asynchronously in the background.
    BackgroundFileCheck,
}

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
    force_password_reset: bool,
    is_new_user: bool,
    location: Option<String>,
    phone_number: Option<String>,
    email: Option<String>,
    line_length: u32,
    ansi_colour: bool,
    account_created: SystemTime,
    flags: BTreeSet<UserFlag>,
    ratio_mode: RatioMode,
    ratio_value: u32,
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
            force_password_reset: false,
            is_new_user: false,
            location: None,
            phone_number: None,
            email: None,
            line_length: 0,
            ansi_colour: false,
            account_created: password_last_updated,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::Disabled,
            ratio_value: 0,
        })
    }

    /// Builds a freshly-registered new user from a completed
    /// registration profile.
    ///
    /// Mirrors the `User.created(...)` consequent of
    /// `session.allium:CompleteNewUserRegistration` (Slice 20). All
    /// non-profile fields are set to the spec's exact defaults: access
    /// level `2`, `is_new_user = true`, `force_password_reset = false`,
    /// thirty-minute per-call / one-hour per-day allowances, zeroed
    /// counters, ZMODEM as the preferred protocol (held implicitly
    /// until Slice 53 introduces the field), and `account_created` /
    /// `last_call` / `password_last_updated` all set to `now`.
    ///
    /// # Errors
    /// Returns [`UserError::SaltRequired`] when
    /// `profile.password_hash_kind` is a PBKDF2 variant and
    /// `profile.password_salt` is `None`. This enforces the spec's
    /// `SaltMatchesAlgorithm` invariant.
    pub fn register_new(profile: NewUserRegistration) -> Result<Self, UserError> {
        let NewUserRegistration {
            slot_number,
            handle,
            location,
            phone_number,
            email,
            password_hash,
            password_salt,
            password_hash_kind,
            line_length,
            ansi_colour,
            flags,
            ratio_mode,
            ratio_value,
            now,
        } = profile;
        if requires_salt(password_hash_kind) && password_salt.is_none() {
            return Err(UserError::SaltRequired);
        }
        Ok(Self {
            slot_number,
            handle,
            password_hash_kind,
            password_hash,
            password_salt,
            password_last_updated: now,
            access_level: 2,
            invalid_attempts: 0,
            account_locked: false,
            times_called: 0,
            last_call: Some(now),
            time_limit_per_call: Duration::from_secs(30 * 60),
            time_limit_per_day: Duration::from_secs(60 * 60),
            time_used_today: Duration::ZERO,
            times_called_today: 0,
            force_password_reset: false,
            is_new_user: true,
            location,
            phone_number,
            email,
            line_length,
            ansi_colour,
            account_created: now,
            flags,
            ratio_mode,
            ratio_value,
        })
    }

    /// Returns `true` when this user is the sysop (slot `1`).
    pub fn is_sysop(&self) -> bool {
        self.slot_number == 1
    }

    /// Returns this user's stable slot number
    /// (`core.allium:User.slot_number`).
    pub fn slot_number(&self) -> u32 {
        self.slot_number
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

    /// Returns the user's access tier (`0..=255`).
    pub fn access_level(&self) -> u8 {
        self.access_level
    }

    /// Spec-derived predicate (`core.allium:User.is_locked_out`,
    /// Slice 16): `access_level <= 1 or account_locked`.
    ///
    /// `access_level == 0` is the explicit lockout tier; `1` is
    /// reserved as "below the minimum non-locked tier" per the spec
    /// (new users start at `2`). Either lower bound, or an
    /// independently set `account_locked` flag, qualifies.
    pub fn is_locked_out(&self) -> bool {
        self.access_level <= 1 || self.account_locked
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

    /// Returns the timestamp the user's password hash was last
    /// rotated. Used by `session.allium:ForcePasswordReset` to detect
    /// expiry against `core/config.password_expiry_days` (Slice 15).
    pub fn password_last_updated(&self) -> SystemTime {
        self.password_last_updated
    }

    /// Returns whether the next logon must force the user through the
    /// password-change sub-flow (`session.allium:Session.user.force_password_reset`,
    /// Slice 15). Set by `ForcePasswordReset`, cleared by
    /// `CompletePasswordReset`.
    pub fn force_password_reset(&self) -> bool {
        self.force_password_reset
    }

    /// Sets [`Self::force_password_reset`]. Used by
    /// `session.allium:ForcePasswordReset` (Slice 15) and by sysop
    /// admin tooling.
    pub fn set_force_password_reset(&mut self, value: bool) {
        self.force_password_reset = value;
    }

    /// Returns whether this account is awaiting sysop validation
    /// (`core.allium:User.is_new_user`). Set by
    /// `session.allium:CompleteNewUserRegistration` (Slice 20);
    /// cleared by the sysop validate-user workflow that lands in
    /// Phase 6.
    pub fn is_new_user(&self) -> bool {
        self.is_new_user
    }

    /// Returns the user's free-text "City, State" location, if any.
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    /// Returns the user's phone number on file, if any.
    pub fn phone_number(&self) -> Option<&str> {
        self.phone_number.as_deref()
    }

    /// Returns the user's email address on file, if any.
    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    /// Returns the user's preferred terminal width (`0` = auto).
    pub fn line_length(&self) -> u32 {
        self.line_length
    }

    /// Returns whether the user wants ANSI colour output.
    pub fn ansi_colour(&self) -> bool {
        self.ansi_colour
    }

    /// Returns the timestamp the account was first created.
    pub fn account_created(&self) -> SystemTime {
        self.account_created
    }

    /// Returns the user's preference flags
    /// (`core.allium:User.flags`).
    pub fn flags(&self) -> &BTreeSet<UserFlag> {
        &self.flags
    }

    /// Returns the ratio enforcement mode in effect for this user.
    pub fn ratio_mode(&self) -> RatioMode {
        self.ratio_mode
    }

    /// Returns the configured ratio threshold (e.g. `3` = three
    /// downloads per upload). `0` with a non-disabled mode means
    /// infinite.
    pub fn ratio_value(&self) -> u32 {
        self.ratio_value
    }

    /// Atomically replaces the user's stored credentials and clears
    /// [`Self::force_password_reset`].
    ///
    /// Mirrors the `ensures` block of
    /// `session.allium:CompletePasswordReset` (Slice 15): updates
    /// `password_hash`, `password_salt`, `password_hash_kind`,
    /// `password_last_updated`, and resets `force_password_reset`.
    ///
    /// # Parameters
    /// - `hash`: opaque output of [`PasswordHasher::compute_password_hash`].
    /// - `salt`: salt the hash was bound to (`None` for hash kinds
    ///   that don't take a salt).
    /// - `kind`: algorithm used for `hash`.
    /// - `at`: timestamp the change happened.
    ///
    /// [`PasswordHasher::compute_password_hash`]: crate::domain::password::PasswordHasher::compute_password_hash
    pub fn record_password_change(
        &mut self,
        hash: String,
        salt: Option<String>,
        kind: PasswordHashKind,
        at: SystemTime,
    ) {
        self.password_hash = hash;
        self.password_salt = salt;
        self.password_hash_kind = kind;
        self.password_last_updated = at;
        self.force_password_reset = false;
    }
}

/// Bundle of fields collected during the new-user registration
/// sub-flow, plus the freshly computed password hash, that
/// [`User::register_new`] consumes.
///
/// Mirrors the `profile` argument of
/// `session.allium:CompleteNewUserRegistration`. The slot number and
/// ratio defaults come from outside the profile (the user
/// repository's [`crate::domain::user_repository::UserRepository::next_free_slot`]
/// and `core/config.default_ratio_*`); they are bundled here so
/// `register_new` has every piece of data the spec rule names without
/// reaching into other ports.
#[derive(Debug, Clone)]
pub struct NewUserRegistration {
    /// Slot allocated by the user repository.
    pub slot_number: u32,
    /// Handle the user typed at the registration prompt.
    pub handle: String,
    /// Free-text "City, State" location.
    pub location: Option<String>,
    /// Phone number.
    pub phone_number: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Pre-computed password hash bytes.
    pub password_hash: String,
    /// Salt the hash was bound to (`None` for hash kinds that don't
    /// take one).
    pub password_salt: Option<String>,
    /// Algorithm used for `password_hash`.
    pub password_hash_kind: PasswordHashKind,
    /// Preferred terminal width (`0` = auto).
    pub line_length: u32,
    /// Whether the user wants ANSI colour output.
    pub ansi_colour: bool,
    /// Initial preference flags.
    pub flags: BTreeSet<UserFlag>,
    /// Ratio enforcement mode (`core/config.default_ratio_mode`).
    pub ratio_mode: RatioMode,
    /// Ratio threshold (`core/config.default_ratio_value`).
    pub ratio_value: u32,
    /// Timestamp recorded as `account_created`, `last_call`, and
    /// `password_last_updated`.
    pub now: SystemTime,
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

    #[test]
    fn new_user_does_not_force_password_reset() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.force_password_reset());
    }

    #[test]
    fn set_force_password_reset_round_trips() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.set_force_password_reset(true);
        assert!(user.force_password_reset());
        user.set_force_password_reset(false);
        assert!(!user.force_password_reset());
    }

    #[test]
    fn access_level_returned_via_accessor() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.access_level(), 100);
    }

    #[test]
    fn is_locked_out_when_access_level_at_or_below_one() {
        let user_zero = User::new(
            2,
            "lo0".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            0,
        )
        .unwrap();
        let user_one = User::new(
            3,
            "lo1".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            1,
        )
        .unwrap();
        let user_two = User::new(
            4,
            "ok".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            2,
        )
        .unwrap();
        assert!(user_zero.is_locked_out());
        assert!(user_one.is_locked_out());
        assert!(!user_two.is_locked_out());
    }

    #[test]
    fn is_locked_out_when_account_locked_regardless_of_access_level() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        // Default access level is 100 — well clear of the threshold.
        assert!(!user.is_locked_out());
        user.lock_account();
        assert!(user.is_locked_out());
    }

    fn registration() -> NewUserRegistration {
        NewUserRegistration {
            slot_number: 7,
            handle: "newbie".to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555-0123".to_string()),
            email: Some("newbie@example.com".to_string()),
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 80,
            ansi_colour: true,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::ByFiles,
            ratio_value: 3,
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        }
    }

    #[test]
    fn register_new_applies_spec_defaults_for_a_fresh_account() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let user = User::register_new(registration()).expect("valid");
        // Identity carried from profile.
        assert_eq!(user.slot_number, 7);
        assert_eq!(user.handle(), "newbie");
        assert_eq!(user.location(), Some("Townsville"));
        assert_eq!(user.phone_number(), Some("555-0123"));
        assert_eq!(user.email(), Some("newbie@example.com"));
        assert_eq!(user.line_length(), 80);
        assert!(user.ansi_colour());
        // Spec defaults.
        assert_eq!(user.access_level(), 2);
        assert!(user.is_new_user());
        assert!(!user.is_account_locked());
        assert!(!user.force_password_reset());
        assert_eq!(user.invalid_attempts(), 0);
        assert_eq!(user.times_called(), 0);
        assert_eq!(user.times_called_today(), 0);
        assert_eq!(user.time_used_today(), Duration::ZERO);
        assert_eq!(user.time_limit_per_call(), Duration::from_secs(30 * 60));
        assert_eq!(user.time_limit_per_day(), Duration::from_secs(60 * 60));
        assert_eq!(user.last_call(), Some(now));
        assert_eq!(user.account_created(), now);
        assert_eq!(user.password_last_updated(), now);
        assert_eq!(user.ratio_mode(), RatioMode::ByFiles);
        assert_eq!(user.ratio_value(), 3);
        assert!(user.flags().is_empty());
    }

    #[test]
    fn register_new_pbkdf2_without_salt_is_rejected() {
        let mut profile = registration();
        profile.password_salt = None;
        let err = User::register_new(profile).expect_err("missing salt should error");
        assert_eq!(err, UserError::SaltRequired);
    }

    #[test]
    fn register_new_user_is_below_lockout_threshold_via_access_level_one() {
        // The spec sets access_level = 2 for new users; downgrading
        // exposes the `is_locked_out` predicate boundary.
        let user = User::register_new(registration()).expect("valid");
        assert!(!user.is_locked_out(), "level 2 should be allowed through");
    }

    #[test]
    fn user_new_defaults_extended_fields_for_existing_accounts() {
        // Pre-Slice-20 callers (tests, seed sysop) treat the new
        // fields as off-by-default: not a new user, no contact info,
        // no flags, ratio disabled.
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.is_new_user());
        assert!(user.location().is_none());
        assert!(user.phone_number().is_none());
        assert!(user.email().is_none());
        assert_eq!(user.line_length(), 0);
        assert!(!user.ansi_colour());
        assert!(user.flags().is_empty());
        assert_eq!(user.ratio_mode(), RatioMode::Disabled);
        assert_eq!(user.ratio_value(), 0);
        // account_created mirrors password_last_updated for legacy
        // construction; the registration constructor sets `now`.
        assert_eq!(user.account_created(), user.password_last_updated());
    }

    #[test]
    fn record_password_change_updates_credentials_and_clears_flag() {
        let mut user = make_user(2, Some("old_salt".to_string())).unwrap();
        user.set_force_password_reset(true);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        user.record_password_change(
            "new_hash".to_string(),
            Some("new_salt".to_string()),
            PasswordHashKind::Pbkdf210000,
            later,
        );
        assert_eq!(user.password_hash(), "new_hash");
        assert_eq!(user.password_salt(), Some("new_salt"));
        assert_eq!(user.password_hash_kind(), PasswordHashKind::Pbkdf210000);
        assert_eq!(user.password_last_updated(), later);
        assert!(!user.force_password_reset());
    }
}
