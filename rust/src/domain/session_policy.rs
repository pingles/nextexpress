//! Session-domain policy values and the password-failure decision.
//!
//! Lifted out of `session.rs` so the module that owns the Session
//! state-machine doesn't also own its tunable knobs. The struct is a
//! pure value type; [`SessionPolicy::password_failure_decision`] takes
//! the two failure counters the session recorded and decides whether
//! to continue, end the session, or lock the account.

use std::time::Duration;

/// Default consecutive bad-password attempts before a session ends or
/// account lockout applies.
const DEFAULT_MAX_PASSWORD_FAILURES: u32 = 3;

/// Default offset past midnight UTC used by [`SessionPolicy::new`].
/// Mirrors the legacy `AmiExpress` constant `21600` seconds (six hours)
/// at `amiexpress/express.e:529`.
const DEFAULT_DAILY_RESET_OFFSET: Duration = Duration::from_hours(6);

/// Default per-input idle timeout (`core.allium:config.input_timeout`,
/// Slice 17). Five minutes.
const DEFAULT_INPUT_TIMEOUT: Duration = Duration::from_mins(5);

/// Default maximum handle attempts during the new-user registration
/// sub-flow before the session bails. Mirrors the legacy `AmiExpress`
/// `doNewUser` retry budget at `amiexpress/express.e:30150`.
const DEFAULT_MAX_REGISTRATION_HANDLE_ATTEMPTS: u32 = 5;

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
    input_timeout: Duration,
    treat_timeout_as_logoff: bool,
    max_registration_handle_attempts: u32,
}

impl SessionPolicy {
    /// Constructs a session policy with spec defaults: six-hour daily
    /// reset, expiry disabled, no password-strength enforcement,
    /// five-minute idle timeout, and the timeout reported as
    /// `carrier_loss`.
    ///
    /// # Parameters
    /// - `max_password_failures`: the number of consecutive bad
    ///   password attempts that ends the session or locks the account.
    ///
    /// # Returns
    /// A [`SessionPolicy`] carrying the supplied password-failure
    /// limit and the listed defaults.
    #[must_use]
    pub fn new(max_password_failures: u32) -> Self {
        Self {
            max_password_failures,
            daily_reset_offset: DEFAULT_DAILY_RESET_OFFSET,
            password_expiry_days: 0,
            min_password_length: 0,
            min_password_categories: 0,
            input_timeout: DEFAULT_INPUT_TIMEOUT,
            treat_timeout_as_logoff: false,
            max_registration_handle_attempts: DEFAULT_MAX_REGISTRATION_HANDLE_ATTEMPTS,
        }
    }

    /// Returns a copy of `self` with [`Self::daily_reset_offset`]
    /// replaced by `offset`.
    #[must_use]
    pub fn with_daily_reset_offset(mut self, offset: Duration) -> Self {
        self.daily_reset_offset = offset;
        self
    }

    /// Returns the configured daily reset offset (Slice 14).
    #[must_use]
    pub fn daily_reset_offset(&self) -> Duration {
        self.daily_reset_offset
    }

    /// Returns a copy of `self` with [`Self::password_expiry_days`]
    /// replaced by `days`. `0` disables expiry.
    #[must_use]
    pub fn with_password_expiry_days(mut self, days: u32) -> Self {
        self.password_expiry_days = days;
        self
    }

    /// Returns the configured password expiry, in days (Slice 15).
    /// `0` disables expiry.
    #[must_use]
    pub fn password_expiry_days(&self) -> u32 {
        self.password_expiry_days
    }

    /// Returns a copy of `self` with [`Self::min_password_length`]
    /// replaced by `length`. `0` disables the length check.
    #[must_use]
    pub fn with_min_password_length(mut self, length: u32) -> Self {
        self.min_password_length = length;
        self
    }

    /// Returns the configured minimum password length (Slice 15).
    /// `0` disables the length check.
    #[must_use]
    pub fn min_password_length(&self) -> u32 {
        self.min_password_length
    }

    /// Returns a copy of `self` with [`Self::min_password_categories`]
    /// replaced by `categories`. `0` disables the category check;
    /// values above `4` are treated as `4`.
    #[must_use]
    pub fn with_min_password_categories(mut self, categories: u32) -> Self {
        self.min_password_categories = categories;
        self
    }

    /// Returns the configured minimum password categories (Slice 15).
    /// `0` disables the category check.
    #[must_use]
    pub fn min_password_categories(&self) -> u32 {
        self.min_password_categories
    }

    /// Returns a copy of `self` with [`Self::input_timeout`] replaced
    /// by `timeout`. Slice 17.
    #[must_use]
    pub fn with_input_timeout(mut self, timeout: Duration) -> Self {
        self.input_timeout = timeout;
        self
    }

    /// Returns the configured per-input idle timeout (Slice 17).
    /// Mirrors `core/config.input_timeout`; the default is five
    /// minutes.
    #[must_use]
    pub fn input_timeout(&self) -> Duration {
        self.input_timeout
    }

    /// Returns a copy of `self` with
    /// [`Self::treat_timeout_as_logoff`] replaced by `value`.
    #[must_use]
    pub fn with_treat_timeout_as_logoff(mut self, value: bool) -> Self {
        self.treat_timeout_as_logoff = value;
        self
    }

    /// Returns whether an idle timeout is reported as
    /// [`crate::domain::session::LogoffReason::InputTimeout`] (`true`)
    /// or [`crate::domain::session::LogoffReason::CarrierLoss`]
    /// (`false`). Mirrors `core/config.treat_timeout_as_logoff`.
    /// Slice 17.
    #[must_use]
    pub fn treat_timeout_as_logoff(&self) -> bool {
        self.treat_timeout_as_logoff
    }

    /// Returns a copy of `self` with
    /// [`Self::max_registration_handle_attempts`] replaced by
    /// `attempts`.
    #[must_use]
    pub fn with_max_registration_handle_attempts(mut self, attempts: u32) -> Self {
        self.max_registration_handle_attempts = attempts;
        self
    }

    /// Maximum number of consecutive unavailable / invalid handles a
    /// user may type during the new-user registration sub-flow before
    /// the session bails. Mirrors the legacy `AmiExpress` `doNewUser`
    /// retry budget at `amiexpress/express.e:30150`.
    #[must_use]
    pub fn max_registration_handle_attempts(&self) -> u32 {
        self.max_registration_handle_attempts
    }

    /// Decides what should happen after a password failure has been
    /// recorded.
    ///
    /// The account-level lockout decision wins over the session-level
    /// end decision when both counters have reached the configured
    /// limit.
    ///
    /// # Parameters
    /// - `user_invalid_attempts`: the bound user's persistent
    ///   failed-attempt counter, after the failure was recorded.
    /// - `password_retry_count`: the session's bad-password strike
    ///   counter, after the failure was recorded.
    ///
    /// # Returns
    /// A [`PasswordFailureDecision`] describing whether the session
    /// may continue, should end, or should lock the bound account.
    #[must_use]
    pub fn password_failure_decision(
        &self,
        user_invalid_attempts: u32,
        password_retry_count: u32,
    ) -> PasswordFailureDecision {
        if user_invalid_attempts >= self.max_password_failures {
            PasswordFailureDecision::LockAccount
        } else if password_retry_count >= self.max_password_failures {
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
