//! [`NewUserDraft`] — input bundle for
//! [`crate::domain::user::User::register_new`] and
//! [`crate::domain::user_repository::UserRepository::create_user`].

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::domain::password::PasswordHashKind;
use crate::domain::user::{RatioMode, UserFlag};

/// Bundle of fields collected during the new-user registration
/// sub-flow, plus the freshly computed password hash, that
/// [`crate::domain::user::User::register_new`] consumes.
///
/// Mirrors the `profile` argument of
/// `session.allium:CompleteNewUserRegistration`. The slot number is
/// not part of the draft — the repository's
/// [`crate::domain::user_repository::UserRepository::create_user`]
/// allocates it inside its own transaction and threads it into
/// `register_new(slot, draft)`. Ratio defaults come from
/// `core/config.default_ratio_*`.
#[derive(Debug, Clone)]
pub struct NewUserDraft {
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
