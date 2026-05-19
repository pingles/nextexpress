//! [`Bytes`] value type (spec: `core.allium:Bytes`).
//!
//! A non-negative count of octets transferred or budgeted. Per the
//! spec comment the legacy storage choice (BCD packing on 32-bit
//! Amiga integers) is irrelevant at the entity level; in Rust we
//! carry a `u64` count.
//!
//! Slice 48 introduces this value as the type of
//! [`crate::domain::messaging::mail::MailAttachment::file_size`].
//! Slice 50 (Files browse + flag) expands the API.

/// Non-negative count of octets (spec: `core.allium:Bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bytes(u64);

impl Bytes {
    /// Constructs a [`Bytes`] count.
    #[must_use]
    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    /// Returns the raw octet count.
    #[must_use]
    pub const fn count(self) -> u64 {
        self.0
    }
}
