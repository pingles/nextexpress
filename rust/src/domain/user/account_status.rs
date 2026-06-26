//! [`AccountStatus`] value object — access tier, lockout counters, and
//! validation status for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module.

/// Access tier, lockout counters, and validation status for a
/// [`crate::domain::user::User`].
#[derive(Debug, Clone)]
pub(super) struct AccountStatus {
    /// `0..=255` access tier (`0` = locked out).
    access_level: u8,
    /// Recent invalid password attempts.
    invalid_attempts: u32,
    /// Independent account-lock flag set by lockout rules/admin tools.
    account_locked: bool,
    /// Whether the account is awaiting sysop validation.
    new_user: bool,
    /// `core.allium:User.censored` — when true the user's posts are
    /// silently downgraded to `private_to_sysop` (`messaging.allium`
    /// visibility selector, Slice 47). Defaults to false; sysop
    /// admin tools that flip the flag are out of scope for Slice 47.
    censored: bool,
}

impl AccountStatus {
    /// Constructs account status for an existing user loaded from
    /// configuration or storage.
    pub(super) fn existing(access_level: u8) -> Self {
        Self {
            access_level,
            invalid_attempts: 0,
            account_locked: false,
            new_user: false,
            censored: false,
        }
    }

    /// Constructs the spec defaults for a freshly registered user.
    pub(super) fn awaiting_validation() -> Self {
        Self {
            access_level: 2,
            invalid_attempts: 0,
            account_locked: false,
            new_user: true,
            censored: false,
        }
    }

    /// Reconstructs the account status from a persisted snapshot.
    ///
    /// Used by [`crate::domain::user::User::from_persisted`] to thread
    /// every persisted bit — including the access level for accounts
    /// that the sysop has promoted past `awaiting_validation`'s default
    /// of `2` — verbatim through the round-trip.
    pub(super) fn from_persisted(
        access_level: u8,
        invalid_attempts: u32,
        account_locked: bool,
        is_new_user: bool,
        censored: bool,
    ) -> Self {
        Self {
            access_level,
            invalid_attempts,
            account_locked,
            new_user: is_new_user,
            censored,
        }
    }

    pub(super) fn is_censored(&self) -> bool {
        self.censored
    }

    pub(super) fn set_censored(&mut self, value: bool) {
        self.censored = value;
    }

    pub(super) fn invalid_attempts(&self) -> u32 {
        self.invalid_attempts
    }

    pub(super) fn is_account_locked(&self) -> bool {
        self.account_locked
    }

    pub(super) fn access_level(&self) -> u8 {
        self.access_level
    }

    pub(super) fn is_locked_out(&self) -> bool {
        self.access_level <= 1 || self.is_account_locked()
    }

    pub(super) fn bump_invalid_attempts(&mut self) {
        self.invalid_attempts = self.invalid_attempts.saturating_add(1);
    }

    pub(super) fn clear_invalid_attempts(&mut self) {
        self.invalid_attempts = 0;
    }

    pub(super) fn lock_account(&mut self) {
        self.account_locked = true;
        self.invalid_attempts = 0;
    }

    pub(super) fn is_new_user(&self) -> bool {
        self.new_user
    }
}
