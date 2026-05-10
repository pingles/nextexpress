//! Outcome enums returned by [`super::Session`] state-machine
//! transitions.
//!
//! Outcomes are pure data — what happened — separate from the
//! transition itself. The driver uses them to choose what to render
//! to the wire and which next-phase wrapper to construct.

/// Outcome of [`super::Session::name_typed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTypedOutcome {
    /// User found; session has moved to authenticating and is ready
    /// for [`super::Session::user`] to drive the password prompt.
    Authenticated,
    /// Handle did not match any user. The retry counter has been
    /// incremented. The listener should re-prompt.
    NotFound,
    /// Five not-found strikes in a row. The session has ended with
    /// [`super::LogoffReason::NewUserRejected`].
    SessionEnded,
    /// The literal `NEW` was typed and registration is permitted. The
    /// session is in [`super::SessionState::NewUserRegistering`]; the
    /// listener now drives the registration sub-flow.
    /// `password_required` is `true` when the new-user password gate
    /// (Slice 20a) is armed and must pass before
    /// [`super::Session::complete_new_user_registration`] will accept
    /// the form.
    NewUserRegistering {
        /// `true` when the gate must pass before registration.
        password_required: bool,
    },
    /// The literal `NEW` was typed but registration is disabled
    /// (`core/config.allow_new_users = false`). The session has
    /// already moved to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`] via
    /// `RejectDisallowedRegistration`; the caller should run
    /// `FinaliseLogoff`.
    NewUserRegistrationDisallowed,
}

/// Outcome of [`super::Session::record_new_user_request`]. Mirrors
/// the spec's `becomes new_user_registering` cluster: the transition
/// either initialises the gate (Initialised) or is short-circuited by
/// `RejectDisallowedRegistration` (Rejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewUserRequestOutcome {
    /// `core/config.allow_new_users = true`. The session is in
    /// [`super::SessionState::NewUserRegistering`] with gate state
    /// initialised. `password_required` is `true` when the
    /// `new_user_password` gate must pass before completion.
    Initialised {
        /// Mirrors `core/config.new_user_password != null`.
        password_required: bool,
    },
    /// `core/config.allow_new_users = false`. The session has
    /// transitioned to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`].
    Rejected,
}

/// Outcome of [`super::Session::apply_new_user_password_attempt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewUserPasswordOutcome {
    /// The candidate matched. The gate is now verified and
    /// [`super::Session::complete_new_user_registration`] may proceed
    /// once the registration form is collected.
    Verified,
    /// The candidate did not match. The attempt counter has been
    /// incremented and a caller-log entry returned. The listener
    /// should re-prompt.
    Mismatch,
    /// The attempt counter has reached
    /// `core/config.max_new_user_password_attempts`. The session has
    /// moved to [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NewUserRejected`].
    TooManyFailures,
}

/// Outcome of [`super::Session::verify_password`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyPasswordOutcome {
    /// Credentials match. The session has moved to
    /// [`super::SessionState::Onboarded`], `authenticated_at` is set,
    /// and `user.invalid_attempts` is cleared.
    Authenticated,
    /// Credentials do not match. The session stays in
    /// [`super::SessionState::Authenticating`]; the listener should
    /// re-prompt.
    NotMatching,
    /// `user.invalid_attempts` reached `max_password_failures`. The
    /// account is now locked, the session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::LockedAccount`].
    AccountLocked,
    /// `password_retry_count` reached `max_password_failures` for
    /// this session. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::ExcessivePasswordFails`].
    TooManyFailures,
    /// Credentials matched, but
    /// `session.allium:RejectLockedOrInsufficientAccess` (Slice 16)
    /// short-circuited the post-auth rule cluster: the user's
    /// account was already locked or below the minimum access tier.
    /// The session has moved to [`super::SessionState::LoggingOff`]
    /// with [`super::LogoffReason::LockedAccount`] or
    /// [`super::LogoffReason::NewUserRejected`].
    LogonRejected,
}

/// Outcome of [`super::Session::auto_rejoin_conference`]
/// (`conferences.allium:JoinConference` for `auto_rejoin`,
/// Slice 30; bulletin suppression added in Slice 31; name-type
/// promotion added in Slice 34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRejoinOutcome {
    /// The user has been attached to a conference. The session
    /// carries a fresh `ConferenceVisit`; the bound user's
    /// `last_joined` mirrors `(conference_number, msgbase_number)`.
    Joined {
        /// 1-indexed number of the conference the session is now
        /// attached to.
        conference_number: u32,
        /// 1-indexed number of the message base within that
        /// conference.
        msgbase_number: u32,
        /// Whether the listener should render the conference
        /// bulletin after the join (spec:
        /// `conferences.allium:ShowConferenceBulletin`,
        /// Slice 31). `false` whenever
        /// [`super::Session::quick_logon`] is set or a
        /// `ConferenceScan` is in progress (Slice 33).
        show_bulletin: bool,
        /// `Some(new_type)` when the join changed the session's
        /// `display_name_type` (spec:
        /// `conferences.allium:JoinedConferenceForNameType`,
        /// Slice 34). The listener renders `SCREEN_REALNAMES` /
        /// `SCREEN_INTERNETNAMES` accordingly. `None` when the
        /// conference's `accepted_name_type` matched what the
        /// session was already using.
        name_type_promoted_to: Option<crate::domain::conference::NameType>,
    },
    /// The user has no granted membership in any catalogued
    /// conference. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NoConferenceAccess`].
    NoAccess,
}

/// Outcome of [`super::Session::explicit_join_conference`]
/// (`conferences.allium:JoinConference` for `explicit_join`,
/// Slice 32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitJoinOutcome {
    /// The user has been attached to a conference. The session
    /// carries a fresh `ConferenceVisit`; the bound user's
    /// `last_joined` mirrors `(conference_number, msgbase_number)`.
    Joined {
        /// 1-indexed number of the conference the session is now
        /// attached to.
        conference_number: u32,
        /// 1-indexed number of the message base within that
        /// conference.
        msgbase_number: u32,
        /// Whether the listener should render the conference
        /// bulletin after the join (spec:
        /// `conferences.allium:ShowConferenceBulletin`,
        /// Slice 31). `false` whenever
        /// [`super::Session::quick_logon`] is set.
        show_bulletin: bool,
        /// `true` when the resolved conference is the one the user
        /// asked for; `false` when the resolver fell through to
        /// `first_accessible_conference` (e.g. user typed `J 7`
        /// without access to 7). The listener uses this to render
        /// the legacy "You do not have access to the requested
        /// conference" notice (`amiexpress/express.e:25157`)
        /// before the JOIN / JOINED screens.
        matched_request: bool,
        /// Mirrors [`AutoRejoinOutcome::Joined::name_type_promoted_to`]
        /// for explicit joins (Slice 34).
        name_type_promoted_to: Option<crate::domain::conference::NameType>,
    },
    /// The user has no granted membership in any catalogued
    /// conference. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NoConferenceAccess`].
    NoAccess,
}

/// Outcome of [`super::Session::start_conference_scan`] and
/// [`super::Session::step_conference_scan`]
/// (`conferences.allium:StartConferenceScan` /
/// `StepConferenceScan` / `FinishConferenceScan`, Slice 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConferenceScanOutcome {
    /// The scan attached the session to a conference. The listener
    /// renders any per-conference scan output (mail-scan summary
    /// hooks land in Slice 41) and either calls
    /// [`super::Session::step_conference_scan`] to continue or lets
    /// the user terminate the scan.
    Stepped {
        /// 1-indexed number of the conference the session is now
        /// attached to.
        conference_number: u32,
        /// 1-indexed number of the message base within that
        /// conference.
        msgbase_number: u32,
        /// Mirrors [`AutoRejoinOutcome::Joined::name_type_promoted_to`]
        /// for scan-step joins (Slice 34).
        name_type_promoted_to: Option<crate::domain::conference::NameType>,
    },
    /// The scan has walked past the last accessible conference.
    /// `in_progress` is now `false` and the session is left
    /// attached to its `User.last_joined` per the spec.
    Finished {
        /// Conference number the session settled on at the end of
        /// the scan, if any. `None` only on a session whose user
        /// somehow has no `last_joined` after stepping (defensive).
        rejoined_conference: Option<u32>,
    },
    /// The user has no granted membership in any catalogued
    /// conference. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::NoConferenceAccess`].
    NoAccess,
}

/// Outcome of [`super::Session::tick_minute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickMinuteOutcome {
    /// The session has time left and remains in `onboarded` or `menu`.
    Continued,
    /// `time_remaining` has reached zero. The session has moved to
    /// [`super::SessionState::LoggingOff`] with
    /// [`super::LogoffReason::OutOfTime`].
    TimeExpired,
}
