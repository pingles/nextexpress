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

use crate::domain::conference::{Conference, ConferenceMembership};
use crate::domain::password::{PasswordError, PasswordHashKind, PasswordHasher};
use crate::domain::user::{User, UserError};

/// Errors returned by [`default_sysop`].
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    /// The hasher couldn't compute a hash for the seed credential.
    #[error("couldn't hash seed credential: {0}")]
    Hash(#[source] PasswordError),
    /// The freshly hashed credential triple failed [`User::new`]'s
    /// invariants. This should never happen for the spec-default
    /// PBKDF2 hash, but is propagated rather than panicking.
    #[error("couldn't construct seeded user: {0}")]
    User(#[source] UserError),
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

/// Grants `user` a `granted = true` [`ConferenceMembership`] for every
/// conference in `conferences` (Slice 34a). Used by the composition
/// root so the seeded sysop can auto-rejoin into a freshly bootstrapped
/// catalogue without a separate admin step. Pre-existing rows for the
/// same conference are upserted to `granted = true`.
pub fn grant_all_memberships(user: &mut User, conferences: &[Conference]) {
    for conference in conferences {
        user.upsert_membership(ConferenceMembership::new(conference.number(), true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::domain::conference::MessageBase;

    fn make_conf(number: u32) -> Conference {
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    #[test]
    fn grant_all_memberships_adds_a_granted_row_per_conference() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        grant_all_memberships(&mut user, &confs);
        for conf in &confs {
            assert!(
                user.has_membership(conf),
                "missing grant for {}",
                conf.number()
            );
        }
    }

    #[test]
    fn grant_all_memberships_upserts_existing_rows_to_granted_true() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        // Pre-existing revoked row for conf 1.
        user.upsert_membership(ConferenceMembership::new(1, false));
        let confs = vec![make_conf(1)];
        grant_all_memberships(&mut user, &confs);
        assert!(user.has_membership(&confs[0]));
    }

    #[test]
    fn grant_all_memberships_with_empty_catalogue_is_a_noop() {
        let hasher = Pbkdf2PasswordHasher::new();
        let mut user = default_sysop(&hasher).expect("seed");
        grant_all_memberships(&mut user, &[]);
        assert!(user.memberships().is_empty());
    }

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
