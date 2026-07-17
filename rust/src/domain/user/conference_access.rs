//! [`ConferenceAccess`] value object — conference memberships, current
//! position, and messaging counters for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module.

use std::time::SystemTime;

use crate::domain::conference::{
    Conference, ConferenceMembership, MessageBase, MessageBaseRef, ScanFlag,
};
use crate::domain::messaging::read_pointers::ReadPointers;

/// Conference memberships, current position, and messaging counters.
#[derive(Debug, Clone)]
pub(super) struct ConferenceAccess {
    /// Per-conference membership rows.
    memberships: Vec<ConferenceMembership>,
    /// Last joined `(conference, msgbase)` pair.
    last_joined: Option<MessageBaseRef>,
    /// Running count of posted messages across all conferences.
    messages_posted: u32,
}

impl ConferenceAccess {
    /// Constructs an empty conference-access record.
    pub(super) fn new() -> Self {
        Self {
            memberships: Vec::new(),
            last_joined: None,
            messages_posted: 0,
        }
    }

    /// Reconstructs the conference-access record from a persisted
    /// snapshot. Used by
    /// [`crate::domain::user::User::from_persisted`].
    pub(super) fn from_persisted(
        memberships: Vec<ConferenceMembership>,
        last_joined: Option<MessageBaseRef>,
        messages_posted: u32,
    ) -> Self {
        Self {
            memberships,
            last_joined,
            messages_posted,
        }
    }

    pub(super) fn memberships(&self) -> &[ConferenceMembership] {
        &self.memberships
    }

    pub(super) fn memberships_mut(&mut self) -> &mut [ConferenceMembership] {
        &mut self.memberships
    }

    pub(super) fn upsert_membership(&mut self, membership: ConferenceMembership) {
        if let Some(existing) = self
            .memberships
            .iter_mut()
            .find(|m| m.conference_number() == membership.conference_number())
        {
            *existing = membership;
        } else {
            self.memberships.push(membership);
        }
    }

    pub(super) fn set_membership_granted(&mut self, conference_number: u32, granted: bool) -> bool {
        if let Some(existing) = self
            .memberships
            .iter_mut()
            .find(|m| m.conference_number() == conference_number)
        {
            existing.set_granted(granted);
            true
        } else {
            false
        }
    }

    pub(super) fn has_membership(&self, conference: &Conference) -> bool {
        crate::domain::conference::has_membership(&self.memberships, conference)
    }

    /// Reads a per-conference [`ScanFlag`] off the membership row for
    /// `conference_number`. Returns `false` when no row exists; a row's
    /// `granted` state is deliberately ignored (revoked rows retain
    /// their scan preferences).
    pub(super) fn scan_flag_for(&self, conference_number: u32, flag: ScanFlag) -> bool {
        self.memberships
            .iter()
            .find(|m| m.conference_number() == conference_number)
            .is_some_and(|m| m.scan_flag(flag))
    }

    pub(super) fn has_granted_membership_for(&self, conference_number: u32) -> bool {
        self.memberships
            .iter()
            .any(|m| m.conference_number() == conference_number && m.is_granted())
    }

    pub(super) fn last_joined(&self) -> Option<MessageBaseRef> {
        self.last_joined
    }

    pub(super) fn record_join(&mut self, conference: &Conference, msgbase: &MessageBase) {
        self.last_joined = Some(MessageBaseRef::new(conference.number(), msgbase.number()));
    }

    pub(super) fn read_pointers_for(&self, msgbase: MessageBaseRef) -> Option<&ReadPointers> {
        self.memberships
            .iter()
            .find(|m| m.conference_number() == msgbase.conference_number())
            .and_then(|m| m.pointers_for(msgbase.msgbase_number()))
    }

    pub(super) fn read_pointers_for_mut(
        &mut self,
        msgbase: MessageBaseRef,
    ) -> Option<&mut ReadPointers> {
        self.memberships
            .iter_mut()
            .find(|m| m.conference_number() == msgbase.conference_number())
            .and_then(|m| m.pointers_for_mut(msgbase.msgbase_number()))
    }

    /// Advances the `last_read` pointer for `msgbase` toward `to`,
    /// lazily creating the [`ReadPointers`] row (with `new_since = now`)
    /// when the membership has none yet. Returns `false` when the user
    /// has no membership row for the parent conference.
    pub(super) fn advance_last_read(
        &mut self,
        msgbase: MessageBaseRef,
        to: u32,
        now: SystemTime,
    ) -> bool {
        self.advance_pointers(msgbase, now, |row| row.advance_last_read(to))
    }

    /// Advances the `last_scanned` pointer for `msgbase` toward `to`,
    /// lazily creating the [`ReadPointers`] row (with `new_since = now`)
    /// when the membership has none yet. Returns `false` when the user
    /// has no membership row for the parent conference.
    pub(super) fn advance_last_scanned(
        &mut self,
        msgbase: MessageBaseRef,
        to: u32,
        now: SystemTime,
    ) -> bool {
        self.advance_pointers(msgbase, now, |row| row.advance_last_scanned(to))
    }

    /// Shared get-or-lazily-create step behind
    /// [`Self::advance_last_read`] / [`Self::advance_last_scanned`]:
    /// finds the membership row for `msgbase`'s conference, creates a
    /// [`ReadPointers::fresh`] row (stamped `now`) when the base has
    /// none, applies `advance` to the row, and reports whether a
    /// membership row existed. Keeping one implementation here means
    /// the lazy-create invariant shared by `ReadMail`, `ScanMail`, and
    /// `ScanMailOnJoin` has a single owner-side home.
    fn advance_pointers(
        &mut self,
        msgbase: MessageBaseRef,
        now: SystemTime,
        advance: impl FnOnce(&mut ReadPointers),
    ) -> bool {
        let Some(membership) = self
            .memberships
            .iter_mut()
            .find(|m| m.conference_number() == msgbase.conference_number())
        else {
            return false;
        };
        if membership.pointers_for(msgbase.msgbase_number()).is_none() {
            membership.upsert_pointers(ReadPointers::fresh(msgbase.msgbase_number(), now));
        }
        let row = membership
            .pointers_for_mut(msgbase.msgbase_number())
            .expect("row exists or was just created");
        advance(row);
        true
    }

    pub(super) fn upsert_read_pointers(
        &mut self,
        pointers: ReadPointers,
        conference_number: u32,
    ) -> bool {
        let Some(membership) = self
            .memberships
            .iter_mut()
            .find(|m| m.conference_number() == conference_number)
        else {
            return false;
        };
        membership.upsert_pointers(pointers);
        true
    }

    pub(super) fn messages_posted(&self) -> u32 {
        self.messages_posted
    }

    pub(super) fn bump_messages_posted(&mut self) {
        self.messages_posted = self.messages_posted.saturating_add(1);
    }

    /// Records one posted message: bumps the cross-conference total
    /// unconditionally, then the tally on the membership row for
    /// `conference_number` when one exists (matched by number only —
    /// the spec consequent says `if exists membership`, not "granted").
    /// Returns whether a membership row was found.
    pub(super) fn record_message_posted(&mut self, conference_number: u32) -> bool {
        self.bump_messages_posted();
        if let Some(membership) = self
            .memberships
            .iter_mut()
            .find(|m| m.conference_number() == conference_number)
        {
            membership.bump_messages_posted();
            true
        } else {
            false
        }
    }
}
