//! Password hashing types and the [`PasswordHasher`] port.
//!
//! Spec references:
//! - `core.allium:PasswordHashKind`
//! - `core.allium` black-box helpers `verify_password` and
//!   `compute_password_hash`.

use crate::domain::user::User;

/// Algorithm used to verify a user's password.
///
/// The Phase 1 schema lists only the spec default for new accounts (see
/// `core.allium:config.password_hash_kind`). Legacy and lower-round
/// PBKDF2 variants land in Slice 64 once a stored user record forces
/// the BBS to read them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasswordHashKind {
    /// PBKDF2 with 10,000 rounds. The default for new accounts.
    Pbkdf210000,
}

/// A freshly computed password hash plus the salt it was bound to.
///
/// The salt is `Some` for every PBKDF2 variant; legacy hashes (Slice 64)
/// will return `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedHash {
    /// Opaque hash string in the adapter's chosen encoding.
    pub hash: String,
    /// Salt string used to produce `hash`, if the algorithm needs one.
    pub salt: Option<String>,
}

/// Errors returned by the [`PasswordHasher`] port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordError {
    /// The hasher does not implement the requested
    /// [`PasswordHashKind`]. Phase 1 only ships
    /// [`PasswordHashKind::Pbkdf210000`]; Slice 64 fills in the rest.
    UnsupportedHashKind(PasswordHashKind),
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedHashKind(kind) => {
                write!(f, "unsupported password hash kind: {kind:?}")
            }
        }
    }
}

impl std::error::Error for PasswordError {}

/// Port over the password-hash adapter.
///
/// Implementations live in [`crate::adapters`]. The methods correspond
/// directly to the `verify_password` and `compute_password_hash`
/// black-box helpers in `core.allium`.
pub trait PasswordHasher {
    /// Verifies a `candidate` password against the credentials stored
    /// on `user`.
    ///
    /// # Errors
    /// Returns [`PasswordError::UnsupportedHashKind`] if the user's
    /// stored kind is not implemented by this hasher.
    fn verify_password(&self, user: &User, candidate: &str) -> Result<bool, PasswordError>;

    /// Computes a fresh hash for `candidate` using `kind`, returning
    /// the hash and (where applicable) the salt that was generated.
    ///
    /// # Errors
    /// Returns [`PasswordError::UnsupportedHashKind`] if `kind` is not
    /// implemented by this hasher.
    fn compute_password_hash(
        &self,
        candidate: &str,
        kind: PasswordHashKind,
    ) -> Result<ComputedHash, PasswordError>;
}
