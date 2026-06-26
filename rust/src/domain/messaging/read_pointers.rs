//! [`ReadPointers`] entity (spec: `core.allium:ReadPointers`).
//!
//! Phase 6, Slice 38: per-(user, message-base) read state. The legacy
//! BBS packed "new-since" plus a single "last read" pointer into the
//! conference-record; the spec splits them because they answer
//! different questions ("show me what's new since I last looked" vs
//! "where did I get up to in this thread"). Real-time mail scans check
//! both.
//!
//! The user record owns one [`ReadPointers`] row per
//! [`crate::domain::conference::MessageBase`] inside each granted
//! [`crate::domain::conference::ConferenceMembership`]; the
//! `read_pointers_for(user, msgbase)` helper lives on
//! [`crate::domain::user::User`].

use std::time::SystemTime;

/// Per-(user, message-base) read state
/// (spec: `core.allium:ReadPointers`).
///
/// All three pointers default to zero (`last_read` / `last_scanned`)
/// or the UNIX epoch (`new_since`) on a fresh row; the spec invariant
/// `ReadDoesNotExceedScanned` (`last_read <= last_scanned`) is
/// enforced both at construction time and on subsequent mutations.
///
/// The parent `MessageBase` is identified by its 1-indexed number
/// inside its `ConferenceMembership`; the conference number is
/// implicit from the membership the row hangs off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPointers {
    msgbase_number: u32,
    last_read: u32,
    last_scanned: u32,
    new_since: SystemTime,
}

impl ReadPointers {
    /// Constructs a [`ReadPointers`] row for `msgbase_number` with the
    /// given pointer values.
    ///
    /// # Errors
    /// Returns [`ReadPointersError::ReadExceedsScanned`] when
    /// `last_read > last_scanned` (spec invariant
    /// `ReadDoesNotExceedScanned`).
    pub fn new(
        msgbase_number: u32,
        last_read: u32,
        last_scanned: u32,
        new_since: SystemTime,
    ) -> Result<Self, ReadPointersError> {
        if last_read > last_scanned {
            return Err(ReadPointersError::ReadExceedsScanned {
                last_read,
                last_scanned,
            });
        }
        Ok(Self {
            msgbase_number,
            last_read,
            last_scanned,
            new_since,
        })
    }

    /// Constructs a fresh row for `msgbase_number` with both pointers
    /// at `0` and `new_since` set to `created_at`. Convenience for the
    /// "first time this user has ever touched this base" case.
    #[must_use]
    pub fn fresh(msgbase_number: u32, created_at: SystemTime) -> Self {
        Self {
            msgbase_number,
            last_read: 0,
            last_scanned: 0,
            new_since: created_at,
        }
    }

    /// Returns the 1-indexed message base this row pertains to.
    #[must_use]
    pub fn msgbase_number(&self) -> u32 {
        self.msgbase_number
    }

    /// Returns the highest message number the user has actually read.
    #[must_use]
    pub fn last_read(&self) -> u32 {
        self.last_read
    }

    /// Returns the highest message number the auto-scan has surfaced
    /// to the user.
    #[must_use]
    pub fn last_scanned(&self) -> u32 {
        self.last_scanned
    }

    /// Returns the "show messages newer than this" cut-off timestamp.
    #[must_use]
    pub fn new_since(&self) -> SystemTime {
        self.new_since
    }

    /// Advances `last_read` toward `to`, lifting `last_scanned` if
    /// necessary to keep `ReadDoesNotExceedScanned`. Movement is
    /// monotonic forward: a `to` value at or below the current
    /// `last_read` is a no-op.
    ///
    /// Mirrors `messaging.allium:ReadMail`'s
    /// `if mail.number > pointers.last_read: pointers.last_read = mail.number`
    /// consequent.
    pub fn advance_last_read(&mut self, to: u32) {
        self.last_read = self.last_read.max(to);
        self.last_scanned = self.last_scanned.max(self.last_read);
    }

    /// Advances `last_scanned` toward `to`. Movement is monotonic
    /// forward: a `to` value at or below the current `last_scanned`
    /// is a no-op. Mirrors `messaging.allium:ScanMail`'s
    /// `pointers.last_scanned = max_of(pointers.last_scanned, msgbase.highest_message)`.
    pub fn advance_last_scanned(&mut self, to: u32) {
        self.last_scanned = self.last_scanned.max(to);
    }

    /// Replaces the `new_since` cut-off. Used by `M`-prompts that let
    /// the user pick "messages since DATE".
    pub fn set_new_since(&mut self, when: SystemTime) {
        self.new_since = when;
    }
}

/// Errors returned by [`ReadPointers::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadPointersError {
    /// `last_read > last_scanned`, violating the spec invariant
    /// `ReadDoesNotExceedScanned`.
    #[error(
        "ReadPointers would violate ReadDoesNotExceedScanned: \
         last_read={last_read} > last_scanned={last_scanned}"
    )]
    ReadExceedsScanned {
        /// Offending `last_read` value.
        last_read: u32,
        /// `last_scanned` ceiling the value would breach.
        last_scanned: u32,
    },
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::domain::messaging::mail_store::test_support::t;

    #[test]
    fn new_round_trips_basic_fields() {
        // Pick non-1, distinct values for every numeric field so a
        // constant-folded accessor mutant cannot slip past.
        let p = ReadPointers::new(2, 5, 7, t(100)).expect("valid");

        assert_eq!(p.msgbase_number(), 2);
        assert_eq!(p.last_read(), 5);
        assert_eq!(p.last_scanned(), 7);
        assert_eq!(p.new_since(), t(100));
    }

    #[test]
    fn new_accepts_zero_pointers_and_epoch_timestamp() {
        // Fresh row: nothing read, nothing scanned, new_since at the
        // epoch. The constructor must not reject this case.
        let p = ReadPointers::new(1, 0, 0, t(0)).expect("valid");
        assert_eq!(p.last_read(), 0);
        assert_eq!(p.last_scanned(), 0);
        assert_eq!(p.new_since(), t(0));
    }

    #[test]
    fn new_accepts_equal_pointers() {
        // `last_read = last_scanned` is the boundary case of
        // ReadDoesNotExceedScanned (`<=`, not `<`).
        let p = ReadPointers::new(1, 4, 4, t(0)).expect("valid");
        assert_eq!(p.last_read(), 4);
        assert_eq!(p.last_scanned(), 4);
    }

    #[test]
    fn new_rejects_last_read_exceeding_last_scanned() {
        // Spec invariant ReadDoesNotExceedScanned: last_read <= last_scanned.
        let err = ReadPointers::new(1, 10, 5, t(0)).expect_err("invariant violation");
        assert_eq!(
            err,
            ReadPointersError::ReadExceedsScanned {
                last_read: 10,
                last_scanned: 5,
            }
        );
    }

    #[test]
    fn fresh_starts_with_zero_pointers_and_supplied_timestamp() {
        let p = ReadPointers::fresh(3, t(42));
        assert_eq!(p.msgbase_number(), 3);
        assert_eq!(p.last_read(), 0);
        assert_eq!(p.last_scanned(), 0);
        assert_eq!(p.new_since(), t(42));
    }

    #[test]
    fn advance_last_read_moves_pointer_forward() {
        let mut p = ReadPointers::new(1, 3, 5, t(0)).expect("valid");
        p.advance_last_read(4);
        assert_eq!(p.last_read(), 4);
        // last_scanned was already higher than 4 — must not move.
        assert_eq!(p.last_scanned(), 5);
    }

    #[test]
    fn advance_last_read_past_last_scanned_lifts_last_scanned() {
        // Spec ReadMail can in principle advance last_read past
        // last_scanned (the user opens an arbitrary message number).
        // The invariant ReadDoesNotExceedScanned must still hold, so
        // last_scanned must rise to match.
        let mut p = ReadPointers::new(1, 3, 5, t(0)).expect("valid");
        p.advance_last_read(7);
        assert_eq!(p.last_read(), 7);
        assert_eq!(p.last_scanned(), 7);
    }

    #[test]
    fn advance_last_read_is_monotonic_forward() {
        // The legacy code never rewinds last_read; a stale advance
        // request (e.g. user re-reads an older message) must not pull
        // the pointer back.
        let mut p = ReadPointers::new(1, 5, 10, t(0)).expect("valid");
        p.advance_last_read(3);
        assert_eq!(p.last_read(), 5);
        assert_eq!(p.last_scanned(), 10);
    }

    #[test]
    fn advance_last_read_no_op_at_current_value() {
        let mut p = ReadPointers::new(1, 5, 10, t(0)).expect("valid");
        p.advance_last_read(5);
        assert_eq!(p.last_read(), 5);
        assert_eq!(p.last_scanned(), 10);
    }

    #[test]
    fn advance_last_scanned_moves_pointer_forward() {
        let mut p = ReadPointers::new(1, 0, 3, t(0)).expect("valid");
        p.advance_last_scanned(8);
        assert_eq!(p.last_scanned(), 8);
        // last_read must not move; ScanMail does not touch it.
        assert_eq!(p.last_read(), 0);
    }

    #[test]
    fn advance_last_scanned_is_monotonic_forward() {
        let mut p = ReadPointers::new(1, 0, 10, t(0)).expect("valid");
        p.advance_last_scanned(3);
        assert_eq!(p.last_scanned(), 10);
    }

    #[test]
    fn advance_last_scanned_no_op_at_current_value() {
        let mut p = ReadPointers::new(1, 0, 5, t(0)).expect("valid");
        p.advance_last_scanned(5);
        assert_eq!(p.last_scanned(), 5);
    }

    #[test]
    fn set_new_since_replaces_timestamp() {
        let mut p = ReadPointers::new(1, 0, 0, t(0)).expect("valid");
        p.set_new_since(t(500));
        assert_eq!(p.new_since(), t(500));
    }
}
