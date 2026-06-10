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

    /// Adds two byte counts, clamping at `u64::MAX`.
    ///
    /// The spec's transfer tallies accumulate byte totals
    /// (`files.allium:270-281` download tallies, `:355-361` upload
    /// tallies); a count can never exceed the carrier type.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Subtracts a byte count, flooring at zero.
    ///
    /// Mirrors the spec's guarded subtraction: `CheckDownloadEligibility`
    /// computes `bytes_remaining_after` with an explicit zero floor
    /// (`files.allium:227-232`), and the non-negativity invariants
    /// (`files.allium:497-505`) forbid a negative count.
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturating_add_sums_counts() {
        let sum = Bytes::new(1_000).saturating_add(Bytes::new(234));
        assert_eq!(sum, Bytes::new(1_234));
    }

    #[test]
    fn saturating_add_clamps_at_u64_max() {
        let sum = Bytes::new(u64::MAX - 1).saturating_add(Bytes::new(5));
        assert_eq!(sum, Bytes::new(u64::MAX));
    }

    #[test]
    fn saturating_sub_subtracts_counts() {
        let diff = Bytes::new(1_234).saturating_sub(Bytes::new(234));
        assert_eq!(diff, Bytes::new(1_000));
    }

    #[test]
    fn saturating_sub_floors_at_zero() {
        let diff = Bytes::new(234).saturating_sub(Bytes::new(1_000));
        assert_eq!(diff, Bytes::new(0));
    }
}
