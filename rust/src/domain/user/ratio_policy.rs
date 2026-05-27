//! [`RatioPolicy`] value object — ratio enforcement settings for a
//! [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module.

use crate::domain::user::RatioMode;

/// Ratio enforcement settings for a [`crate::domain::user::User`].
#[derive(Debug, Clone)]
pub(super) struct RatioPolicy {
    /// Active ratio enforcement mode.
    mode: RatioMode,
    /// Ratio threshold (`0` means infinite for an enabled mode).
    value: u32,
}

impl RatioPolicy {
    /// Constructs a disabled ratio policy.
    pub(super) fn disabled() -> Self {
        Self {
            mode: RatioMode::Disabled,
            value: 0,
        }
    }

    /// Constructs a ratio policy from registration/default config or a
    /// persisted snapshot.
    pub(super) fn new(mode: RatioMode, value: u32) -> Self {
        Self { mode, value }
    }

    pub(super) fn mode(&self) -> RatioMode {
        self.mode
    }

    pub(super) fn value(&self) -> u32 {
        self.value
    }
}
