//! [`Mail`] entity (spec: `messaging.allium:Mail`).
//!
//! Phase 6, Slice 37 introduces the persistent message and its on-disk
//! store. Per the schema-growth principle in [`SLICES.md`], only the
//! fields the rules in this slice touch are present:
//!
//! - all header fields named in `messaging.allium:Mail` (`number`,
//!   `visibility`, `from_name`, `to_name`, `broadcast_to`, `subject`,
//!   `posted_at`, `received_at`, `author`, `addressee`, `body`)
//! - the parent [`MessageBaseRef`] coordinate
//!
//! `MailAttachment` (Slice 48) and the `ext_msg_num` field used by the
//! `ExternalMessagesHaveExtId` invariant (deferred until external
//! message bases land) are intentionally omitted.

use std::time::SystemTime;

use crate::domain::bytes::Bytes;
use crate::domain::conference::{AllowedAddressing, MessageBaseRef};

/// Visibility of a [`Mail`] (spec: `messaging.allium:MailVisibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailVisibility {
    /// Visible to anyone with conference access.
    Public,
    /// Visible to the addressee and the sysop only.
    Private,
    /// Censored author; only the sysop reads it.
    PrivateToSysop,
    /// Soft-deleted; not shown to ordinary readers.
    Deleted,
}

impl MailVisibility {
    /// Single-character glyph used by the mail listing screen
    /// (spec `messaging.allium:MailVisibility` comment: "the sysop
    /// sees the same letter glyph in lower case on the listing
    /// screen"). Slice 47 introduces the lowercase `p` variant for
    /// censored mail.
    ///
    /// - [`Public`] → `' '` (unmarked — the default).
    /// - [`Private`] → `'P'`.
    /// - [`PrivateToSysop`] → `'p'` (lowercase; spec wording).
    /// - [`Deleted`] → `'D'` (defensive; ordinary readers never see deleted mail).
    ///
    /// [`Public`]: MailVisibility::Public
    /// [`Private`]: MailVisibility::Private
    /// [`PrivateToSysop`]: MailVisibility::PrivateToSysop
    /// [`Deleted`]: MailVisibility::Deleted
    #[must_use]
    pub fn status_glyph(self) -> char {
        match self {
            MailVisibility::Public => ' ',
            MailVisibility::Private => 'P',
            MailVisibility::PrivateToSysop => 'p',
            MailVisibility::Deleted => 'D',
        }
    }
}

/// Special addressees that aren't user handles (spec:
/// `messaging.allium:BroadcastTo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BroadcastTo {
    /// Normal user addressee.
    None,
    /// "ALL" — everyone in this conference.
    All,
    /// "EALL" — everyone across all conferences (echo-all).
    Eall,
}

/// True when a message base configured with `allowed` accepts a
/// post addressed with `broadcast` (spec:
/// `messaging.allium:addressing_allows`).
///
/// Individual addressees (`BroadcastTo::None`) are always permitted —
/// no `AllowedAddressing` variant forbids per-user mail. The ALL and
/// EALL branches gate against the variant.
#[must_use]
pub fn addressing_allows(allowed: AllowedAddressing, broadcast: BroadcastTo) -> bool {
    match broadcast {
        BroadcastTo::None => true,
        BroadcastTo::All => matches!(
            allowed,
            AllowedAddressing::IndividualOrAll | AllowedAddressing::Any
        ),
        BroadcastTo::Eall => matches!(
            allowed,
            AllowedAddressing::IndividualOrEall | AllowedAddressing::Any
        ),
    }
}

/// Caller-supplied payload for posting a new message
/// (spec: `messaging.allium:PostMail` consequent fields, minus the
/// store-assigned `number` and the parent `msgbase`).
///
/// The store fills in `msgbase` (from its own configuration) and
/// `number` (allocated as `highest_message + 1`) when turning a
/// [`MailDraft`] into a persisted [`Mail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraft {
    /// Mail visibility at post time.
    pub visibility: MailVisibility,
    /// Author's display name (caller honours `FromNameMatchesAuthor`).
    pub from_name: String,
    /// Addressee's typed name, or `"ALL"` / `"EALL"` for broadcasts.
    pub to_name: String,
    /// Broadcast discriminator on `to_name`.
    pub broadcast_to: BroadcastTo,
    /// Free-text subject.
    pub subject: String,
    /// When the message was posted.
    pub posted_at: SystemTime,
    /// Author's stable slot number.
    pub author_slot: u32,
    /// Addressee's stable slot number, or `None` for ALL / EALL.
    pub addressee_slot: Option<u32>,
    /// Message body.
    pub body: String,
}

/// Constructor payload for [`Mail::new`].
///
/// The spec's `Mail` entity has eleven header fields plus a body; a
/// struct-of-parameters pattern (matching
/// [`crate::domain::user::NewUserDraft`]) keeps construction
/// readable at call sites and side-steps `clippy::too_many_arguments`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMail {
    /// Parent message base (spec: `Mail.msgbase`).
    pub msgbase: MessageBaseRef,
    /// 1-indexed message number, unique within the message base
    /// (spec: `Mail.number`, invariant `MessageNumbersUniquePerBase`).
    pub number: u32,
    /// Mail visibility at post time (spec: `Mail.visibility`).
    pub visibility: MailVisibility,
    /// Author's display name (spec: `Mail.from_name`).
    ///
    /// The caller is responsible for honouring the
    /// `FromNameMatchesAuthor` invariant — i.e. supplying
    /// `display_name_of(author, msgbase.conference)`.
    pub from_name: String,
    /// Addressee's typed name, or `"ALL"` / `"EALL"` for broadcasts
    /// (spec: `Mail.to_name`).
    pub to_name: String,
    /// Broadcast discriminator on `to_name`
    /// (spec: `Mail.broadcast_to`).
    pub broadcast_to: BroadcastTo,
    /// Free-text subject (spec: `Mail.subject`).
    pub subject: String,
    /// When the message was posted (spec: `Mail.posted_at`).
    pub posted_at: SystemTime,
    /// Author's stable slot number (spec: `Mail.author`).
    pub author_slot: u32,
    /// Addressee's stable slot number, or `None` for ALL / EALL
    /// (spec: `Mail.addressee`, constrained `when broadcast_to = none`).
    pub addressee_slot: Option<u32>,
    /// Message body (spec: `Mail.body`).
    pub body: String,
}

/// A file attached to a [`Mail`] (spec:
/// `messaging.allium:MailAttachment`).
///
/// The attached file itself lives outside the spec (in the file
/// area, Phase 9). The attachment row is just the
/// `(file_name, file_size)` pair tagged onto the host mail —
/// Slice 48 only models the metadata; the wire transfer that
/// materialises the file is Phase 10's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailAttachment {
    file_name: String,
    file_size: Bytes,
}

impl MailAttachment {
    /// Constructs a [`MailAttachment`] from its file name and size.
    #[must_use]
    pub fn new(file_name: String, file_size: Bytes) -> Self {
        Self {
            file_name,
            file_size,
        }
    }

    /// Returns the attached file's name as it lives in the file area.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Returns the attached file's [`Bytes`] size.
    #[must_use]
    pub fn file_size(&self) -> Bytes {
        self.file_size
    }
}

/// A persistent mail message (spec: `messaging.allium:Mail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    msgbase: MessageBaseRef,
    number: u32,
    visibility: MailVisibility,
    from_name: String,
    to_name: String,
    broadcast_to: BroadcastTo,
    subject: String,
    posted_at: SystemTime,
    received_at: Option<SystemTime>,
    author_slot: u32,
    addressee_slot: Option<u32>,
    body: String,
    attachments: Vec<MailAttachment>,
}

impl Mail {
    /// Constructs a [`Mail`] from a store-allocated number, the parent
    /// [`MessageBaseRef`] and a caller-supplied [`MailDraft`].
    ///
    /// `received_at` starts as `None` (per `messaging.allium:PostMail`'s
    /// `ensures: received_at: null`).
    #[must_use]
    pub fn from_draft(msgbase: MessageBaseRef, number: u32, draft: MailDraft) -> Self {
        Self::new(NewMail {
            msgbase,
            number,
            visibility: draft.visibility,
            from_name: draft.from_name,
            to_name: draft.to_name,
            broadcast_to: draft.broadcast_to,
            subject: draft.subject,
            posted_at: draft.posted_at,
            author_slot: draft.author_slot,
            addressee_slot: draft.addressee_slot,
            body: draft.body,
        })
    }

    /// Constructs a freshly-posted [`Mail`] (spec: `Mail.created`).
    ///
    /// `received_at` starts as `None` (per `messaging.allium:PostMail`'s
    /// `ensures: received_at: null`).
    #[must_use]
    pub fn new(payload: NewMail) -> Self {
        let NewMail {
            msgbase,
            number,
            visibility,
            from_name,
            to_name,
            broadcast_to,
            subject,
            posted_at,
            author_slot,
            addressee_slot,
            body,
        } = payload;
        Self {
            msgbase,
            number,
            visibility,
            from_name,
            to_name,
            broadcast_to,
            subject,
            posted_at,
            received_at: None,
            author_slot,
            addressee_slot,
            body,
            attachments: Vec::new(),
        }
    }

    /// Returns the parent message base coordinate.
    #[must_use]
    pub fn msgbase(&self) -> MessageBaseRef {
        self.msgbase
    }

    /// Returns the message's attachments (spec:
    /// `messaging.allium:Mail.attachments`). Slice 48 only adds
    /// the metadata rows; the wire transfer lands with Phase 10.
    #[must_use]
    pub fn attachments(&self) -> &[MailAttachment] {
        &self.attachments
    }

    /// Appends an attachment row to this mail (spec:
    /// `messaging.allium:MailAttachment.created`).
    pub fn push_attachment(&mut self, attachment: MailAttachment) {
        self.attachments.push(attachment);
    }

    /// Drops every attachment row (spec
    /// `messaging.allium:DeleteMail`'s `for a in mail.attachments:
    /// not exists a` consequent).
    pub fn clear_attachments(&mut self) {
        self.attachments.clear();
    }

    /// Rewrites the [`subject`](Self::subject) (spec
    /// `messaging.allium:EditMailHeader`'s `mail.subject =
    /// new_subject` consequent).
    pub fn set_subject(&mut self, subject: String) {
        self.subject = subject;
    }

    /// Rewrites the [`to_name`](Self::to_name) and
    /// [`addressee_slot`](Self::addressee_slot) (spec
    /// `messaging.allium:EditMailHeader`'s `mail.to_name =
    /// new_to_name` / `mail.addressee = lookup_user_by_name(...)`
    /// consequents). The caller has already resolved the slot via
    /// the conference's `accepted_name_type`.
    pub fn set_addressee(&mut self, to_name: String, addressee_slot: Option<u32>) {
        self.to_name = to_name;
        self.addressee_slot = addressee_slot;
    }

    /// Returns the message's number within its base.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Returns the current visibility.
    #[must_use]
    pub fn visibility(&self) -> MailVisibility {
        self.visibility
    }

    /// Returns the author's display name as recorded at post time.
    #[must_use]
    pub fn from_name(&self) -> &str {
        &self.from_name
    }

    /// Returns the addressee's typed name (or `"ALL"` / `"EALL"`).
    #[must_use]
    pub fn to_name(&self) -> &str {
        &self.to_name
    }

    /// Returns the broadcast discriminator on `to_name`.
    #[must_use]
    pub fn broadcast_to(&self) -> BroadcastTo {
        self.broadcast_to
    }

    /// Returns the free-text subject.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Returns when the message was posted.
    #[must_use]
    pub fn posted_at(&self) -> SystemTime {
        self.posted_at
    }

    /// Returns when the addressee first read the message, if at all.
    #[must_use]
    pub fn received_at(&self) -> Option<SystemTime> {
        self.received_at
    }

    /// Returns the author's stable slot number.
    #[must_use]
    pub fn author_slot(&self) -> u32 {
        self.author_slot
    }

    /// Returns the addressee's stable slot number, or `None` when the
    /// message is a broadcast (ALL / EALL).
    #[must_use]
    pub fn addressee_slot(&self) -> Option<u32> {
        self.addressee_slot
    }

    /// Returns the message body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Attempts to move `visibility` to `new_visibility`
    /// (spec: `messaging.allium:Mail.transitions visibility`).
    ///
    /// Legal transitions:
    /// - `public -> deleted`, `public -> private`
    /// - `private -> deleted`, `private -> public`
    /// - `private_to_sysop -> deleted`
    ///
    /// `deleted` is terminal — no transitions out are allowed.
    ///
    /// # Errors
    /// Returns [`MailTransitionError::IllegalTransition`] when the
    /// requested move is not in the spec's transition matrix.
    pub fn transition_to(
        &mut self,
        new_visibility: MailVisibility,
    ) -> Result<(), MailTransitionError> {
        use MailVisibility::{Deleted, Private, PrivateToSysop, Public};
        let legal = matches!(
            (self.visibility, new_visibility),
            (Public, Deleted | Private) | (Private, Deleted | Public) | (PrivateToSysop, Deleted)
        );
        if !legal {
            return Err(MailTransitionError::IllegalTransition {
                from: self.visibility,
                to: new_visibility,
            });
        }
        self.visibility = new_visibility;
        // Spec invariant DeletedMessagesHaveNoActiveReceived: a deleted
        // mail has no live received_at. The legacy DeleteMail flow
        // discards messages regardless of read state, so cascade-clear
        // rather than reject the transition.
        if matches!(new_visibility, MailVisibility::Deleted) {
            self.received_at = None;
        }
        Ok(())
    }

    /// Marks the message as received at `when`
    /// (spec: `messaging.allium:ReadMail` `ensures: received_at = now`).
    ///
    /// # Errors
    /// Returns [`MailError::AlreadyDeleted`] when the message has
    /// already been soft-deleted — `ReadMail` rejects deleted mails
    /// upstream via `requires: not mail.is_deleted`, but the entity
    /// guards against the invariant being broken anyway.
    pub fn mark_received(&mut self, when: SystemTime) -> Result<(), MailError> {
        if matches!(self.visibility, MailVisibility::Deleted) {
            return Err(MailError::AlreadyDeleted);
        }
        self.received_at = Some(when);
        Ok(())
    }
}

/// Errors raised by [`Mail`] mutating operations other than visibility
/// transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MailError {
    /// The operation cannot be performed because the message is in
    /// [`MailVisibility::Deleted`]. Spec invariant
    /// `DeletedMessagesHaveNoActiveReceived` forbids writes that would
    /// leave a deleted message with a live `received_at`.
    #[error("mail is already deleted")]
    AlreadyDeleted,
}

/// Errors returned by [`Mail::transition_to`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MailTransitionError {
    /// The requested visibility move is not in the spec's
    /// `transitions visibility` matrix.
    #[error("illegal mail visibility transition: {from:?} -> {to:?}")]
    IllegalTransition {
        /// Current visibility.
        from: MailVisibility,
        /// Requested visibility.
        to: MailVisibility,
    },
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::conference::{AllScanScope, AllowedAddressing};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn sample(number: u32) -> NewMail {
        NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Welcome".to_string(),
            posted_at: t(100),
            author_slot: 1,
            addressee_slot: Some(2),
            body: "Hello, alice!".to_string(),
        }
    }

    #[test]
    fn mail_round_trips_basic_header_fields() {
        // Pick non-default, non-1 values for every numeric field so a
        // constant-folded accessor mutant can't slip past.
        let mail = Mail::new(sample(7));

        assert_eq!(mail.msgbase(), MessageBaseRef::new(2, 1));
        assert_eq!(mail.number(), 7);
        assert_eq!(mail.visibility(), MailVisibility::Public);
        assert_eq!(mail.from_name(), "Sysop");
        assert_eq!(mail.to_name(), "alice");
        assert_eq!(mail.broadcast_to(), BroadcastTo::None);
        assert_eq!(mail.subject(), "Welcome");
        assert_eq!(mail.posted_at(), t(100));
        assert_eq!(mail.author_slot(), 1);
        assert_eq!(mail.addressee_slot(), Some(2));
        assert_eq!(mail.body(), "Hello, alice!");
    }

    #[test]
    fn newly_posted_mail_is_unread() {
        // Spec PostMail: ensures `received_at: null` at post time.
        let mail = Mail::new(sample(1));
        assert_eq!(mail.received_at(), None);
    }

    #[test]
    fn public_mail_can_transition_to_deleted() {
        // Spec messaging.allium:Mail `transitions visibility`:
        //   public -> deleted
        let mut mail = Mail::new(sample(1));
        mail.transition_to(MailVisibility::Deleted)
            .expect("public -> deleted is a legal transition");
        assert_eq!(mail.visibility(), MailVisibility::Deleted);
    }

    #[test]
    fn transition_matrix_matches_spec() {
        // Spec messaging.allium:Mail `transitions visibility`:
        //   public -> deleted
        //   private -> deleted
        //   private_to_sysop -> deleted
        //   public -> private
        //   private -> public
        //   terminal: deleted
        //
        // Every other (from, to) pair must be rejected. Identity moves
        // (e.g. public -> public) aren't listed in the spec and are
        // therefore not allowed either.
        use MailVisibility::{Deleted, Private, PrivateToSysop, Public};
        let all = [Public, Private, PrivateToSysop, Deleted];
        let legal: &[(MailVisibility, MailVisibility)] = &[
            (Public, Deleted),
            (Private, Deleted),
            (PrivateToSysop, Deleted),
            (Public, Private),
            (Private, Public),
        ];
        for &from in &all {
            for &to in &all {
                let is_legal = legal.contains(&(from, to));
                // Reconstruct a fresh `Mail` for each pair so the
                // outcome is independent of test ordering. Set the
                // starting visibility via direct field assignment in
                // tests — production paths always go through
                // `Mail::new` then `transition_to`, so legal `from`
                // states arrive there via earlier legal transitions.
                let mut mail = Mail::new(sample(1));
                mail.visibility = from;
                let result = mail.transition_to(to);
                if is_legal {
                    assert!(
                        result.is_ok(),
                        "{from:?} -> {to:?} should be legal but was rejected: {result:?}",
                    );
                    assert_eq!(
                        mail.visibility(),
                        to,
                        "{from:?} -> {to:?} should land on {to:?}",
                    );
                } else {
                    assert_eq!(
                        result,
                        Err(MailTransitionError::IllegalTransition { from, to }),
                        "{from:?} -> {to:?} should be illegal",
                    );
                    // Rejected transitions must not mutate state.
                    assert_eq!(
                        mail.visibility(),
                        from,
                        "{from:?} -> {to:?} was rejected but visibility changed",
                    );
                }
            }
        }
    }

    #[test]
    fn mark_received_on_deleted_mail_is_rejected() {
        // Defensive: ReadMail's upstream `requires: not mail.is_deleted`
        // makes this unreachable through the rule, but the entity must
        // refuse to break DeletedMessagesHaveNoActiveReceived even when
        // called directly.
        let mut mail = Mail::new(sample(1));
        mail.transition_to(MailVisibility::Deleted).unwrap();
        let err = mail
            .mark_received(t(500))
            .expect_err("deleted mail must not accept a received_at");
        assert_eq!(err, MailError::AlreadyDeleted);
        assert_eq!(mail.received_at(), None);
    }

    #[test]
    fn transitioning_to_deleted_clears_received_at() {
        // Spec messaging.allium invariant DeletedMessagesHaveNoActiveReceived:
        //   visibility = deleted implies received_at = null.
        // A message read by its addressee and then deleted must have
        // its received_at cleared so the invariant holds.
        let mut mail = Mail::new(sample(1));
        mail.mark_received(t(500))
            .expect("addressee may mark an unread, undeleted message as received");
        assert_eq!(mail.received_at(), Some(t(500)));

        mail.transition_to(MailVisibility::Deleted).unwrap();
        assert_eq!(
            mail.received_at(),
            None,
            "received_at must be cleared when message becomes deleted",
        );
    }

    #[test]
    fn addressing_allows_individual_always_permitted() {
        // Spec: a per-user addressee never depends on AllowedAddressing.
        for allowed in [
            AllowedAddressing::IndividualOnly,
            AllowedAddressing::IndividualOrAll,
            AllowedAddressing::IndividualOrEall,
            AllowedAddressing::Any,
        ] {
            assert!(
                addressing_allows(allowed, BroadcastTo::None),
                "individual should be allowed under {allowed:?}",
            );
        }
    }

    #[test]
    fn addressing_allows_all_only_when_variant_permits() {
        // Spec messaging.allium:AllowedAddressing —
        //   IndividualOrAll / Any permit ALL; the other two forbid it.
        assert!(!addressing_allows(
            AllowedAddressing::IndividualOnly,
            BroadcastTo::All
        ));
        assert!(addressing_allows(
            AllowedAddressing::IndividualOrAll,
            BroadcastTo::All
        ));
        assert!(!addressing_allows(
            AllowedAddressing::IndividualOrEall,
            BroadcastTo::All
        ));
        assert!(addressing_allows(AllowedAddressing::Any, BroadcastTo::All));
    }

    #[test]
    fn addressing_allows_eall_only_when_variant_permits() {
        // Spec messaging.allium:AllowedAddressing —
        //   IndividualOrEall / Any permit EALL; the other two forbid it.
        assert!(!addressing_allows(
            AllowedAddressing::IndividualOnly,
            BroadcastTo::Eall
        ));
        assert!(!addressing_allows(
            AllowedAddressing::IndividualOrAll,
            BroadcastTo::Eall
        ));
        assert!(addressing_allows(
            AllowedAddressing::IndividualOrEall,
            BroadcastTo::Eall
        ));
        assert!(addressing_allows(AllowedAddressing::Any, BroadcastTo::Eall));
    }

    #[test]
    fn allowed_addressing_default_permits_everything() {
        // Default matches the legacy `enterMSG` behaviour where bare
        // conferences accepted both broadcasts; sysops narrow it only
        // when bridging to an external system.
        assert_eq!(AllowedAddressing::default(), AllowedAddressing::Any);
        assert!(addressing_allows(
            AllowedAddressing::default(),
            BroadcastTo::All
        ));
        assert!(addressing_allows(
            AllowedAddressing::default(),
            BroadcastTo::Eall
        ));
    }

    #[test]
    fn all_scan_scope_default_is_all_users_in_conf() {
        // The legacy `searchNewMail` always counted ALL toward every
        // member's unread tally — broadcast == "to me" regardless of
        // visit state.
        assert_eq!(AllScanScope::default(), AllScanScope::AllUsersInConf);
    }

    #[test]
    fn deleted_mail_cannot_be_resurrected_to_public() {
        // Spec messaging.allium:Mail `transitions visibility`:
        //   terminal: deleted
        let mut mail = Mail::new(sample(1));
        mail.transition_to(MailVisibility::Deleted).unwrap();
        let err = mail
            .transition_to(MailVisibility::Public)
            .expect_err("deleted is terminal");
        assert_eq!(
            err,
            MailTransitionError::IllegalTransition {
                from: MailVisibility::Deleted,
                to: MailVisibility::Public,
            }
        );
        // visibility unchanged after a rejected transition
        assert_eq!(mail.visibility(), MailVisibility::Deleted);
    }

    #[test]
    fn status_glyph_uses_lowercase_for_sysop_only_mail() {
        // Spec `MailVisibility` (Slice 47): the sysop sees the same
        // letter glyph in lower case for censored mail.
        assert_eq!(MailVisibility::Public.status_glyph(), ' ');
        assert_eq!(MailVisibility::Private.status_glyph(), 'P');
        assert_eq!(MailVisibility::PrivateToSysop.status_glyph(), 'p');
        assert_eq!(MailVisibility::Deleted.status_glyph(), 'D');
    }
}
