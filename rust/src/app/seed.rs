//! Default seed data used when the configured user repository is empty
//! (Slice 13a).
//!
//! Phase 1 ships only the in-memory [`UserRepository`][repo] adapter, so
//! a fresh boot has no users to log in as. To make the binary usable
//! out of the box, [`default_sysop`] returns a slot-1 sysop with handle
//! `sysop` and password `sysop`, hashed with the spec's default
//! algorithm. This is explicitly a development seed; later slices that
//! introduce a persistent user store will replace it with proper
//! enrolment.
//!
//! [repo]: crate::domain::user_repository::UserRepository

use std::time::SystemTime;

use crate::domain::password::{PasswordError, PasswordHashKind, PasswordHasher};
use crate::domain::user::{User, UserError};

/// Errors returned by [`default_sysop`].
#[derive(Debug)]
pub enum SeedError {
    /// The hasher couldn't compute a hash for the seed credential.
    Hash(PasswordError),
    /// The freshly hashed credential triple failed [`User::new`]'s
    /// invariants. This should never happen for the spec-default
    /// PBKDF2 hash, but is propagated rather than panicking.
    User(UserError),
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hash(error) => write!(f, "couldn't hash seed credential: {error}"),
            Self::User(error) => write!(f, "couldn't construct seeded user: {error}"),
        }
    }
}

impl std::error::Error for SeedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hash(error) => Some(error),
            Self::User(error) => Some(error),
        }
    }
}

/// Builds the default sysop seed user.
///
/// Returns a slot-`1` [`User`] with handle `sysop`, access level `255`
/// and a password hash for the literal string `sysop` produced by
/// `hasher`. `password_last_updated` is set to [`SystemTime::UNIX_EPOCH`]
/// so the seed sorts before any real account.
///
/// # Errors
/// Returns [`SeedError::Hash`] if the hasher rejects the spec-default
/// [`PasswordHashKind::Pbkdf210000`], or [`SeedError::User`] if the
/// freshly hashed triple fails [`User::new`]'s invariants.
pub fn default_sysop(hasher: &dyn PasswordHasher) -> Result<User, SeedError> {
    let kind = PasswordHashKind::Pbkdf210000;
    let computed = hasher
        .compute_password_hash("sysop", kind)
        .map_err(SeedError::Hash)?;
    User::new(
        1,
        "sysop".to_string(),
        kind,
        computed.hash,
        computed.salt,
        SystemTime::UNIX_EPOCH,
        255,
    )
    .map_err(SeedError::User)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;

    #[test]
    fn default_sysop_uses_slot_1_and_handle_sysop() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(user.is_sysop());
        assert_eq!(user.handle(), "sysop");
    }

    #[test]
    fn default_sysop_authenticates_against_correct_password() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(hasher.verify_password(&user, "sysop").expect("verify"));
    }

    #[test]
    fn default_sysop_rejects_other_passwords() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert!(!hasher.verify_password(&user, "wrong").expect("verify"));
        assert!(!hasher.verify_password(&user, "").expect("verify"));
        assert!(!hasher.verify_password(&user, "Sysop").expect("verify"));
    }

    #[test]
    fn default_sysop_uses_pbkdf2_hash_kind() {
        let hasher = Pbkdf2PasswordHasher::new();
        let user = default_sysop(&hasher).expect("seed");
        assert_eq!(user.password_hash_kind(), PasswordHashKind::Pbkdf210000);
        assert!(user.password_salt().is_some(), "PBKDF2 user has a salt");
    }
}
