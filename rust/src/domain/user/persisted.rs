//! [`PersistedUser`] — snapshot of a [`crate::domain::user::User`] as
//! read from durable storage.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use crate::domain::conference::{ConferenceMembership, MessageBaseRef};
use crate::domain::password::PasswordHashKind;
use crate::domain::user::{RatioMode, UserFlag};

/// Complete snapshot of a [`crate::domain::user::User`]'s persistent
/// state as read from durable storage.
///
/// Sibling to [`crate::domain::user::NewUserDraft`]: where that bundles
/// fresh-account fields, this bundles every piece of state the BBS
/// persists for an already-existing account. A storage adapter (e.g.
/// [`crate::adapters::sqlite_user_repository::SqliteUserRepository`])
/// reads its columns into a [`PersistedUser`] and constructs a
/// [`crate::domain::user::User`] via
/// [`crate::domain::user::User::from_persisted`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "this struct mirrors a row of boolean columns persisted in the user store; \
              compressing them into enums would obscure the schema mapping"
)]
pub struct PersistedUser {
    /// Stable account id; `1` is the sysop.
    pub slot_number: u32,
    /// Unique login name.
    pub handle: String,
    /// Algorithm used to verify the stored password hash.
    pub password_hash_kind: PasswordHashKind,
    /// Opaque stored password hash.
    pub password_hash: String,
    /// Salt the hash was bound to (`None` for hash kinds that don't
    /// take one).
    pub password_salt: Option<String>,
    /// Timestamp when the credential triple was last rotated.
    pub password_last_updated: SystemTime,
    /// Whether the next logon must force a password change.
    pub force_password_reset: bool,
    /// `0..=255` access tier (`0` = locked out).
    pub access_level: u8,
    /// Recent invalid password attempts.
    pub invalid_attempts: u32,
    /// Independent account-lock flag set by lockout rules/admin tools.
    pub account_locked: bool,
    /// Whether the account is awaiting sysop validation.
    pub is_new_user: bool,
    /// Whether the user's posts are silently downgraded to
    /// `private_to_sysop`.
    pub censored: bool,
    /// Number of completed logons recorded for this user.
    pub times_called: u32,
    /// Number of completed logons in the current accounting day.
    pub times_called_today: u32,
    /// Timestamp of the most recently completed logon, if any.
    pub last_call: Option<SystemTime>,
    /// Per-call wall-clock allowance.
    pub time_limit_per_call: Duration,
    /// Combined per-day allowance.
    pub time_limit_per_day: Duration,
    /// Wall-clock time used today.
    pub time_used_today: Duration,
    /// Free-text "City, State" location.
    pub location: Option<String>,
    /// Phone number.
    pub phone_number: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Preferred terminal width (`0` = auto).
    pub line_length: u32,
    /// Whether the user wants ANSI colour output.
    pub ansi_colour: bool,
    /// Whether the user is in expert mode (Tier A quickwin A6).
    pub expert_mode: bool,
    /// Timestamp the account was first created.
    pub account_created: SystemTime,
    /// User preference flags.
    pub flags: BTreeSet<UserFlag>,
    /// Ratio enforcement mode in effect for this user.
    pub ratio_mode: RatioMode,
    /// Ratio threshold (`0` with non-disabled mode = infinite).
    pub ratio_value: u32,
    /// Per-conference membership rows (already including any read
    /// pointer rows).
    pub memberships: Vec<ConferenceMembership>,
    /// Last joined `(conference, msgbase)` pair, if any.
    pub last_joined: Option<MessageBaseRef>,
    /// Running count of posted messages across all conferences.
    pub messages_posted: u32,
}
