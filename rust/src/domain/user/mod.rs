//! [`User`] entity (spec: `core.allium:User`).
//!
//! `User` remains the aggregate root. Internally, related state is
//! grouped into private value objects (`Credentials`, `AccountStatus`,
//! `UsageAccounting`, `Profile`, `RatioPolicy`, and
//! `ConferenceAccess`) so invariants live near the data they protect
//! while the public user API stays stable for rules and adapters.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use crate::domain::conference::{Conference, ConferenceMembership, MessageBase, MessageBaseRef};
use crate::domain::messaging::read_pointers::ReadPointers;
use crate::domain::password::PasswordHashKind;

/// Maximum value the user-typed `line_length` registration field
/// accepts. The legacy `AmiExpress` display routines store line length
/// in a single byte, so values above `255` are rejected; the constant
/// keeps the limit colocated with the [`User::line_length`] getter.
pub const MAX_LINE_LENGTH: u32 = 255;

mod account_status;
mod conference_access;
mod credentials;
mod draft;
mod persisted;
mod profile;
mod ratio_policy;
mod usage_accounting;

use account_status::AccountStatus;
use conference_access::ConferenceAccess;
use credentials::Credentials;
use profile::Profile;
use ratio_policy::RatioPolicy;
use usage_accounting::UsageAccounting;

pub use draft::NewUserDraft;
pub use persisted::PersistedUser;

/// Ratio enforcement mode for a user (spec: `core.allium:RatioMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatioMode {
    /// Ratio enforcement is off.
    Disabled,
    /// Enforce uploads:downloads file count.
    ByFiles,
    /// Enforce uploads:downloads byte count.
    ByBytes,
}

/// Access rights checked by the spec's `has_access(user, right)`
/// black-box function (catalogued across `conferences.allium`,
/// `messaging.allium`, and `files.allium`).
///
/// Each variant corresponds to a `has_access(_, <right>)` call in a
/// rule's `requires` clause. The mapping from a [`User`]'s `access_level`
/// (and other state) to the set of granted rights is the responsibility
/// of [`User::has_access`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Right {
    /// `messaging.allium:ReadMail` precondition.
    ReadMessage,
    /// `messaging.allium:PostMail` precondition.
    EnterMessage,
    /// `messaging.allium:PostCommentToSysop` precondition.
    CommentToSysop,
    /// `messaging.allium:EditMailHeader` precondition.
    MessageEdit,
    /// `messaging.allium:AttachFileToMail` precondition.
    AttachFiles,
    /// `files.allium:BeginDownload` precondition.
    Download,
    /// `files.allium:BeginUpload` precondition.
    Upload,
    /// `files.allium:CheckDownloadEligibility` time-limit override.
    OverrideTimeLimit,
    /// `files.allium:MoveFile` / `DeleteFile` precondition.
    EditFiles,
    /// `conferences.allium:CreateConference` precondition.
    CreateConference,
    /// `conferences.allium:EditConferenceScanFlags` precondition — the
    /// `CF` command's `ACS_CONFFLAGS` gate (`amiexpress/express.e:24686`).
    EditConferenceFlags,
}

impl Right {
    /// Returns every variant in declaration order. Useful for tests
    /// and any callers that need to iterate the full rights catalogue.
    #[must_use]
    pub fn all() -> [Self; 11] {
        [
            Self::ReadMessage,
            Self::EnterMessage,
            Self::CommentToSysop,
            Self::MessageEdit,
            Self::AttachFiles,
            Self::Download,
            Self::Upload,
            Self::OverrideTimeLimit,
            Self::EditFiles,
            Self::CreateConference,
            Self::EditConferenceFlags,
        ]
    }
}

/// Bit-flag preferences persisted on a user record
/// (spec: `core.allium:UserFlag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UserFlag {
    /// Show the "new user" greeting once.
    ShowNewUserMessage,
    /// Auto-join the first conference on logon.
    AutoJoinFirstConf,
    /// Show one-time messages.
    ShowOneTimeMessages,
    /// Clear the screen after each message.
    ScreenClearAfterMessage,
    /// User has paid; affects screens, not access.
    IsDonor,
    /// Use the full-screen editor.
    EditorFullScreen,
    /// Show editor prompts.
    EditorPrompts,
    /// Check uploads asynchronously in the background.
    BackgroundFileCheck,
}

/// A registered BBS user.
///
/// Construct via [`User::new`], which enforces the
/// `SaltMatchesAlgorithm` invariant from the spec. The lockout state
/// (`invalid_attempts`, `account_locked`) starts cleared and is mutated
/// by the `VerifyPassword` rule.
#[derive(Debug, Clone)]
pub struct User {
    slot_number: u32,
    handle: String,
    credentials: Credentials,
    account: AccountStatus,
    usage: UsageAccounting,
    profile: Profile,
    ratio: RatioPolicy,
    conferences: ConferenceAccess,
}

impl User {
    /// Constructs a new [`User`].
    ///
    /// # Parameters
    /// - `slot_number`: stable account id; `1` is the sysop.
    /// - `handle`: unique login name.
    /// - `password_hash_kind`, `password_hash`, `password_salt`: the
    ///   opaque credential triple verified by the password adapter.
    /// - `password_last_updated`: when the credential triple was last
    ///   rotated.
    /// - `access_level`: `0..=255` access tier (`0` = locked out).
    ///
    /// # Errors
    /// Returns [`UserError::SaltRequired`] when `password_hash_kind` is
    /// a PBKDF2 variant and `password_salt` is `None`. This enforces
    /// the spec's `SaltMatchesAlgorithm` invariant.
    pub fn new(
        slot_number: u32,
        handle: String,
        password_hash_kind: PasswordHashKind,
        password_hash: String,
        password_salt: Option<String>,
        password_last_updated: SystemTime,
        access_level: u8,
    ) -> Result<Self, UserError> {
        let credentials = Credentials::new(
            password_hash_kind,
            password_hash,
            password_salt,
            password_last_updated,
        )?;
        Ok(Self {
            slot_number,
            handle,
            credentials,
            account: AccountStatus::existing(access_level),
            usage: UsageAccounting::new(),
            profile: Profile::existing(password_last_updated),
            ratio: RatioPolicy::disabled(),
            conferences: ConferenceAccess::new(),
        })
    }

    /// Builds a freshly-registered new user from an allocated slot and
    /// a completed registration draft.
    ///
    /// Mirrors the `User.created(...)` consequent of
    /// `session.allium:CompleteNewUserRegistration` (Slice 20). All
    /// non-profile fields are set to the spec's exact defaults: access
    /// level `2`, `is_new_user = true`, `force_password_reset = false`,
    /// thirty-minute per-call / one-hour per-day allowances, zeroed
    /// counters, ZMODEM as the preferred protocol (held implicitly
    /// until Slice 53 introduces the field), and `account_created` /
    /// `last_call` / `password_last_updated` all set to `now`.
    ///
    /// `slot` is supplied by the caller — production callers receive it
    /// from [`crate::domain::user_repository::UserRepository::create_user`],
    /// which allocates the next free slot inside its own transaction.
    ///
    /// # Errors
    /// Returns [`UserError::SaltRequired`] when
    /// `draft.password_hash_kind` is a PBKDF2 variant and
    /// `draft.password_salt` is `None`. This enforces the spec's
    /// `SaltMatchesAlgorithm` invariant.
    pub fn register_new(slot: u32, draft: NewUserDraft) -> Result<Self, UserError> {
        let NewUserDraft {
            handle,
            location,
            phone_number,
            email,
            password_hash,
            password_salt,
            password_hash_kind,
            line_length,
            ansi_colour,
            flags,
            ratio_mode,
            ratio_value,
            now,
        } = draft;
        let credentials = Credentials::new(password_hash_kind, password_hash, password_salt, now)?;
        Ok(Self {
            slot_number: slot,
            handle,
            credentials,
            account: AccountStatus::awaiting_validation(),
            usage: UsageAccounting::for_fresh_registration(now),
            profile: Profile::registered(
                location,
                phone_number,
                email,
                line_length,
                ansi_colour,
                // New accounts start in non-expert mode
                // (`amiexpress/express.e:30452`).
                false,
                now,
                flags,
            ),
            ratio: RatioPolicy::new(ratio_mode, ratio_value),
            conferences: ConferenceAccess::new(),
        })
    }

    /// Reconstructs a [`User`] from a [`PersistedUser`] snapshot read
    /// from durable storage (e.g. the `SQLite` adapter).
    ///
    /// Mirrors the field assignments performed by [`User::new`] and
    /// [`User::register_new`] but threads every persisted counter and
    /// preference verbatim. The `SaltMatchesAlgorithm` invariant is
    /// enforced here too: PBKDF2 records must carry a salt.
    ///
    /// # Errors
    /// Returns [`UserError::SaltRequired`] when `state.password_hash_kind`
    /// is a PBKDF2 variant and `state.password_salt` is `None`.
    pub fn from_persisted(state: PersistedUser) -> Result<Self, UserError> {
        let PersistedUser {
            slot_number,
            handle,
            password_hash_kind,
            password_hash,
            password_salt,
            password_last_updated,
            force_password_reset,
            access_level,
            invalid_attempts,
            account_locked,
            is_new_user,
            censored,
            times_called,
            times_called_today,
            last_call,
            time_limit_per_call,
            time_limit_per_day,
            time_used_today,
            location,
            phone_number,
            email,
            line_length,
            ansi_colour,
            expert_mode,
            account_created,
            flags,
            ratio_mode,
            ratio_value,
            memberships,
            last_joined,
            messages_posted,
        } = state;
        let mut credentials = Credentials::new(
            password_hash_kind,
            password_hash,
            password_salt,
            password_last_updated,
        )?;
        credentials.set_reset_required(force_password_reset);
        let account = AccountStatus::from_persisted(
            access_level,
            invalid_attempts,
            account_locked,
            is_new_user,
            censored,
        );
        let usage = UsageAccounting::from_persisted(
            times_called,
            times_called_today,
            last_call,
            time_limit_per_call,
            time_limit_per_day,
            time_used_today,
        );
        let profile = Profile::registered(
            location,
            phone_number,
            email,
            line_length,
            ansi_colour,
            expert_mode,
            account_created,
            flags,
        );
        let ratio = RatioPolicy::new(ratio_mode, ratio_value);
        let conferences =
            ConferenceAccess::from_persisted(memberships, last_joined, messages_posted);
        Ok(Self {
            slot_number,
            handle,
            credentials,
            account,
            usage,
            profile,
            ratio,
            conferences,
        })
    }

    /// Returns `true` when this user is the sysop (slot `1`).
    #[must_use]
    pub fn is_sysop(&self) -> bool {
        self.slot_number == 1
    }

    /// Returns this user's stable slot number
    /// (`core.allium:User.slot_number`).
    #[must_use]
    pub fn slot_number(&self) -> u32 {
        self.slot_number
    }

    /// Returns the user's handle (login name).
    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Returns the algorithm used to verify the stored password hash.
    #[must_use]
    pub fn password_hash_kind(&self) -> PasswordHashKind {
        self.credentials.hash_kind()
    }

    /// Returns the opaque stored password hash.
    #[must_use]
    pub fn password_hash(&self) -> &str {
        self.credentials.hash()
    }

    /// Returns the salt the stored password hash was bound to, if the
    /// algorithm uses one.
    #[must_use]
    pub fn password_salt(&self) -> Option<&str> {
        self.credentials.salt()
    }

    /// Returns the number of recent invalid password attempts. Cleared
    /// to zero when the account is locked or a successful login lands.
    #[must_use]
    pub fn invalid_attempts(&self) -> u32 {
        self.account.invalid_attempts()
    }

    /// Returns whether the account is currently locked out.
    #[must_use]
    pub fn is_account_locked(&self) -> bool {
        self.account.is_account_locked()
    }

    /// Returns the user's access tier (`0..=255`).
    #[must_use]
    pub fn access_level(&self) -> u8 {
        self.account.access_level()
    }

    /// Spec-derived predicate (`core.allium:User.is_locked_out`,
    /// Slice 16): `access_level <= 1 or account_locked`.
    ///
    /// `access_level == 0` is the explicit lockout tier; `1` is
    /// reserved as "below the minimum non-locked tier" per the spec
    /// (new users start at `2`). Either lower bound, or an
    /// independently set `account_locked` flag, qualifies.
    #[must_use]
    pub fn is_locked_out(&self) -> bool {
        self.account.is_locked_out()
    }

    /// Increments [`Self::invalid_attempts`] by one. Used by
    /// `session.allium:VerifyPassword` (Slice 11) when a candidate
    /// fails to match.
    pub fn bump_invalid_attempts(&mut self) {
        self.account.bump_invalid_attempts();
    }

    /// Resets [`Self::invalid_attempts`] to zero.
    pub fn clear_invalid_attempts(&mut self) {
        self.account.clear_invalid_attempts();
    }

    /// Marks the account as locked and resets `invalid_attempts` to
    /// preserve the spec's `LockoutClearsAttempts` invariant.
    pub fn lock_account(&mut self) {
        self.account.lock_account();
    }

    /// Returns the number of completed logons recorded for this user.
    #[must_use]
    pub fn times_called(&self) -> u32 {
        self.usage.times_called()
    }

    /// Returns the timestamp of the most recently completed logon, if
    /// any.
    #[must_use]
    pub fn last_call(&self) -> Option<SystemTime> {
        self.usage.last_call()
    }

    /// Increments [`Self::times_called`] by one. Used by
    /// `session.allium:EnterMenu` (Slice 12).
    pub fn bump_times_called(&mut self) {
        self.usage.bump_times_called();
    }

    /// Updates [`Self::last_call`] to `at`. Used by
    /// `session.allium:FinaliseLogoff` (Slice 13).
    pub fn record_last_call(&mut self, at: SystemTime) {
        self.usage.record_last_call(at);
    }

    /// Returns the per-call time allowance configured for this user.
    #[must_use]
    pub fn time_limit_per_call(&self) -> Duration {
        self.usage.time_limit_per_call()
    }

    /// Returns the per-day combined time allowance configured for this
    /// user.
    #[must_use]
    pub fn time_limit_per_day(&self) -> Duration {
        self.usage.time_limit_per_day()
    }

    /// Returns how much wall-clock time the user has burned through
    /// today, accumulated across calls in the current accounting day.
    #[must_use]
    pub fn time_used_today(&self) -> Duration {
        self.usage.time_used_today()
    }

    /// Returns the number of completed logons recorded for this user
    /// in the current accounting day.
    #[must_use]
    pub fn times_called_today(&self) -> u32 {
        self.usage.times_called_today()
    }

    /// Sets the per-call and per-day time allowances. Used by the
    /// new-user registration flow and admin tooling.
    ///
    /// # Parameters
    /// - `per_call`: how much time a single visit may consume.
    /// - `per_day`: combined allowance across all visits in one
    ///   accounting day.
    pub fn set_time_limits(&mut self, per_call: Duration, per_day: Duration) {
        self.usage.set_time_limits(per_call, per_day);
    }

    /// Resets the daily counters at the start of a new accounting day.
    ///
    /// Mirrors the new-day branch of `session.allium:InitialiseDailyBudget`
    /// (Slice 14): `times_called_today` and `time_used_today` are
    /// cleared. Daily byte counters and chat-minute accounting land
    /// with the slices that introduce them.
    pub fn reset_daily_counters(&mut self) {
        self.usage.reset_daily_counters();
    }

    /// Increments [`Self::times_called_today`] by one. Used by the
    /// same-day branch of `session.allium:InitialiseDailyBudget`.
    pub fn bump_times_called_today(&mut self) {
        self.usage.bump_times_called_today();
    }

    /// Adds `elapsed` to [`Self::time_used_today`]. Used by
    /// `session.allium:UpdateTimeUsed` (Slice 14).
    pub fn add_time_used_today(&mut self, elapsed: Duration) {
        self.usage.add_time_used_today(elapsed);
    }

    /// Returns the timestamp the user's password hash was last
    /// rotated. Used by `session.allium:ForcePasswordReset` to detect
    /// expiry against `core/config.password_expiry_days` (Slice 15).
    #[must_use]
    pub fn password_last_updated(&self) -> SystemTime {
        self.credentials.last_updated()
    }

    /// Returns whether the next logon must force the user through the
    /// password-change sub-flow (`session.allium:Session.user.force_password_reset`,
    /// Slice 15). Set by `ForcePasswordReset`, cleared by
    /// `CompletePasswordReset`.
    #[must_use]
    pub fn force_password_reset(&self) -> bool {
        self.credentials.reset_required()
    }

    /// Sets [`Self::force_password_reset`]. Used by
    /// `session.allium:ForcePasswordReset` (Slice 15) and by sysop
    /// admin tooling.
    pub fn set_force_password_reset(&mut self, value: bool) {
        self.credentials.set_reset_required(value);
    }

    /// Returns whether this user is censored
    /// (`core.allium:User.censored`, Slice 47). Read by
    /// `messaging.allium:PostMail`'s visibility selector to force
    /// posts to `private_to_sysop`.
    #[must_use]
    pub fn is_censored(&self) -> bool {
        self.account.is_censored()
    }

    /// Sets the user's [`Self::is_censored`] flag. The sysop rule
    /// that flips this in-band lands with Slice 49; in the meantime
    /// the setter is used by storage loading and by tests.
    pub fn set_censored(&mut self, value: bool) {
        self.account.set_censored(value);
    }

    /// Returns whether this account is awaiting sysop validation
    /// (`core.allium:User.is_new_user`). Set by
    /// `session.allium:CompleteNewUserRegistration` (Slice 20);
    /// cleared by the sysop validate-user workflow that lands in
    /// Phase 5.
    #[must_use]
    pub fn is_new_user(&self) -> bool {
        self.account.is_new_user()
    }

    /// Returns whether this user has the given access [`Right`]
    /// (`conferences.allium:has_access(user, right)`).
    ///
    /// While [`Self::is_new_user`] is true the account sits in the
    /// pending-validation tier defined by Slice 21: only
    /// [`Right::ReadMessage`] and [`Right::CommentToSysop`] are granted
    /// — every other right is denied until a sysop validates the
    /// account.
    ///
    /// For validated accounts the per-tier mapping from `access_level`
    /// to specific rights is not yet modelled; later phases narrow
    /// this down. Until then a validated user is treated as having
    /// every right.
    #[must_use]
    pub fn has_access(&self, right: Right) -> bool {
        if self.is_new_user() {
            matches!(right, Right::ReadMessage | Right::CommentToSysop)
        } else {
            true
        }
    }

    /// Returns the user's free-text "City, State" location, if any.
    #[must_use]
    pub fn location(&self) -> Option<&str> {
        self.profile.location()
    }

    /// Returns the user's phone number on file, if any.
    #[must_use]
    pub fn phone_number(&self) -> Option<&str> {
        self.profile.phone_number()
    }

    /// Returns the user's email address on file, if any.
    #[must_use]
    pub fn email(&self) -> Option<&str> {
        self.profile.email()
    }

    /// Returns the user's preferred terminal width (`0` = auto).
    #[must_use]
    pub fn line_length(&self) -> u32 {
        self.profile.line_length()
    }

    /// Returns whether the user wants ANSI colour output.
    #[must_use]
    pub fn ansi_colour(&self) -> bool {
        self.profile.ansi_colour()
    }

    /// Returns whether the user is in expert mode (Tier A quickwin A6).
    /// In expert mode the menu screen is not auto-displayed before the
    /// command prompt; the user requests it with `?`.
    #[must_use]
    pub fn expert_mode(&self) -> bool {
        self.profile.expert_mode()
    }

    /// Sets the user's expert-mode flag (the `X` command's mutation).
    /// Persisted with the user record on logoff.
    pub fn set_expert_mode(&mut self, value: bool) {
        self.profile.set_expert_mode(value);
    }

    /// Returns the timestamp the account was first created.
    #[must_use]
    pub fn account_created(&self) -> SystemTime {
        self.profile.account_created()
    }

    /// Returns the user's preference flags
    /// (`core.allium:User.flags`).
    #[must_use]
    pub fn flags(&self) -> &BTreeSet<UserFlag> {
        self.profile.flags()
    }

    /// Returns the ratio enforcement mode in effect for this user.
    #[must_use]
    pub fn ratio_mode(&self) -> RatioMode {
        self.ratio.mode()
    }

    /// Returns the configured ratio threshold (e.g. `3` = three
    /// downloads per upload). `0` with a non-disabled mode means
    /// infinite.
    #[must_use]
    pub fn ratio_value(&self) -> u32 {
        self.ratio.value()
    }

    /// Returns the user's per-conference membership rows
    /// (`core.allium:User.memberships`).
    #[must_use]
    pub fn memberships(&self) -> &[ConferenceMembership] {
        self.conferences.memberships()
    }

    /// Returns a mutable slice over the user's per-conference
    /// membership rows. Used by `messaging.allium:PostMail` (Slice 42)
    /// to bump the per-membership `messages_posted` counter without
    /// dropping the surrounding borrow on `self`.
    pub fn memberships_mut(&mut self) -> &mut [ConferenceMembership] {
        self.conferences.memberships_mut()
    }

    /// Adds a [`ConferenceMembership`] row, replacing any existing
    /// row for the same `conference_number` so the user record never
    /// carries two rows for the same conference. Used by
    /// `conferences.allium:SysopGrantsConferenceAccess`'s "create new
    /// row" branch and by adapters seeding a user record.
    pub fn upsert_membership(&mut self, membership: ConferenceMembership) {
        self.conferences.upsert_membership(membership);
    }

    /// Toggles the `granted` flag on the membership for
    /// `conference_number`.
    ///
    /// Mirrors `conferences.allium:SysopGrantsConferenceAccess`'s
    /// "existing row" branch and `SysopRevokesConferenceAccess`. When
    /// no row exists for the conference, returns `false` so the
    /// caller can decide whether to create one (grant) or surface an
    /// error (revoke).
    pub fn set_membership_granted(&mut self, conference_number: u32, granted: bool) -> bool {
        self.conferences
            .set_membership_granted(conference_number, granted)
    }

    /// Returns `true` when the user has a granted membership for
    /// `conference` (spec: `conferences.allium:has_membership`).
    #[must_use]
    pub fn has_membership(&self, conference: &Conference) -> bool {
        self.conferences.has_membership(conference)
    }

    /// Returns `true` when the user holds a granted membership row for
    /// `conference_number` — the messaging-rule analogue of
    /// [`Self::has_membership`] that takes the bare conference number
    /// (as carried on a [`MessageBaseRef`]) instead of the full
    /// [`Conference`] catalogue entry.
    ///
    /// A revoked row (`granted = false`) returns `false` here, matching
    /// the spec's intent that revoking access immediately denies reads,
    /// scans and posts to that conference. Used by
    /// [`crate::domain::messaging::read_mail::read_mail`],
    /// [`crate::domain::messaging::scan_mail::scan_mail`] and
    /// [`crate::domain::messaging::post_mail::post_mail`].
    #[must_use]
    pub fn has_granted_membership_for(&self, conference_number: u32) -> bool {
        self.conferences
            .has_granted_membership_for(conference_number)
    }

    /// Returns the user's last-joined (conference, msgbase) pair, if
    /// any. Mirrors the `last_joined_conference` /
    /// `last_joined_msgbase` pair on `core.allium:User`. They are
    /// modelled here as a single optional [`MessageBaseRef`] so the
    /// `VisitedMsgBaseBelongsToVisitedConference` invariant cannot
    /// be violated by setting one without the other.
    #[must_use]
    pub fn last_joined(&self) -> Option<MessageBaseRef> {
        self.conferences.last_joined()
    }

    /// Records that the user joined `msgbase` inside `conference`.
    /// Used by `conferences.allium:JoinConference` (Slice 30).
    pub fn record_join(&mut self, conference: &Conference, msgbase: &MessageBase) {
        self.conferences.record_join(conference, msgbase);
    }

    /// Returns the [`ReadPointers`] row this user holds for `msgbase`,
    /// if any. This is the spec's
    /// `core.allium:read_pointers_for(user, msgbase)` helper.
    ///
    /// A `None` return value means either: the user has no membership
    /// row for the parent conference at all, or the membership exists
    /// but no rule has yet caused a [`ReadPointers`] row to be created
    /// for `msgbase`. The two cases share a return value because all
    /// of `ReadMail`, `ScanMail`, and `ScanMailOnJoin` treat them
    /// equivalently — they lazily create a fresh row before mutating.
    #[must_use]
    pub fn read_pointers_for(&self, msgbase: MessageBaseRef) -> Option<&ReadPointers> {
        self.conferences.read_pointers_for(msgbase)
    }

    /// Returns a mutable reference to the [`ReadPointers`] row for
    /// `msgbase`, if any. Same lookup semantics as
    /// [`Self::read_pointers_for`].
    pub fn read_pointers_for_mut(&mut self, msgbase: MessageBaseRef) -> Option<&mut ReadPointers> {
        self.conferences.read_pointers_for_mut(msgbase)
    }

    /// Inserts or replaces a [`ReadPointers`] row for `msgbase` on the
    /// user's [`ConferenceMembership`] for the parent conference.
    ///
    /// Returns `true` when the row was upserted, `false` when the user
    /// has no membership row for `msgbase.conference_number()` (in
    /// which case the caller should refuse the operation — the spec's
    /// `read_message` precondition implies the user has access).
    pub fn upsert_read_pointers(&mut self, pointers: ReadPointers, conference_number: u32) -> bool {
        self.conferences
            .upsert_read_pointers(pointers, conference_number)
    }

    /// Returns the running count of messages this user has posted
    /// across all conferences (spec: `core.allium:User.messages_posted`).
    #[must_use]
    pub fn messages_posted(&self) -> u32 {
        self.conferences.messages_posted()
    }

    /// Increments [`Self::messages_posted`] by one. Used by
    /// `messaging.allium:PostMail` (Slice 42).
    pub fn bump_messages_posted(&mut self) {
        self.conferences.bump_messages_posted();
    }

    /// Returns a [`PersistedUser`] snapshot of every field the durable
    /// store is expected to persist.
    ///
    /// Symmetric with [`User::from_persisted`]: round-tripping a
    /// `User` through `to_persisted` → `from_persisted` reproduces the
    /// same observable state. Adapters use this to project a `User`
    /// onto their schema without sprinkling individual getter calls
    /// through the write path.
    #[must_use]
    pub fn to_persisted(&self) -> PersistedUser {
        PersistedUser {
            slot_number: self.slot_number,
            handle: self.handle.clone(),
            password_hash_kind: self.credentials.hash_kind(),
            password_hash: self.credentials.hash().to_string(),
            password_salt: self.credentials.salt().map(str::to_string),
            password_last_updated: self.credentials.last_updated(),
            force_password_reset: self.credentials.reset_required(),
            access_level: self.account.access_level(),
            invalid_attempts: self.account.invalid_attempts(),
            account_locked: self.account.is_account_locked(),
            is_new_user: self.account.is_new_user(),
            censored: self.account.is_censored(),
            times_called: self.usage.times_called(),
            times_called_today: self.usage.times_called_today(),
            last_call: self.usage.last_call(),
            time_limit_per_call: self.usage.time_limit_per_call(),
            time_limit_per_day: self.usage.time_limit_per_day(),
            time_used_today: self.usage.time_used_today(),
            location: self.profile.location().map(str::to_string),
            phone_number: self.profile.phone_number().map(str::to_string),
            email: self.profile.email().map(str::to_string),
            line_length: self.profile.line_length(),
            ansi_colour: self.profile.ansi_colour(),
            expert_mode: self.profile.expert_mode(),
            account_created: self.profile.account_created(),
            flags: self.profile.flags().clone(),
            ratio_mode: self.ratio.mode(),
            ratio_value: self.ratio.value(),
            memberships: self.conferences.memberships().to_vec(),
            last_joined: self.conferences.last_joined(),
            messages_posted: self.conferences.messages_posted(),
        }
    }

    /// Atomically replaces the user's stored credentials and clears
    /// [`Self::force_password_reset`].
    ///
    /// Mirrors the `ensures` block of
    /// `session.allium:CompletePasswordReset` (Slice 15): updates
    /// `password_hash`, `password_salt`, `password_hash_kind`,
    /// `password_last_updated`, and resets `force_password_reset`.
    ///
    /// # Parameters
    /// - `hash`: opaque output of [`PasswordHasher::compute_password_hash`].
    /// - `salt`: salt the hash was bound to (`None` for hash kinds
    ///   that don't take a salt).
    /// - `kind`: algorithm used for `hash`.
    /// - `at`: timestamp the change happened.
    ///
    /// [`PasswordHasher::compute_password_hash`]: crate::domain::password::PasswordHasher::compute_password_hash
    pub fn record_password_change(
        &mut self,
        hash: String,
        salt: Option<String>,
        kind: PasswordHashKind,
        at: SystemTime,
    ) {
        self.credentials.record_change(hash, salt, kind, at);
    }
}

/// Errors returned by [`User::new`] and [`User::from_persisted`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    /// The chosen [`PasswordHashKind`] requires a non-null salt
    /// (spec invariant `SaltMatchesAlgorithm`).
    #[error("password hash kind requires a salt")]
    SaltRequired,
}

/// Whether the spec's `SaltMatchesAlgorithm` invariant requires a non-null
/// salt for `kind`.
fn requires_salt(kind: PasswordHashKind) -> bool {
    match kind {
        PasswordHashKind::Pbkdf210000 => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(slot: u32, salt: Option<String>) -> Result<User, UserError> {
        User::new(
            slot,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
    }

    #[test]
    fn slot_one_is_sysop() {
        let user = make_user(1, Some("salt".to_string())).expect("valid user");
        assert!(user.is_sysop());
    }

    #[test]
    fn other_slots_are_not_sysop() {
        let user = make_user(2, Some("salt".to_string())).expect("valid user");
        assert!(!user.is_sysop());
    }

    #[test]
    fn pbkdf2_without_salt_is_rejected() {
        let err = make_user(1, None).expect_err("missing salt should error");
        assert_eq!(err, UserError::SaltRequired);
    }

    #[test]
    fn pbkdf2_with_salt_is_accepted() {
        assert!(make_user(1, Some("salt".to_string())).is_ok());
    }

    #[test]
    fn new_user_has_clean_lockout_state() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.invalid_attempts(), 0);
        assert!(!user.is_account_locked());
    }

    #[test]
    fn bump_invalid_attempts_increments() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.bump_invalid_attempts();
        assert_eq!(user.invalid_attempts(), 2);
    }

    #[test]
    fn clear_invalid_attempts_resets() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.clear_invalid_attempts();
        assert_eq!(user.invalid_attempts(), 0);
    }

    #[test]
    fn lock_account_clears_attempts() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_invalid_attempts();
        user.bump_invalid_attempts();
        user.lock_account();
        assert!(user.is_account_locked());
        // LockoutClearsAttempts invariant.
        assert_eq!(user.invalid_attempts(), 0);
    }

    #[test]
    fn new_user_has_zero_time_accounting() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.time_limit_per_call(), Duration::ZERO);
        assert_eq!(user.time_limit_per_day(), Duration::ZERO);
        assert_eq!(user.time_used_today(), Duration::ZERO);
        assert_eq!(user.times_called_today(), 0);
    }

    #[test]
    fn set_time_limits_updates_both_caps() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.set_time_limits(Duration::from_mins(1), Duration::from_hours(1));
        assert_eq!(user.time_limit_per_call(), Duration::from_mins(1));
        assert_eq!(user.time_limit_per_day(), Duration::from_hours(1));
    }

    #[test]
    fn reset_daily_counters_clears_today_counters() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_times_called_today();
        user.add_time_used_today(Duration::from_mins(2));
        user.reset_daily_counters();
        assert_eq!(user.times_called_today(), 0);
        assert_eq!(user.time_used_today(), Duration::ZERO);
    }

    #[test]
    fn bump_times_called_today_increments() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_times_called_today();
        user.bump_times_called_today();
        assert_eq!(user.times_called_today(), 2);
    }

    #[test]
    fn add_time_used_today_accumulates() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.add_time_used_today(Duration::from_secs(30));
        user.add_time_used_today(Duration::from_secs(45));
        assert_eq!(user.time_used_today(), Duration::from_secs(75));
    }

    #[test]
    fn newly_constructed_user_is_not_censored_by_default() {
        // Spec `core.allium:User.censored: Boolean` defaults to false
        // until a sysop flips it (Slice 49). The flag exists on every
        // user record from Slice 47 onwards.
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.is_censored());
    }

    #[test]
    fn set_censored_round_trips() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.set_censored(true);
        assert!(user.is_censored());
        user.set_censored(false);
        assert!(!user.is_censored());
    }

    #[test]
    fn new_user_does_not_force_password_reset() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.force_password_reset());
    }

    #[test]
    fn set_force_password_reset_round_trips() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.set_force_password_reset(true);
        assert!(user.force_password_reset());
        user.set_force_password_reset(false);
        assert!(!user.force_password_reset());
    }

    #[test]
    fn access_level_returned_via_accessor() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.access_level(), 100);
    }

    #[test]
    fn is_locked_out_when_access_level_at_or_below_one() {
        let user_zero = User::new(
            2,
            "lo0".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            0,
        )
        .unwrap();
        let user_one = User::new(
            3,
            "lo1".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            1,
        )
        .unwrap();
        let user_two = User::new(
            4,
            "ok".to_string(),
            PasswordHashKind::Pbkdf210000,
            "h".to_string(),
            Some("s".to_string()),
            SystemTime::UNIX_EPOCH,
            2,
        )
        .unwrap();
        assert!(user_zero.is_locked_out());
        assert!(user_one.is_locked_out());
        assert!(!user_two.is_locked_out());
    }

    #[test]
    fn is_locked_out_when_account_locked_regardless_of_access_level() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        // Default access level is 100 — well clear of the threshold.
        assert!(!user.is_locked_out());
        user.lock_account();
        assert!(user.is_locked_out());
    }

    #[test]
    fn register_new_user_starts_in_non_expert_mode() {
        // Tier A quickwin A6: new accounts default to `expert = "N"`
        // (`amiexpress/express.e:30452`).
        let user = User::register_new(7, draft()).expect("valid");
        assert!(!user.expert_mode());
    }

    #[test]
    fn set_expert_mode_round_trips() {
        // The `X` command flips `User.expert_mode` in place; the
        // toggle reads then writes via this setter.
        let mut user = User::register_new(7, draft()).expect("valid");
        user.set_expert_mode(true);
        assert!(user.expert_mode());
        user.set_expert_mode(false);
        assert!(!user.expert_mode());
    }

    fn draft() -> NewUserDraft {
        NewUserDraft {
            handle: "newbie".to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555-0123".to_string()),
            email: Some("newbie@example.com".to_string()),
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 80,
            ansi_colour: true,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::ByFiles,
            ratio_value: 3,
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        }
    }

    #[test]
    fn register_new_applies_spec_defaults_for_a_fresh_account() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let user = User::register_new(7, draft()).expect("valid");
        // Identity carried from caller + draft.
        assert_eq!(user.slot_number, 7);
        assert_eq!(user.handle(), "newbie");
        assert_eq!(user.location(), Some("Townsville"));
        assert_eq!(user.phone_number(), Some("555-0123"));
        assert_eq!(user.email(), Some("newbie@example.com"));
        assert_eq!(user.line_length(), 80);
        assert!(user.ansi_colour());
        // Spec defaults.
        assert_eq!(user.access_level(), 2);
        assert!(user.is_new_user());
        assert!(!user.is_account_locked());
        assert!(!user.force_password_reset());
        assert_eq!(user.invalid_attempts(), 0);
        assert_eq!(user.times_called(), 0);
        assert_eq!(user.times_called_today(), 0);
        assert_eq!(user.time_used_today(), Duration::ZERO);
        assert_eq!(user.time_limit_per_call(), Duration::from_mins(30));
        assert_eq!(user.time_limit_per_day(), Duration::from_hours(1));
        assert_eq!(user.last_call(), Some(now));
        assert_eq!(user.account_created(), now);
        assert_eq!(user.password_last_updated(), now);
        assert_eq!(user.ratio_mode(), RatioMode::ByFiles);
        assert_eq!(user.ratio_value(), 3);
        assert!(user.flags().is_empty());
    }

    #[test]
    fn register_new_preserves_profile_flags() {
        let mut draft = draft();
        draft.flags.insert(UserFlag::ShowNewUserMessage);
        draft.flags.insert(UserFlag::EditorPrompts);

        let user = User::register_new(7, draft).expect("valid");

        assert_eq!(user.flags().len(), 2);
        assert!(user.flags().contains(&UserFlag::ShowNewUserMessage));
        assert!(user.flags().contains(&UserFlag::EditorPrompts));
    }

    #[test]
    fn register_new_pbkdf2_without_salt_is_rejected() {
        let mut draft = draft();
        draft.password_salt = None;
        let err = User::register_new(7, draft).expect_err("missing salt should error");
        assert_eq!(err, UserError::SaltRequired);
    }

    #[test]
    fn register_new_user_is_below_lockout_threshold_via_access_level_one() {
        // The spec sets access_level = 2 for new users; downgrading
        // exposes the `is_locked_out` predicate boundary.
        let user = User::register_new(7, draft()).expect("valid");
        assert!(!user.is_locked_out(), "level 2 should be allowed through");
    }

    #[test]
    fn user_new_defaults_extended_fields_for_existing_accounts() {
        // Pre-Slice-20 callers (tests, seed sysop) treat the new
        // fields as off-by-default: not a new user, no contact info,
        // no flags, ratio disabled.
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.is_new_user());
        assert!(user.location().is_none());
        assert!(user.phone_number().is_none());
        assert!(user.email().is_none());
        assert_eq!(user.line_length(), 0);
        assert!(!user.ansi_colour());
        assert!(user.flags().is_empty());
        assert_eq!(user.ratio_mode(), RatioMode::Disabled);
        assert_eq!(user.ratio_value(), 0);
        // account_created mirrors password_last_updated for legacy
        // construction; the registration constructor sets `now`.
        assert_eq!(user.account_created(), user.password_last_updated());
    }

    #[test]
    fn new_user_has_only_read_message_and_comment_to_sysop_rights() {
        // Slice 21: while `is_new_user` is true the account sits in a
        // pending-validation tier. The black-box `has_access` from
        // `conferences.allium` grants only the two non-destructive
        // rights the spec calls out for that tier.
        let user = User::register_new(7, draft()).expect("valid");
        assert!(user.is_new_user());
        assert!(user.has_access(Right::ReadMessage));
        assert!(user.has_access(Right::CommentToSysop));
        assert!(!user.has_access(Right::EnterMessage));
        assert!(!user.has_access(Right::Download));
        assert!(!user.has_access(Right::Upload));
        assert!(!user.has_access(Right::MessageEdit));
        assert!(!user.has_access(Right::CreateConference));
        assert!(!user.has_access(Right::EditFiles));
        assert!(!user.has_access(Right::AttachFiles));
        assert!(!user.has_access(Right::OverrideTimeLimit));
        // C5: an awaiting-validation new user cannot edit conference flags.
        assert!(!user.has_access(Right::EditConferenceFlags));
    }

    #[test]
    fn existing_user_has_every_right_until_per_tier_modelling_lands() {
        // Slice 21 only models the new-user tier; for validated users
        // every right is granted until later phases narrow the mapping
        // from `access_level` to specific rights.
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.is_new_user());
        for right in Right::all() {
            assert!(
                user.has_access(right),
                "existing user should have {right:?}"
            );
        }
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
    fn new_user_has_no_memberships_or_last_joined() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(user.memberships().is_empty());
        assert!(user.last_joined().is_none());
        assert!(!user.has_membership(&make_conf(1)));
    }

    #[test]
    fn upsert_membership_appends_new_rows() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(1, true));
        user.upsert_membership(ConferenceMembership::new(2, true));
        assert_eq!(user.memberships().len(), 2);
        let nums: Vec<u32> = user
            .memberships()
            .iter()
            .map(ConferenceMembership::conference_number)
            .collect();
        assert_eq!(nums, vec![1, 2]);
    }

    #[test]
    fn upsert_membership_replaces_existing_rows() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(1, true));
        user.upsert_membership(ConferenceMembership::new(1, false));
        assert_eq!(user.memberships().len(), 1);
        assert!(!user.memberships()[0].is_granted());
    }

    #[test]
    fn has_membership_uses_conference_number_match() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(2, true));
        assert!(user.has_membership(&make_conf(2)));
        assert!(!user.has_membership(&make_conf(1)));
    }

    #[test]
    fn has_membership_ignores_revoked_rows() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(1, false));
        assert!(!user.has_membership(&make_conf(1)));
    }

    #[test]
    fn set_membership_granted_toggles_existing_row() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(1, true));
        assert!(user.set_membership_granted(1, false));
        assert!(!user.has_membership(&make_conf(1)));
        assert!(user.set_membership_granted(1, true));
        assert!(user.has_membership(&make_conf(1)));
    }

    #[test]
    fn set_membership_granted_returns_false_for_unknown_conference() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(!user.set_membership_granted(99, true));
        assert!(user.memberships().is_empty());
    }

    #[test]
    fn record_join_stores_conference_and_msgbase_pair() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        let conf = make_conf(7);
        let mb = conf.msgbases()[0].clone();
        user.record_join(&conf, &mb);
        let joined = user.last_joined().expect("set");
        assert_eq!(joined.conference_number(), 7);
        assert_eq!(joined.msgbase_number(), 1);
    }

    #[test]
    fn record_join_overwrites_previous_join() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        let confs = [make_conf(1), make_conf(2)];
        user.record_join(&confs[0], &confs[0].msgbases()[0]);
        user.record_join(&confs[1], &confs[1].msgbases()[0]);
        let joined = user.last_joined().expect("set");
        assert_eq!(joined.conference_number(), 2);
    }

    #[test]
    fn read_pointers_for_returns_none_when_no_membership_exists() {
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert!(user.read_pointers_for(MessageBaseRef::new(7, 1)).is_none());
    }

    #[test]
    fn read_pointers_for_returns_none_when_membership_has_no_row_for_msgbase() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(7, true));
        // No pointer rows seeded; the helper must surface this case as
        // None so callers know to lazily-create.
        assert!(user.read_pointers_for(MessageBaseRef::new(7, 1)).is_none());
    }

    #[test]
    fn read_pointers_for_returns_existing_row() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(7, true));
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(50);
        assert!(user.upsert_read_pointers(ReadPointers::new(2, 3, 5, when).expect("valid"), 7,));

        let got = user
            .read_pointers_for(MessageBaseRef::new(7, 2))
            .expect("present");
        assert_eq!(got.msgbase_number(), 2);
        assert_eq!(got.last_read(), 3);
        assert_eq!(got.last_scanned(), 5);
        assert_eq!(got.new_since(), when);
    }

    #[test]
    fn read_pointers_for_does_not_cross_conferences() {
        // A pointer row for (conf=7, msgbase=1) must not satisfy a
        // lookup for (conf=8, msgbase=1) — the conference component of
        // MessageBaseRef is load-bearing.
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(7, true));
        assert!(user.upsert_read_pointers(ReadPointers::fresh(1, SystemTime::UNIX_EPOCH), 7,));
        assert!(user.read_pointers_for(MessageBaseRef::new(8, 1)).is_none());
    }

    #[test]
    fn upsert_read_pointers_refuses_when_no_membership_exists() {
        // The spec's read_message precondition implies the user has a
        // membership; the helper must refuse to silently create one.
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        let upserted =
            user.upsert_read_pointers(ReadPointers::fresh(1, SystemTime::UNIX_EPOCH), 99);
        assert!(!upserted);
        assert!(user.memberships().is_empty());
    }

    #[test]
    fn read_pointers_for_mut_advances_in_place() {
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.upsert_membership(ConferenceMembership::new(7, true));
        user.upsert_read_pointers(ReadPointers::fresh(1, SystemTime::UNIX_EPOCH), 7);

        let row = user
            .read_pointers_for_mut(MessageBaseRef::new(7, 1))
            .expect("present");
        row.advance_last_read(4);

        let after = user
            .read_pointers_for(MessageBaseRef::new(7, 1))
            .expect("present");
        assert_eq!(after.last_read(), 4);
        assert_eq!(after.last_scanned(), 4);
    }

    #[test]
    fn new_user_starts_with_zero_messages_posted() {
        // Spec core.allium:User.messages_posted is initialised to 0 by
        // `session.allium:CompleteNewUserRegistration` (line 532). For
        // legacy User::new construction the field defaults to 0 too —
        // `messages_posted` is the spec's running counter, not state
        // imported from elsewhere.
        let user = make_user(2, Some("salt".to_string())).unwrap();
        assert_eq!(user.messages_posted(), 0);
    }

    #[test]
    fn register_new_user_starts_with_zero_messages_posted() {
        let user = User::register_new(7, draft()).expect("valid");
        assert_eq!(user.messages_posted(), 0);
    }

    #[test]
    fn bump_messages_posted_increments_by_one() {
        // Spec messaging.allium:PostMail (Slice 42) consequent:
        //   session.user.messages_posted = session.user.messages_posted + 1
        let mut user = make_user(2, Some("salt".to_string())).unwrap();
        user.bump_messages_posted();
        user.bump_messages_posted();
        assert_eq!(user.messages_posted(), 2);
    }

    #[test]
    fn to_persisted_then_from_persisted_round_trips_an_existing_user() {
        // The SQLite adapter projects a User onto columns via
        // `to_persisted`, then reconstitutes it via `from_persisted`.
        // Round-tripping must preserve every observable field.
        let mut user = User::register_new(7, draft()).expect("valid");
        user.bump_invalid_attempts();
        user.bump_invalid_attempts();
        user.bump_times_called();
        user.bump_times_called_today();
        user.add_time_used_today(Duration::from_secs(45));
        user.set_force_password_reset(true);
        user.set_censored(true);
        user.set_expert_mode(true);
        user.bump_messages_posted();
        user.upsert_membership(ConferenceMembership::new(1, true));
        user.upsert_membership(ConferenceMembership::new(2, false));
        let mb = MessageBase::new(1, 1, "main".to_string());
        user.record_join(&make_conf(1), &mb);
        user.upsert_read_pointers(
            ReadPointers::new(1, 3, 5, SystemTime::UNIX_EPOCH + Duration::from_secs(200))
                .expect("valid pointers"),
            1,
        );

        let snapshot = user.to_persisted();
        let restored = User::from_persisted(snapshot).expect("restore");

        assert_eq!(restored.slot_number(), user.slot_number());
        assert_eq!(restored.handle(), user.handle());
        assert_eq!(restored.password_hash_kind(), user.password_hash_kind());
        assert_eq!(restored.password_hash(), user.password_hash());
        assert_eq!(restored.password_salt(), user.password_salt());
        assert_eq!(
            restored.password_last_updated(),
            user.password_last_updated()
        );
        assert_eq!(restored.force_password_reset(), user.force_password_reset());
        assert_eq!(restored.access_level(), user.access_level());
        assert_eq!(restored.invalid_attempts(), user.invalid_attempts());
        assert_eq!(restored.is_account_locked(), user.is_account_locked());
        assert_eq!(restored.is_new_user(), user.is_new_user());
        assert_eq!(restored.is_censored(), user.is_censored());
        assert_eq!(restored.times_called(), user.times_called());
        assert_eq!(restored.times_called_today(), user.times_called_today());
        assert_eq!(restored.last_call(), user.last_call());
        assert_eq!(restored.time_limit_per_call(), user.time_limit_per_call());
        assert_eq!(restored.time_limit_per_day(), user.time_limit_per_day());
        assert_eq!(restored.time_used_today(), user.time_used_today());
        assert_eq!(restored.location(), user.location());
        assert_eq!(restored.phone_number(), user.phone_number());
        assert_eq!(restored.email(), user.email());
        assert_eq!(restored.line_length(), user.line_length());
        assert_eq!(restored.ansi_colour(), user.ansi_colour());
        assert_eq!(restored.expert_mode(), user.expert_mode());
        assert_eq!(restored.account_created(), user.account_created());
        assert_eq!(restored.flags(), user.flags());
        assert_eq!(restored.ratio_mode(), user.ratio_mode());
        assert_eq!(restored.ratio_value(), user.ratio_value());
        assert_eq!(restored.messages_posted(), user.messages_posted());
        assert_eq!(restored.last_joined(), user.last_joined());
        assert_eq!(restored.memberships().len(), user.memberships().len());
    }

    #[test]
    fn from_persisted_preserves_locked_account_state_and_attempts() {
        // The lockout-clears-attempts invariant applies to in-flight
        // mutation; restoration must preserve the values exactly as
        // stored (sysop edits to invalid_attempts on a locked account
        // are otherwise unrecoverable across restarts).
        let user = User::register_new(7, draft()).expect("valid");
        let mut snapshot = user.to_persisted();
        snapshot.account_locked = true;
        snapshot.invalid_attempts = 7;
        let restored = User::from_persisted(snapshot).expect("restore");
        assert!(restored.is_account_locked());
        assert_eq!(restored.invalid_attempts(), 7);
    }

    #[test]
    fn from_persisted_rejects_pbkdf2_without_salt() {
        let user = User::register_new(7, draft()).expect("valid");
        let mut snapshot = user.to_persisted();
        snapshot.password_salt = None;
        assert_eq!(
            User::from_persisted(snapshot).expect_err("missing salt"),
            UserError::SaltRequired
        );
    }

    #[test]
    fn record_password_change_updates_credentials_and_clears_flag() {
        let mut user = make_user(2, Some("old_salt".to_string())).unwrap();
        user.set_force_password_reset(true);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        user.record_password_change(
            "new_hash".to_string(),
            Some("new_salt".to_string()),
            PasswordHashKind::Pbkdf210000,
            later,
        );
        assert_eq!(user.password_hash(), "new_hash");
        assert_eq!(user.password_salt(), Some("new_salt"));
        assert_eq!(user.password_hash_kind(), PasswordHashKind::Pbkdf210000);
        assert_eq!(user.password_last_updated(), later);
        assert!(!user.force_password_reset());
    }
}
