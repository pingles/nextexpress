//! PBKDF2-HMAC-SHA256 [`PasswordHasher`] adapter.
//!
//! Phase 1 only implements [`PasswordHashKind::Pbkdf210000`], the spec
//! default for new accounts. Other kinds (legacy, lower-round PBKDF2)
//! return [`PasswordError::UnsupportedHashKind`] until Slice 64 fills
//! them in.

use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

use crate::domain::password::{ComputedHash, PasswordError, PasswordHashKind, PasswordHasher};
use crate::domain::user::User;

/// Length in bytes of the random salt generated for new hashes.
const SALT_BYTES: usize = 16;

/// Length in bytes of the derived key (SHA-256 output).
const HASH_BYTES: usize = 32;

/// Number of PBKDF2 iterations for [`PasswordHashKind::Pbkdf210000`].
const PBKDF2_ROUNDS: u32 = 10_000;

/// PBKDF2-HMAC-SHA256 hasher with a hex-encoded hash and salt.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pbkdf2PasswordHasher;

impl Pbkdf2PasswordHasher {
    /// Constructs a new hasher.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PasswordHasher for Pbkdf2PasswordHasher {
    fn verify_password(&self, user: &User, candidate: &str) -> Result<bool, PasswordError> {
        let kind = user.password_hash_kind();
        let rounds = rounds_for(kind);
        let salt = user
            .password_salt()
            .expect("PBKDF2 user has salt — invariant SaltMatchesAlgorithm");
        let Ok(salt_bytes) = hex::decode(salt) else {
            return Ok(false);
        };
        let computed = pbkdf2_hex(candidate.as_bytes(), &salt_bytes, rounds);
        Ok(constant_time_eq(
            computed.as_bytes(),
            user.password_hash().as_bytes(),
        ))
    }

    fn compute_password_hash(
        &self,
        candidate: &str,
        kind: PasswordHashKind,
    ) -> Result<ComputedHash, PasswordError> {
        let rounds = rounds_for(kind);
        let mut salt_bytes = [0u8; SALT_BYTES];
        rand::thread_rng().fill_bytes(&mut salt_bytes);
        let hash = pbkdf2_hex(candidate.as_bytes(), &salt_bytes, rounds);
        Ok(ComputedHash {
            hash,
            salt: Some(hex::encode(salt_bytes)),
        })
    }
}

/// Returns the iteration count for `kind`.
///
/// The match is exhaustive today; later slices that add variants must
/// extend it.
fn rounds_for(kind: PasswordHashKind) -> u32 {
    match kind {
        PasswordHashKind::Pbkdf210000 => PBKDF2_ROUNDS,
    }
}

/// Runs PBKDF2-HMAC-SHA256 and hex-encodes the derived key.
fn pbkdf2_hex(password: &[u8], salt: &[u8], rounds: u32) -> String {
    let mut out = [0u8; HASH_BYTES];
    pbkdf2_hmac::<Sha256>(password, salt, rounds, &mut out);
    hex::encode(out)
}

/// Length-aware constant-time equality on byte slices.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    fn user_with_credentials(hash: String, salt: Option<String>) -> User {
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            hash,
            salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    #[test]
    fn computed_hash_verifies_correct_password() {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash("secret", PasswordHashKind::Pbkdf210000)
            .expect("compute");
        let user = user_with_credentials(computed.hash, computed.salt);
        assert!(hasher.verify_password(&user, "secret").expect("verify"));
    }

    #[test]
    fn wrong_candidate_does_not_verify() {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash("secret", PasswordHashKind::Pbkdf210000)
            .expect("compute");
        let user = user_with_credentials(computed.hash, computed.salt);
        assert!(!hasher.verify_password(&user, "wrong").expect("verify"));
    }

    #[test]
    fn distinct_invocations_use_distinct_salts() {
        let hasher = Pbkdf2PasswordHasher::new();
        let a = hasher
            .compute_password_hash("secret", PasswordHashKind::Pbkdf210000)
            .expect("compute");
        let b = hasher
            .compute_password_hash("secret", PasswordHashKind::Pbkdf210000)
            .expect("compute");
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.hash, b.hash);
    }
}
