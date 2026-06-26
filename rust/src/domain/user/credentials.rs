//! [`Credentials`] value object — stored password hash, salt, kind, and
//! reset-required flag for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module. The [`User`] aggregate
//! delegates to these methods through its public accessors.

use std::time::SystemTime;

use crate::domain::password::PasswordHashKind;
use crate::domain::user::{requires_salt, UserError};

/// Stored credentials and password-reset state for a
/// [`crate::domain::user::User`].
#[derive(Debug, Clone)]
pub(super) struct Credentials {
    /// Algorithm used to verify the stored password hash.
    hash_kind: PasswordHashKind,
    /// Opaque stored password hash.
    hash: String,
    /// Salt the hash was bound to, if the algorithm uses one.
    salt: Option<String>,
    /// Timestamp when the credential triple was last rotated.
    last_updated: SystemTime,
    /// Whether the next logon must force a password change.
    reset: bool,
}

impl Credentials {
    /// Constructs a stored credential set, enforcing the spec's
    /// `SaltMatchesAlgorithm` invariant.
    pub(super) fn new(
        hash_kind: PasswordHashKind,
        hash: String,
        salt: Option<String>,
        last_updated: SystemTime,
    ) -> Result<Self, UserError> {
        if requires_salt(hash_kind) && salt.is_none() {
            return Err(UserError::SaltRequired);
        }
        Ok(Self {
            hash_kind,
            hash,
            salt,
            last_updated,
            reset: false,
        })
    }

    pub(super) fn hash_kind(&self) -> PasswordHashKind {
        self.hash_kind
    }

    pub(super) fn hash(&self) -> &str {
        &self.hash
    }

    pub(super) fn salt(&self) -> Option<&str> {
        self.salt.as_deref()
    }

    pub(super) fn last_updated(&self) -> SystemTime {
        self.last_updated
    }

    pub(super) fn reset_required(&self) -> bool {
        self.reset
    }

    pub(super) fn set_reset_required(&mut self, value: bool) {
        self.reset = value;
    }

    pub(super) fn record_change(
        &mut self,
        hash: String,
        salt: Option<String>,
        kind: PasswordHashKind,
        at: SystemTime,
    ) {
        self.hash = hash;
        self.salt = salt;
        self.hash_kind = kind;
        self.last_updated = at;
        self.reset = false;
    }
}
