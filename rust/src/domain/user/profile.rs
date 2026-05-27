//! [`Profile`] value object — user-entered profile data and
//! presentation preferences for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::domain::user::UserFlag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiColourPreference {
    Disabled,
    Enabled,
}

impl AnsiColourPreference {
    fn enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl From<bool> for AnsiColourPreference {
    fn from(value: bool) -> Self {
        if value {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

/// User-entered profile data and presentation preferences.
#[derive(Debug, Clone)]
pub(super) struct Profile {
    /// Free-text "City, State" location.
    location: Option<String>,
    /// Phone number on file.
    phone_number: Option<String>,
    /// Email address on file.
    email: Option<String>,
    /// Preferred terminal width (`0` = auto).
    line_length: u32,
    /// Whether the user wants ANSI colour output.
    ansi_colour: AnsiColourPreference,
    /// Timestamp the account was first created.
    account_created: SystemTime,
    /// User preference flags.
    flags: BTreeSet<UserFlag>,
}

impl Profile {
    /// Constructs the profile defaults for an existing account.
    pub(super) fn existing(account_created: SystemTime) -> Self {
        Self {
            location: None,
            phone_number: None,
            email: None,
            line_length: 0,
            ansi_colour: AnsiColourPreference::Disabled,
            account_created,
            flags: BTreeSet::new(),
        }
    }

    /// Constructs a profile from the registration form fields. Reused
    /// by [`crate::domain::user::User::from_persisted`] to thread the
    /// persisted shape through the same constructor — the field set is
    /// identical.
    pub(super) fn registered(
        location: Option<String>,
        phone_number: Option<String>,
        email: Option<String>,
        line_length: u32,
        ansi_colour: bool,
        account_created: SystemTime,
        flags: BTreeSet<UserFlag>,
    ) -> Self {
        Self {
            location,
            phone_number,
            email,
            line_length,
            ansi_colour: AnsiColourPreference::from(ansi_colour),
            account_created,
            flags,
        }
    }

    pub(super) fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    pub(super) fn phone_number(&self) -> Option<&str> {
        self.phone_number.as_deref()
    }

    pub(super) fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    pub(super) fn line_length(&self) -> u32 {
        self.line_length
    }

    pub(super) fn ansi_colour(&self) -> bool {
        self.ansi_colour.enabled()
    }

    pub(super) fn account_created(&self) -> SystemTime {
        self.account_created
    }

    pub(super) fn flags(&self) -> &BTreeSet<UserFlag> {
        &self.flags
    }
}
