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

    /// Extracts this mail's header and body into a [`MailDraft`] —
    /// the faithful inverse of [`Self::from_draft`], carrying the
    /// real `visibility` (spec `messaging.allium:MoveMail`'s
    /// `visibility: mail.visibility` consequent) plus every other
    /// draft field.
    ///
    /// The store-assigned coordinates (`msgbase`, `number`) and the
    /// fields a [`MailDraft`] cannot express (`received_at`,
    /// `attachments`) are not carried — see
    /// [`Self::carry_state_from`] for the latter.
    #[must_use]
    pub fn to_draft(&self) -> MailDraft {
        MailDraft {
            visibility: self.visibility,
            from_name: self.from_name.clone(),
            to_name: self.to_name.clone(),
            broadcast_to: self.broadcast_to,
            subject: self.subject.clone(),
            posted_at: self.posted_at,
            author_slot: self.author_slot,
            addressee_slot: self.addressee_slot,
            body: self.body.clone(),
        }
    }

    /// Copies from `source` the two fields a [`MailDraft`] cannot
    /// express — `received_at` and `attachments` — onto this mail
    /// (spec `messaging.allium:MoveMail`'s `received_at:
    /// mail.received_at` and `for a in mail.attachments:
    /// MailAttachment.created` consequents).
    ///
    /// A `Deleted` source carries `received_at = None` by the
    /// `DeletedMessagesHaveNoActiveReceived` invariant, so the copy
    /// stays legal whatever its own visibility.
    pub fn carry_state_from(&mut self, source: &Mail) {
        self.received_at = source.received_at;
        self.attachments.clone_from(&source.attachments);
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

    /// True when this mail has been soft-deleted (spec
    /// `messaging.allium:Mail`'s derived attribute `is_deleted:
    /// visibility = deleted`).
    ///
    /// This is the owner predicate behind the spec's `requires: not
    /// mail.is_deleted` clauses and the
    /// `DeletedMessagesHaveNoActiveReceived` invariant.
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        matches!(self.visibility, MailVisibility::Deleted)
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
        // rather than reject the transition. This check must follow
        // the visibility assignment above: the mail is now deleted,
        // so cascade-clear received_at.
        if self.is_deleted() {
            self.received_at = None;
        }
        Ok(())
    }

    /// Soft-deletes this mail (spec `messaging.allium:DeleteMail`
    /// consequents, owned by the entity): transitions `visibility` to
    /// [`MailVisibility::Deleted`] and strips every attachment row
    /// (the spec's `for a in mail.attachments: not exists a`).
    /// `received_at` is cleared by the transition's cascade, keeping
    /// the `DeletedMessagesHaveNoActiveReceived` invariant.
    ///
    /// Transition-matrix rationale, recorded once here: every
    /// non-deleted visibility (`public`, `private`,
    /// `private_to_sysop`) may transition to `deleted`; only a
    /// `Deleted` receiver errors.
    ///
    /// # Errors
    /// Returns [`MailError::AlreadyDeleted`] when the mail is already
    /// soft-deleted — `deleted` is terminal in the spec's
    /// `transitions visibility` matrix.
    ///
    /// # Panics
    /// Never in practice: the `AlreadyDeleted` guard filters the only
    /// visibility (`Deleted`) the transition matrix rejects from
    /// moving to `deleted`, so the internal transition cannot fail.
    pub fn soft_delete(&mut self) -> Result<(), MailError> {
        if self.is_deleted() {
            return Err(MailError::AlreadyDeleted);
        }
        self.transition_to(MailVisibility::Deleted)
            .expect("every non-deleted visibility may transition to deleted");
        self.clear_attachments();
        Ok(())
    }

    /// True when this mail is unread (`received_at = None`, the spec's
    /// `is_unread` derived attribute) *and* addressed to `reader_slot`
    /// — the conjunction `mail.is_unread and mail.addressee =
    /// session.user` shared by `messaging.allium:ReadMail`'s ensures
    /// clause and the mail scan's unread test.
    ///
    /// A soft-deleted mail has `received_at = None` (the
    /// `DeletedMessagesHaveNoActiveReceived` invariant), so it
    /// vacuously counts as unread here — callers that must exclude
    /// deleted mail gate on [`Self::is_deleted`] first, as
    /// [`Self::record_read_by`] does.
    #[must_use]
    pub fn is_unread_addressed_to(&self, reader_slot: u32) -> bool {
        self.received_at.is_none() && self.addressee_slot == Some(reader_slot)
    }

    /// Records that the reader at `reader_slot` read this mail at `now`
    /// (spec `messaging.allium:ReadMail`'s `ensures: if mail.is_unread
    /// and mail.addressee = session.user: received_at = now`).
    ///
    /// Infallible: no-ops unless all of the following hold —
    /// - the mail is not soft-deleted (preserves
    ///   `DeletedMessagesHaveNoActiveReceived`; a deleted mail has
    ///   `received_at = None` and would otherwise pass the unread
    ///   check);
    /// - the mail is unread (first-read wins — a second read never
    ///   overwrites the original timestamp);
    /// - the mail is addressed to `reader_slot` (broadcasts have no
    ///   addressee and are never marked).
    pub fn record_read_by(&mut self, reader_slot: u32, now: SystemTime) {
        if self.is_deleted() || !self.is_unread_addressed_to(reader_slot) {
            return;
        }
        self.received_at = Some(now);
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
        if self.is_deleted() {
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

    use super::*;
    use crate::domain::conference::{AllScanScope, AllowedAddressing};
    use crate::domain::messaging::mail_store::test_support::t;

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
    fn non_deleted_transition_preserves_received_at() {
        // The DeletedMessagesHaveNoActiveReceived cascade in
        // `transition_to` fires only on arrival at `deleted` — a
        // received mail moving public -> private keeps its read
        // timestamp.
        let mut mail = Mail::new(sample(1));
        mail.mark_received(t(200)).expect("public mail is markable");
        mail.transition_to(MailVisibility::Private)
            .expect("public -> private is a legal transition");
        assert_eq!(mail.received_at(), Some(t(200)));
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
    fn is_deleted_reflects_visibility() {
        // Spec messaging.allium:Mail derived attribute:
        //   is_deleted: visibility = deleted
        // Only the Deleted variant satisfies the predicate.
        use MailVisibility::{Deleted, Private, PrivateToSysop, Public};
        for (visibility, expected) in [
            (Public, false),
            (Private, false),
            (PrivateToSysop, false),
            (Deleted, true),
        ] {
            // Direct field assignment in tests, as in
            // `transition_matrix_matches_spec` — production paths
            // arrive at each state via legal transitions.
            let mut mail = Mail::new(sample(1));
            mail.visibility = visibility;
            assert_eq!(
                mail.is_deleted(),
                expected,
                "is_deleted() for {visibility:?}",
            );
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
    fn soft_delete_deletes_strips_attachments_and_clears_received_at() {
        // Spec messaging.allium:DeleteMail consequents, owned by the
        // entity: visibility = deleted, `for a in mail.attachments:
        // not exists a`, and (via the transition cascade)
        // DeletedMessagesHaveNoActiveReceived.
        let mut mail = Mail::new(sample(1));
        mail.mark_received(t(500))
            .expect("undeleted mail accepts received_at");
        mail.push_attachment(MailAttachment::new(
            "a.txt".to_string(),
            crate::domain::bytes::Bytes::new(3),
        ));

        mail.soft_delete().expect("public mail can be soft-deleted");

        assert_eq!(mail.visibility(), MailVisibility::Deleted);
        assert!(mail.attachments().is_empty());
        assert_eq!(mail.received_at(), None);
    }

    #[test]
    fn soft_delete_on_deleted_mail_returns_already_deleted() {
        // Spec DeleteMail `requires: not mail.is_deleted` — deleted is
        // terminal, so a second soft-delete is rejected by the owner.
        let mut mail = Mail::new(sample(1));
        mail.soft_delete().expect("first soft-delete succeeds");
        let err = mail
            .soft_delete()
            .expect_err("deleted mail must reject a second soft-delete");
        assert_eq!(err, MailError::AlreadyDeleted);
    }

    #[test]
    fn soft_delete_works_from_every_non_deleted_visibility() {
        // Transition-matrix rationale, pinned once at the owner: every
        // non-deleted visibility may transition to deleted.
        use MailVisibility::{Private, PrivateToSysop, Public};
        for visibility in [Public, Private, PrivateToSysop] {
            let mut mail = Mail::new(sample(1));
            mail.visibility = visibility;
            mail.soft_delete()
                .unwrap_or_else(|err| panic!("{visibility:?} soft-delete failed: {err:?}"));
            assert_eq!(mail.visibility(), MailVisibility::Deleted);
        }
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
    fn to_draft_is_the_faithful_inverse_of_from_draft() {
        // MoveMail consequent `visibility: mail.visibility` — the
        // draft must carry the real visibility. A PrivateToSysop mail
        // cannot be rebuilt via a Public draft plus a transition,
        // since Public -> PrivateToSysop is outside the matrix.
        let mut mail = Mail::new(sample(7));
        mail.visibility = MailVisibility::PrivateToSysop;

        let draft = mail.to_draft();

        assert_eq!(draft.visibility, MailVisibility::PrivateToSysop);
        assert_eq!(draft.from_name, "Sysop");
        assert_eq!(draft.to_name, "alice");
        assert_eq!(draft.broadcast_to, BroadcastTo::None);
        assert_eq!(draft.subject, "Welcome");
        assert_eq!(draft.posted_at, t(100));
        assert_eq!(draft.author_slot, 1);
        assert_eq!(draft.addressee_slot, Some(2));
        assert_eq!(draft.body, "Hello, alice!");
        // Round trip: rebuilding from the draft reproduces the mail
        // (received_at / attachments are not draft-expressible and
        // are None / empty on both sides here).
        assert_eq!(Mail::from_draft(mail.msgbase(), mail.number(), draft), mail);
    }

    #[test]
    fn carry_state_from_copies_received_at_and_attachments() {
        // MoveMail consequents `received_at: mail.received_at` and
        // `for a in mail.attachments: MailAttachment.created` — the
        // two fields a MailDraft cannot express.
        let mut source = Mail::new(sample(1));
        source
            .mark_received(t(500))
            .expect("undeleted mail accepts received_at");
        source.push_attachment(MailAttachment::new("a.txt".to_string(), Bytes::new(3)));

        let mut copy = Mail::new(sample(2));
        copy.carry_state_from(&source);

        assert_eq!(copy.received_at(), Some(t(500)));
        assert_eq!(copy.attachments(), source.attachments());
    }

    #[test]
    fn carry_state_from_a_deleted_source_keeps_received_at_none() {
        // A Deleted source has received_at = None by
        // DeletedMessagesHaveNoActiveReceived, so the carried copy
        // stays legal (and unread) — and a previously-set timestamp
        // on the copy is overwritten, not merged.
        let mut source = Mail::new(sample(1));
        source.mark_received(t(500)).unwrap();
        source.soft_delete().expect("public mail soft-deletes");

        let mut copy = Mail::new(sample(2));
        copy.mark_received(t(900)).unwrap();
        copy.carry_state_from(&source);

        assert_eq!(copy.received_at(), None);
        assert!(copy.attachments().is_empty());
    }

    #[test]
    fn record_read_by_sets_received_at_for_unread_addressee() {
        // Spec messaging.allium:ReadMail ensures: if mail.is_unread
        // and mail.addressee = session.user: received_at = now.
        // sample() addresses slot 2.
        let mut mail = Mail::new(sample(1));
        mail.record_read_by(2, t(100));
        assert_eq!(mail.received_at(), Some(t(100)));
    }

    #[test]
    fn record_read_by_keeps_the_first_read_timestamp() {
        // First-read wins: a second read must not overwrite the
        // original received_at.
        let mut mail = Mail::new(sample(1));
        mail.record_read_by(2, t(100));
        mail.record_read_by(2, t(200));
        assert_eq!(mail.received_at(), Some(t(100)));
    }

    #[test]
    fn record_read_by_ignores_a_reader_who_is_not_the_addressee() {
        // The `mail.addressee = session.user` clause fails for any
        // other slot — including broadcasts (addressee_slot = None).
        let mut mail = Mail::new(sample(1));
        mail.record_read_by(9, t(100));
        assert_eq!(mail.received_at(), None);

        let mut broadcast = Mail::new(NewMail {
            broadcast_to: BroadcastTo::All,
            to_name: "ALL".to_string(),
            addressee_slot: None,
            ..sample(1)
        });
        broadcast.record_read_by(2, t(100));
        assert_eq!(broadcast.received_at(), None);
    }

    #[test]
    fn record_read_by_on_deleted_mail_leaves_received_at_none() {
        // Defensive: ReadMail's `requires: not mail.is_deleted` makes
        // this unreachable through the rule, but the entity must not
        // break DeletedMessagesHaveNoActiveReceived when called
        // directly — a deleted mail has received_at = None and would
        // otherwise satisfy the unread-addressee predicate.
        let mut mail = Mail::new(sample(1));
        mail.transition_to(MailVisibility::Deleted).unwrap();
        mail.record_read_by(2, t(100));
        assert_eq!(mail.received_at(), None);
    }

    #[test]
    fn is_unread_addressed_to_requires_unread_and_matching_slot() {
        // Spec conjunction `mail.is_unread and mail.addressee =
        // session.user`, shared by ReadMail's ensure and the scan's
        // unread test. sample() addresses slot 2.
        let mut mail = Mail::new(sample(1));
        assert!(mail.is_unread_addressed_to(2));
        assert!(!mail.is_unread_addressed_to(9), "wrong slot");
        mail.mark_received(t(100)).unwrap();
        assert!(!mail.is_unread_addressed_to(2), "already read");
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
