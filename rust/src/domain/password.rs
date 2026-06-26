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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    /// The hasher does not implement the requested
    /// [`PasswordHashKind`]. Phase 1 only ships
    /// [`PasswordHashKind::Pbkdf210000`]; Slice 64 fills in the rest.
    #[error("unsupported password hash kind: {0:?}")]
    UnsupportedHashKind(PasswordHashKind),
}

/// `session.allium:meets_password_strength` black-box helper (Slice 15).
///
/// Returns `true` when `candidate` meets both length and category
/// thresholds. Mirrors the legacy `checkPasswordStrength` at
/// `amiexpress/express.e:908`:
/// - if `min_length > 0`, the candidate must be at least that many
///   characters,
/// - if `min_categories > 0`, the candidate must use at least that
///   many distinct categories, capped at four — lowercase letter,
///   uppercase letter, ASCII digit, anything else (symbol).
///
/// Both thresholds are independently disabled at zero, matching the
/// legacy convention where unset config values gate out the check.
///
/// # Parameters
/// - `candidate`: the plaintext password.
/// - `min_length`: minimum character count.
/// - `min_categories`: minimum distinct character categories.
#[must_use]
pub fn meets_password_strength(candidate: &str, min_length: u32, min_categories: u32) -> bool {
    // `min_length == 0` disables the check: `min_len` is then 0, and a
    // `char` count can never be `< 0`, so no explicit guard is needed.
    let min_len = usize::try_from(min_length).unwrap_or(usize::MAX);
    if candidate.chars().count() < min_len {
        return false;
    }
    if min_categories > 0 {
        let cap = min_categories.min(4);
        let mut lower = false;
        let mut upper = false;
        let mut digit = false;
        let mut symbol = false;
        for ch in candidate.chars() {
            if ch.is_ascii_digit() {
                digit = true;
            } else if ch.is_ascii_uppercase() {
                upper = true;
            } else if ch.is_ascii_lowercase() {
                lower = true;
            } else {
                symbol = true;
            }
        }
        let categories = u32::from(lower) + u32::from(upper) + u32::from(digit) + u32::from(symbol);
        if categories < cap {
            return false;
        }
    }
    true
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_disabled_passes_any_length() {
        assert!(meets_password_strength("", 0, 0));
        assert!(meets_password_strength("ab", 0, 0));
    }

    #[test]
    fn length_enforced_rejects_short() {
        assert!(!meets_password_strength("abc", 6, 0));
        assert!(meets_password_strength("abcdef", 6, 0));
        assert!(meets_password_strength("abcdefg", 6, 0));
    }

    #[test]
    fn categories_disabled_passes_any_charset() {
        assert!(meets_password_strength("aaaaaa", 6, 0));
    }

    #[test]
    fn one_category_passes_single_class() {
        assert!(meets_password_strength("abcdef", 0, 1));
        assert!(meets_password_strength("123456", 0, 1));
    }

    #[test]
    fn two_categories_requires_mix() {
        assert!(!meets_password_strength("abcdef", 0, 2));
        assert!(meets_password_strength("abc123", 0, 2));
        assert!(meets_password_strength("Abcdef", 0, 2));
        assert!(meets_password_strength("abc!@#", 0, 2));
    }

    #[test]
    fn three_categories_requires_three_classes() {
        assert!(!meets_password_strength("abc123", 0, 3));
        assert!(meets_password_strength("Abc123", 0, 3));
        assert!(meets_password_strength("abc12!", 0, 3));
    }

    #[test]
    fn four_categories_requires_all_classes() {
        assert!(!meets_password_strength("Abc123", 0, 4));
        assert!(meets_password_strength("Abc12!", 0, 4));
    }

    #[test]
    fn min_categories_caps_at_four() {
        // Mirrors `IF min>4 THEN min:=4` at amiexpress/express.e:918.
        assert!(meets_password_strength("Abc12!", 0, 100));
    }

    #[test]
    fn length_and_categories_both_enforced() {
        assert!(!meets_password_strength("Ab1!", 6, 4));
        assert!(meets_password_strength("Ab123!", 6, 4));
    }

    #[test]
    fn non_ascii_letters_count_as_symbols() {
        // The legacy ASCII categorisation classifies anything
        // outside ASCII letters/digits as "symbol". Mirrored here.
        assert!(meets_password_strength("héllo", 0, 2));
    }
}
