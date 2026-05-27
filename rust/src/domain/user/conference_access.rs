//! [`ConferenceAccess`] value object — conference memberships, current
//! position, and messaging counters for a [`crate::domain::user::User`].
//!
//! Private to the `domain::user` module.

use crate::domain::conference::{Conference, ConferenceMembership, MessageBase, MessageBaseRef};
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
}
