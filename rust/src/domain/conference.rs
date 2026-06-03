//! [`Conference`] and [`MessageBase`] entities (spec:
//! `core.allium:Conference`, `core.allium:MessageBase`).
//!
//! Phase 4 introduces conferences as data; the join workflow,
//! membership checks and bulletins land in later slices in this phase.
//! Following the schema-growth principle from
//! [`SLICES.md`](../../../SLICES.md), only the fields Phase 4 actually
//! reads or writes are present. `accepted_name_type` arrives with
//! Slice 34 (`JoinedConferenceForNameType`), per-conference accounting
//! with Slice 61, and message high-water marks with Slice 37 (mail
//! storage). [`ConferenceMembership`]'s `pointers` collection arrives
//! with Slice 38 (`ReadPointers`).

use crate::domain::messaging::read_pointers::ReadPointers;

/// Which kinds of `to:` addresses a message base accepts when posting
/// (spec: `messaging.allium:AllowedAddressing`). The legacy default
/// permits both ALL and EALL alongside individual addressees; external
/// bridges narrow this when they cannot fan out broadcasts.
///
/// Lives in `conference` rather than `mail` because it is per-msgbase
/// configuration carried on [`MessageBase`], not a property of a
/// [`crate::domain::messaging::mail::Mail`] in flight; keeping it here also breaks
/// the import cycle between `conference` and `mail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AllowedAddressing {
    /// `IndividualOnly` — the poster must address a specific user.
    IndividualOnly,
    /// `IndividualOrAll` — ALL is permitted; EALL is not.
    IndividualOrAll,
    /// `IndividualOrEall` — EALL is permitted; ALL is not.
    IndividualOrEall,
    /// `Any` — individual, ALL and EALL are all permitted (default).
    #[default]
    Any,
}

/// Whether the conference shows ALL-addressed messages to every member
/// or only to users currently visiting (spec:
/// `messaging.allium:AllScanScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AllScanScope {
    /// `Local` — ALL counts as "to me" only for the user currently
    /// visiting this message base's conference.
    Local,
    /// `AllUsersInConf` — ALL is broadcast to every member of the
    /// conference, regardless of which one they're currently in
    /// (default; matches the legacy `searchNewMail` behaviour).
    #[default]
    AllUsersInConf,
}

/// How the user's display name is rendered when reading or posting
/// messages in a given conference (spec: `core.allium:NameType`).
///
/// Real-name and internet-name conferences flip the session's
/// `display_name_type` on join (Slice 34) so subsequent message
/// rendering uses the right identity. The default `Handle` matches
/// the BBS-wide username.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NameType {
    /// The BBS username (default).
    #[default]
    Handle,
    /// Legal name, used in real-name conferences.
    RealName,
    /// Internet-style identity, used for gateway conferences.
    InternetName,
}

/// Coordinates of a particular message base within a particular
/// conference (spec: `core.allium:MessageBaseRef`).
///
/// Both fields are 1-indexed entity numbers, mirroring how the BBS
/// surfaces them to users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageBaseRef {
    conference_number: u32,
    msgbase_number: u32,
}

impl MessageBaseRef {
    /// Constructs a [`MessageBaseRef`] from raw conference and message
    /// base numbers. Prefer [`MessageBase::msgbase_ref`] when you have
    /// a [`MessageBase`] in hand — that helper threads the back-ref
    /// the spec maintains.
    #[must_use]
    pub fn new(conference_number: u32, msgbase_number: u32) -> Self {
        Self {
            conference_number,
            msgbase_number,
        }
    }

    /// Returns the conference's 1-indexed number.
    #[must_use]
    pub fn conference_number(&self) -> u32 {
        self.conference_number
    }

    /// Returns the message base's 1-indexed number within its
    /// conference.
    #[must_use]
    pub fn msgbase_number(&self) -> u32 {
        self.msgbase_number
    }
}

/// A sub-forum within a [`Conference`] (spec: `core.allium:MessageBase`).
///
/// Each base is identified by its conference-scoped 1-indexed
/// `number`. The on-disk message store and the read-pointer mechanics
/// are introduced in Phase 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBase {
    conference_number: u32,
    number: u32,
    name: String,
    allowed_addressing: AllowedAddressing,
    all_scan_scope: AllScanScope,
}

impl MessageBase {
    /// Constructs a new [`MessageBase`] with default broadcast policy
    /// (any addressing accepted, ALL scoped to every member).
    ///
    /// # Parameters
    /// - `conference_number`: the parent conference's 1-indexed
    ///   number. The base records this so [`Self::msgbase_ref`] can
    ///   produce a [`MessageBaseRef`] without a separate
    ///   [`Conference`] handle, mirroring the spec's
    ///   `msgbase_ref_for(msgbase)` helper which takes a single
    ///   argument.
    /// - `number`: 1-indexed within the parent conference.
    /// - `name`: human-readable label.
    #[must_use]
    pub fn new(conference_number: u32, number: u32, name: String) -> Self {
        Self::with_options(
            conference_number,
            number,
            name,
            AllowedAddressing::default(),
            AllScanScope::default(),
        )
    }

    /// Constructs a new [`MessageBase`] with explicit
    /// [`AllowedAddressing`] and [`AllScanScope`] policies (Slice 43).
    /// Used by the conference loader when a sysop narrows the base's
    /// broadcast policy.
    #[must_use]
    pub fn with_options(
        conference_number: u32,
        number: u32,
        name: String,
        allowed_addressing: AllowedAddressing,
        all_scan_scope: AllScanScope,
    ) -> Self {
        Self {
            conference_number,
            number,
            name,
            allowed_addressing,
            all_scan_scope,
        }
    }

    /// Returns the parent conference's 1-indexed number.
    #[must_use]
    pub fn conference_number(&self) -> u32 {
        self.conference_number
    }

    /// Returns this message base's 1-indexed number.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Returns this message base's human-readable name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the kinds of `to:` addresses this base accepts at post
    /// time (spec: `messaging.allium:MessageBase.allowed_addressing`).
    #[must_use]
    pub fn allowed_addressing(&self) -> AllowedAddressing {
        self.allowed_addressing
    }

    /// Returns whether ALL-addressed mail is scoped to the current
    /// visit only, or to every member (spec:
    /// `messaging.allium:MessageBase.all_scan_scope`).
    #[must_use]
    pub fn all_scan_scope(&self) -> AllScanScope {
        self.all_scan_scope
    }

    /// Returns a [`MessageBaseRef`] coordinate pair pointing at
    /// `self`. This is the spec's `msgbase_ref_for(msgbase)` helper.
    #[must_use]
    pub fn msgbase_ref(&self) -> MessageBaseRef {
        MessageBaseRef::new(self.conference_number, self.number)
    }
}

/// A discussion area on the BBS (spec: `core.allium:Conference`).
///
/// Phase 4 carries only the fields it needs: the 1-indexed
/// `number`, the human-readable `name`, and the conference's
/// non-empty collection of [`MessageBase`]s. Construction enforces
/// the spec's `AtLeastOneMessageBase` invariant and rejects any
/// message base whose `conference_number` disagrees with the
/// conference being built — that's how the entity guarantees the
/// downstream `VisitedMsgBaseBelongsToVisitedConference` invariant
/// (Phase 4, Slice 30) holds without a runtime cross-check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conference {
    number: u32,
    name: String,
    msgbases: Vec<MessageBase>,
    accepted_name_type: NameType,
}

impl Conference {
    /// Constructs a new [`Conference`] with the supplied
    /// [`MessageBase`]s and the default ([`NameType::Handle`])
    /// accepted name type.
    ///
    /// # Parameters
    /// - `number`: 1-indexed conference number, displayed to users.
    /// - `name`: human-readable label (e.g. `"Programming"`).
    /// - `msgbases`: the conference's message bases.
    ///
    /// # Errors
    /// Returns [`ConferenceError::NoMessageBases`] when `msgbases` is
    /// empty (spec invariant `AtLeastOneMessageBase`).
    /// Returns [`ConferenceError::MsgbaseConferenceMismatch`] when
    /// any element of `msgbases` carries a `conference_number` that
    /// disagrees with `number`; the offending base's number is
    /// reported on the error so callers can locate the mismatch.
    pub fn new(
        number: u32,
        name: String,
        msgbases: Vec<MessageBase>,
    ) -> Result<Self, ConferenceError> {
        Self::with_name_type(number, name, msgbases, NameType::Handle)
    }

    /// Constructs a new [`Conference`] with an explicit
    /// `accepted_name_type` (Slice 34). Real-name and internet-name
    /// conferences override the default [`NameType::Handle`].
    ///
    /// # Errors
    /// Same as [`Self::new`].
    pub fn with_name_type(
        number: u32,
        name: String,
        msgbases: Vec<MessageBase>,
        accepted_name_type: NameType,
    ) -> Result<Self, ConferenceError> {
        if msgbases.is_empty() {
            return Err(ConferenceError::NoMessageBases);
        }
        if let Some(bad) = msgbases.iter().find(|m| m.conference_number != number) {
            return Err(ConferenceError::MsgbaseConferenceMismatch {
                conference_number: number,
                msgbase_conference_number: bad.conference_number,
                msgbase_number: bad.number,
            });
        }
        Ok(Self {
            number,
            name,
            msgbases,
            accepted_name_type,
        })
    }

    /// Returns this conference's 1-indexed number.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Returns this conference's human-readable name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the conference's message bases in declared order.
    #[must_use]
    pub fn msgbases(&self) -> &[MessageBase] {
        &self.msgbases
    }

    /// Returns the [`MessageBase`] with the supplied 1-indexed number,
    /// or `None` when this conference has no base with that number.
    /// Shared by every callsite that previously walked
    /// `c.msgbases().iter().find(|m| m.number() == ...)`.
    #[must_use]
    pub fn find_msgbase(&self, msgbase_number: u32) -> Option<&MessageBase> {
        self.msgbases.iter().find(|m| m.number() == msgbase_number)
    }

    /// Returns the [`NameType`] this conference expects for posters'
    /// display names (spec: `core.allium:Conference.accepted_name_type`).
    #[must_use]
    pub fn accepted_name_type(&self) -> NameType {
        self.accepted_name_type
    }
}

/// Per-(user, conference) access record (spec:
/// `core.allium:ConferenceMembership`).
///
/// Phase 4 only consumes the `granted` flag; per-conference
/// accounting (bytes, file counts, ratios) and the `messages_posted`
/// counter arrive in the slices that introduce the rules reading
/// them. Slice 38 adds the per-`MessageBase` [`ReadPointers`]
/// collection.
///
/// The membership is identified by its `conference_number` rather
/// than by an owned [`Conference`] reference; the conference
/// catalogue is loaded once at startup and indexed by number, so
/// duplicating the entity here would just invite drift.
///
/// The per-conference scan preferences (`mail_scan`, `mailscan_all`,
/// `file_scan`, `zoom_scan`) are the M/A/F/Z columns the legacy `CF`
/// command edits (`amiexpress/express.e:24672`); see [`ScanFlag`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConferenceMembership {
    conference_number: u32,
    granted: bool,
    pointers: Vec<ReadPointers>,
    messages_posted: u32,
    mail_scan: bool,
    mailscan_all: bool,
    file_scan: bool,
    zoom_scan: bool,
}

/// A per-conference scan preference flag — the M/A/F/Z columns of the
/// legacy `CF` command (`internalCommandCF`, `amiexpress/express.e:24672`).
///
/// Each variant notes the legacy bitmask value it corresponds to
/// (`amiexpress/axconsts.e:45-48`), kept for parity reference; the Rust
/// model stores one boolean per flag rather than packing a bitfield.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScanFlag {
    /// `M` — include the conference in the new-mail scan
    /// (`MAIL_SCAN_MASK` = 4).
    MailScan,
    /// `A` — scan all messages, not just those addressed to the caller
    /// (`MAILSCAN_ALL` = 128).
    MailScanAll,
    /// `F` — include the conference in the new-files scan
    /// (`FILE_SCAN_MASK` = 8).
    FileScan,
    /// `Z` — include the conference in the ZOOM/QWK gather
    /// (`ZOOM_SCAN_MASK` = 2).
    Zoom,
}

impl ConferenceMembership {
    /// Constructs a new membership row.
    ///
    /// Sysop grants and revokes are modelled as toggling the
    /// `granted` flag rather than adding and removing rows: the
    /// legacy spec retains revoked rows so per-conference history
    /// (counters, ratios) survives a re-grant.
    ///
    /// New memberships start with no read-pointer rows; per the
    /// schema-growth principle, a [`ReadPointers`] row is created
    /// lazily by [`Self::upsert_pointers`] the first time a
    /// `ReadMail`, `ScanMail`, or `ScanMailOnJoin` rule touches a
    /// particular [`MessageBase`].
    #[must_use]
    pub fn new(conference_number: u32, granted: bool) -> Self {
        Self {
            conference_number,
            granted,
            pointers: Vec::new(),
            messages_posted: 0,
            // Design D2: mail/file scans default on so a fresh membership
            // is swept out of the box; "all messages" and ZOOM are opt-in.
            mail_scan: true,
            mailscan_all: false,
            file_scan: true,
            zoom_scan: false,
        }
    }

    /// Returns whether the per-conference [`ScanFlag`] is set (spec:
    /// `core.allium:ConferenceMembership.mail_scan` and siblings).
    #[must_use]
    pub fn scan_flag(&self, flag: ScanFlag) -> bool {
        match flag {
            ScanFlag::MailScan => self.mail_scan,
            ScanFlag::MailScanAll => self.mailscan_all,
            ScanFlag::FileScan => self.file_scan,
            ScanFlag::Zoom => self.zoom_scan,
        }
    }

    /// Sets the per-conference [`ScanFlag`] to `on`. Used by the `CF`
    /// editor (`conferences.allium:EditConferenceScanFlags`).
    pub fn set_scan_flag(&mut self, flag: ScanFlag, on: bool) {
        match flag {
            ScanFlag::MailScan => self.mail_scan = on,
            ScanFlag::MailScanAll => self.mailscan_all = on,
            ScanFlag::FileScan => self.file_scan = on,
            ScanFlag::Zoom => self.zoom_scan = on,
        }
    }

    /// Flips the per-conference [`ScanFlag`] (the `*` / conference-list
    /// toggle of the `CF` editor).
    pub fn toggle_scan_flag(&mut self, flag: ScanFlag) {
        self.set_scan_flag(flag, !self.scan_flag(flag));
    }

    /// Returns the running count of messages this user has posted to
    /// the conference (spec:
    /// `core.allium:ConferenceMembership.messages_posted`).
    #[must_use]
    pub fn messages_posted(&self) -> u32 {
        self.messages_posted
    }

    /// Increments [`Self::messages_posted`] by one. Used by
    /// `messaging.allium:PostMail` (Slice 42).
    pub fn bump_messages_posted(&mut self) {
        self.messages_posted = self.messages_posted.saturating_add(1);
    }

    /// Returns the 1-indexed conference number this membership
    /// applies to.
    #[must_use]
    pub fn conference_number(&self) -> u32 {
        self.conference_number
    }

    /// Returns whether the user currently has access to the
    /// conference. A `false` row indicates a previously-granted
    /// membership that the sysop has since revoked.
    #[must_use]
    pub fn is_granted(&self) -> bool {
        self.granted
    }

    /// Sets [`Self::is_granted`]. Used by
    /// `conferences.allium:SysopGrantsConferenceAccess` and
    /// `SysopRevokesConferenceAccess`.
    pub fn set_granted(&mut self, granted: bool) {
        self.granted = granted;
    }

    /// Returns every [`ReadPointers`] row attached to this
    /// membership, in insertion order (spec:
    /// `core.allium:ConferenceMembership.pointers`).
    #[must_use]
    pub fn pointers(&self) -> &[ReadPointers] {
        &self.pointers
    }

    /// Returns the [`ReadPointers`] row for `msgbase_number`, if any.
    #[must_use]
    pub fn pointers_for(&self, msgbase_number: u32) -> Option<&ReadPointers> {
        self.pointers
            .iter()
            .find(|p| p.msgbase_number() == msgbase_number)
    }

    /// Returns a mutable reference to the [`ReadPointers`] row for
    /// `msgbase_number`, if any.
    pub fn pointers_for_mut(&mut self, msgbase_number: u32) -> Option<&mut ReadPointers> {
        self.pointers
            .iter_mut()
            .find(|p| p.msgbase_number() == msgbase_number)
    }

    /// Replaces the [`ReadPointers`] row for `pointers.msgbase_number()`
    /// or appends a new one. Mirrors the lazy-create behaviour expected
    /// by `messaging.allium:ReadMail`, `ScanMail`, and `ScanMailOnJoin`:
    /// callers do not need to pre-seed pointers when the user has never
    /// touched a base before.
    pub fn upsert_pointers(&mut self, pointers: ReadPointers) {
        if let Some(existing) = self.pointers_for_mut(pointers.msgbase_number()) {
            *existing = pointers;
        } else {
            self.pointers.push(pointers);
        }
    }
}

/// Returns the lowest-numbered [`Conference`] in `conferences` for
/// which `memberships` carries a granted row, or `None` when the
/// user has no granted access at all (spec:
/// `conferences.allium:first_accessible_conference(user)`).
///
/// `conferences` is expected to be sorted in ascending number order
/// — that is the contract
/// [`crate::domain::conference_repository::ConferenceRepository::load_all`]
/// already enforces.
#[must_use]
pub fn first_accessible_conference<'a>(
    memberships: &[ConferenceMembership],
    conferences: &'a [Conference],
) -> Option<&'a Conference> {
    conferences
        .iter()
        .find(|conf| has_membership(memberships, conf))
}

/// Returns `true` when `memberships` contains a granted row for
/// `conference` (spec:
/// `conferences.allium:has_membership(user, conference)`).
#[must_use]
pub fn has_membership(memberships: &[ConferenceMembership], conference: &Conference) -> bool {
    memberships
        .iter()
        .any(|m| m.conference_number == conference.number && m.granted)
}

/// Returns the [`MessageBase`] in `catalogue` matching `coord`, or
/// `None` when the coordinate is not registered. Shared by every
/// callsite that previously composed `c.iter().find(...)` and
/// `c.msgbases().iter().find(...)` to resolve per-msgbase policy
/// (e.g. `AllowedAddressing`, `AllScanScope`).
#[must_use]
pub fn find_msgbase_in(catalogue: &[Conference], coord: MessageBaseRef) -> Option<&MessageBase> {
    catalogue
        .iter()
        .find(|c| c.number() == coord.conference_number())
        .and_then(|c| c.find_msgbase(coord.msgbase_number()))
}

/// Errors returned by [`Conference::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConferenceError {
    /// The conference would have no message bases. Violates the
    /// spec's `AtLeastOneMessageBase` invariant.
    #[error("conference must contain at least one message base")]
    NoMessageBases,
    /// A supplied [`MessageBase`] carries a `conference_number` that
    /// disagrees with the conference being constructed.
    #[error(
        "message base {msgbase_number} claims conference \
         {msgbase_conference_number} but is being added to conference \
         {conference_number}"
    )]
    MsgbaseConferenceMismatch {
        /// Number of the conference under construction.
        conference_number: u32,
        /// Conference number recorded on the offending message base.
        msgbase_conference_number: u32,
        /// Number of the offending message base.
        msgbase_number: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conference_requires_at_least_one_message_base() {
        let err = Conference::new(1, "Programming".to_string(), vec![])
            .expect_err("empty msgbase list violates AtLeastOneMessageBase");
        assert_eq!(err, ConferenceError::NoMessageBases);
    }

    #[test]
    fn conference_with_one_message_base_is_valid() {
        let msgbase = MessageBase::new(1, 1, "main".to_string());
        let conf = Conference::new(1, "Programming".to_string(), vec![msgbase])
            .expect("non-empty msgbase list");
        assert_eq!(conf.number(), 1);
        assert_eq!(conf.name(), "Programming");
        assert_eq!(conf.msgbases().len(), 1);
        assert_eq!(conf.msgbases()[0].name(), "main");
    }

    #[test]
    fn conference_preserves_msgbase_order() {
        let bases = vec![
            MessageBase::new(2, 1, "main".to_string()),
            MessageBase::new(2, 2, "tech".to_string()),
            MessageBase::new(2, 3, "off-topic".to_string()),
        ];
        let conf = Conference::new(2, "Two".to_string(), bases).expect("valid");
        // Pin the conference's own number too; otherwise a constant-folded
        // accessor (always returning 1) would slip past these tests.
        assert_eq!(conf.number(), 2);
        let names: Vec<&str> = conf.msgbases().iter().map(MessageBase::name).collect();
        assert_eq!(names, vec!["main", "tech", "off-topic"]);
    }

    #[test]
    fn conference_rejects_msgbases_belonging_to_a_different_conference() {
        let bad = MessageBase::new(9, 1, "wrong-conf".to_string());
        let err = Conference::new(1, "Programming".to_string(), vec![bad])
            .expect_err("conference_number mismatch must be rejected");
        assert_eq!(
            err,
            ConferenceError::MsgbaseConferenceMismatch {
                conference_number: 1,
                msgbase_conference_number: 9,
                msgbase_number: 1,
            }
        );
    }

    #[test]
    fn conference_rejects_when_only_some_msgbases_mismatch() {
        let bases = vec![
            MessageBase::new(1, 1, "main".to_string()),
            MessageBase::new(99, 2, "stray".to_string()),
        ];
        let err = Conference::new(1, "Programming".to_string(), bases).expect_err("mismatch");
        assert_eq!(
            err,
            ConferenceError::MsgbaseConferenceMismatch {
                conference_number: 1,
                msgbase_conference_number: 99,
                msgbase_number: 2,
            }
        );
    }

    #[test]
    fn message_base_accessors_round_trip() {
        let base = MessageBase::new(4, 2, "tech".to_string());
        assert_eq!(base.conference_number(), 4);
        assert_eq!(base.number(), 2);
        assert_eq!(base.name(), "tech");
    }

    #[test]
    fn message_base_defaults_to_permissive_addressing_and_broadcast_scope() {
        // Spec messaging.allium: a freshly-constructed message base
        // accepts any addressing and broadcasts ALL to every member of
        // the conference. The defaults preserve legacy AmiExpress
        // behaviour where conferences with no `EXT-OUT` bridge permit
        // both ALL and EALL.
        let base = MessageBase::new(4, 2, "tech".to_string());
        assert_eq!(base.allowed_addressing(), AllowedAddressing::Any);
        assert_eq!(base.all_scan_scope(), AllScanScope::AllUsersInConf);
    }

    #[test]
    fn message_base_with_overrides_preserves_addressing_and_scope() {
        // Sysops with an external bridge narrow `allowed_addressing` to
        // `IndividualOrAll` (no EALL fan-out) — the entity must round
        // trip the supplied values.
        let base = MessageBase::with_options(
            4,
            2,
            "tech".to_string(),
            AllowedAddressing::IndividualOrAll,
            AllScanScope::Local,
        );
        assert_eq!(
            base.allowed_addressing(),
            AllowedAddressing::IndividualOrAll
        );
        assert_eq!(base.all_scan_scope(), AllScanScope::Local);
    }

    #[test]
    fn msgbase_ref_pairs_conference_and_msgbase_numbers() {
        let base = MessageBase::new(7, 3, "tech".to_string());
        let r = base.msgbase_ref();
        assert_eq!(r.conference_number(), 7);
        assert_eq!(r.msgbase_number(), 3);
    }

    #[test]
    fn message_base_ref_new_round_trips() {
        let r = MessageBaseRef::new(11, 4);
        assert_eq!(r.conference_number(), 11);
        assert_eq!(r.msgbase_number(), 4);
    }

    #[test]
    fn message_base_ref_equality_is_structural() {
        assert_eq!(MessageBaseRef::new(1, 2), MessageBaseRef::new(1, 2));
        assert_ne!(MessageBaseRef::new(1, 2), MessageBaseRef::new(1, 3));
        assert_ne!(MessageBaseRef::new(1, 2), MessageBaseRef::new(2, 2));
    }

    fn make_conf(number: u32) -> Conference {
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    #[test]
    fn membership_round_trips_conference_number_and_granted_flag() {
        let m = ConferenceMembership::new(7, true);
        assert_eq!(m.conference_number(), 7);
        assert!(m.is_granted());

        let revoked = ConferenceMembership::new(7, false);
        assert!(!revoked.is_granted());
    }

    #[test]
    fn set_granted_toggles_the_flag() {
        let mut m = ConferenceMembership::new(3, true);
        m.set_granted(false);
        assert!(!m.is_granted());
        m.set_granted(true);
        assert!(m.is_granted());
    }

    #[test]
    fn new_membership_defaults_mail_and_file_scan_on() {
        // C5 / design D2: a fresh membership is swept for new mail and new
        // files out of the box; the "all messages" and ZOOM masks are
        // opt-in (`core.allium:ConferenceMembership` inline defaults).
        let m = ConferenceMembership::new(2, true);
        assert!(m.scan_flag(ScanFlag::MailScan), "mail_scan defaults on");
        assert!(m.scan_flag(ScanFlag::FileScan), "file_scan defaults on");
        assert!(
            !m.scan_flag(ScanFlag::MailScanAll),
            "mailscan_all defaults off"
        );
        assert!(!m.scan_flag(ScanFlag::Zoom), "zoom_scan defaults off");
    }

    #[test]
    fn set_and_toggle_scan_flag_round_trip_for_every_flag() {
        let mut m = ConferenceMembership::new(2, true);
        for flag in [
            ScanFlag::MailScan,
            ScanFlag::MailScanAll,
            ScanFlag::FileScan,
            ScanFlag::Zoom,
        ] {
            let before = m.scan_flag(flag);
            m.toggle_scan_flag(flag);
            assert_eq!(m.scan_flag(flag), !before, "{flag:?} toggles");
            m.set_scan_flag(flag, true);
            assert!(m.scan_flag(flag), "{flag:?} set on");
            m.set_scan_flag(flag, false);
            assert!(!m.scan_flag(flag), "{flag:?} set off");
        }
    }

    #[test]
    fn has_membership_returns_true_when_a_granted_row_exists() {
        let memberships = vec![ConferenceMembership::new(1, true)];
        assert!(has_membership(&memberships, &make_conf(1)));
    }

    #[test]
    fn has_membership_returns_false_when_no_row_exists() {
        let memberships: Vec<ConferenceMembership> = vec![];
        assert!(!has_membership(&memberships, &make_conf(1)));
    }

    #[test]
    fn has_membership_returns_false_when_only_a_revoked_row_exists() {
        // Revoked rows are kept for history but must not grant access.
        let memberships = vec![ConferenceMembership::new(2, false)];
        assert!(!has_membership(&memberships, &make_conf(2)));
    }

    #[test]
    fn has_membership_only_matches_the_named_conference() {
        let memberships = vec![ConferenceMembership::new(1, true)];
        assert!(!has_membership(&memberships, &make_conf(2)));
    }

    #[test]
    fn first_accessible_conference_returns_lowest_numbered_granted_conference() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let memberships = vec![
            ConferenceMembership::new(2, true),
            ConferenceMembership::new(3, true),
        ];
        let first = first_accessible_conference(&memberships, &confs).expect("some");
        assert_eq!(first.number(), 2);
    }

    #[test]
    fn first_accessible_conference_skips_revoked_rows() {
        let confs = vec![make_conf(1), make_conf(2)];
        let memberships = vec![
            ConferenceMembership::new(1, false),
            ConferenceMembership::new(2, true),
        ];
        let first = first_accessible_conference(&memberships, &confs).expect("some");
        assert_eq!(first.number(), 2);
    }

    #[test]
    fn first_accessible_conference_returns_none_when_no_grants_match() {
        let confs = vec![make_conf(1), make_conf(2)];
        let memberships = vec![ConferenceMembership::new(99, true)];
        assert!(first_accessible_conference(&memberships, &confs).is_none());
    }

    #[test]
    fn first_accessible_conference_returns_none_for_empty_memberships() {
        let confs = vec![make_conf(1)];
        let memberships: Vec<ConferenceMembership> = vec![];
        assert!(first_accessible_conference(&memberships, &confs).is_none());
    }

    #[test]
    fn new_membership_has_no_read_pointer_rows() {
        // Per the schema-growth doc-comment: pointer rows are created
        // lazily by the first ReadMail / ScanMail / ScanMailOnJoin
        // call. A freshly-constructed membership must not pre-allocate
        // them.
        let m = ConferenceMembership::new(5, true);
        assert!(m.pointers().is_empty());
        assert!(m.pointers_for(1).is_none());
    }

    #[test]
    fn upsert_pointers_appends_new_rows() {
        let mut m = ConferenceMembership::new(5, true);
        m.upsert_pointers(ReadPointers::fresh(1, std::time::SystemTime::UNIX_EPOCH));
        m.upsert_pointers(ReadPointers::fresh(2, std::time::SystemTime::UNIX_EPOCH));
        assert_eq!(m.pointers().len(), 2);
        let bases: Vec<u32> = m
            .pointers()
            .iter()
            .map(ReadPointers::msgbase_number)
            .collect();
        assert_eq!(bases, vec![1, 2]);
    }

    #[test]
    fn upsert_pointers_replaces_existing_rows_for_same_msgbase() {
        let mut m = ConferenceMembership::new(5, true);
        m.upsert_pointers(ReadPointers::fresh(1, std::time::SystemTime::UNIX_EPOCH));
        let replaced =
            ReadPointers::new(1, 4, 4, std::time::SystemTime::UNIX_EPOCH).expect("valid");
        m.upsert_pointers(replaced);

        assert_eq!(m.pointers().len(), 1);
        let only = m.pointers_for(1).expect("present");
        assert_eq!(only.last_read(), 4);
        assert_eq!(only.last_scanned(), 4);
    }

    #[test]
    fn pointers_for_mut_returns_mutable_handle_to_existing_row() {
        let mut m = ConferenceMembership::new(5, true);
        m.upsert_pointers(ReadPointers::fresh(2, std::time::SystemTime::UNIX_EPOCH));
        let row = m.pointers_for_mut(2).expect("present");
        row.advance_last_read(3);
        assert_eq!(m.pointers_for(2).expect("present").last_read(), 3);
    }

    #[test]
    fn pointers_for_mut_returns_none_when_msgbase_unknown() {
        let mut m = ConferenceMembership::new(5, true);
        m.upsert_pointers(ReadPointers::fresh(2, std::time::SystemTime::UNIX_EPOCH));
        assert!(m.pointers_for_mut(99).is_none());
    }

    #[test]
    fn new_membership_starts_with_zero_messages_posted() {
        // Spec core.allium:ConferenceMembership.messages_posted is
        // initialised to 0 by `session.allium:CompleteNewUserRegistration`
        // (line 532) and `conferences.allium:SysopGrantsConferenceAccess`
        // (line 269).
        let m = ConferenceMembership::new(5, true);
        assert_eq!(m.messages_posted(), 0);
    }

    #[test]
    fn bump_messages_posted_increments_by_one() {
        // Spec messaging.allium:PostMail (Slice 42) consequent:
        //   membership.messages_posted = membership.messages_posted + 1
        let mut m = ConferenceMembership::new(5, true);
        m.bump_messages_posted();
        m.bump_messages_posted();
        assert_eq!(m.messages_posted(), 2);
    }
}
