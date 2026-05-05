//! [`User`] entity (spec: `core.allium:User`).
//!
//! Phase 1 holds only the fields the sign-in / log-off loop actually
//! reads. Lockout, time accounting, ratios and conference state arrive
//! in later slices that introduce the rules reading them.

use std::time::SystemTime;

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
}
