//! Phase-typed wrappers around [`Session`].
//!
//! The pure [`Session`] entity tracks its phase via an internal
//! `SessionPhase` enum and rejects out-of-phase operations with
//! `WrongState*` errors. The driver, by virtue of its control flow,
//! always knows which phase the session is in — but without a typed
//! API it has to assert that knowledge with `.expect("session is in
//! X")` after every call. That re-encodes the state machine outside
//! the domain and turns mismatches into runtime panics.
//!
//! These wrappers move that knowledge into the type system. Each
//! wrapper owns a [`Session`] by value and exposes only the operations
//! valid for that phase. Transitions consume `self` and yield the
//! next-phase wrapper, so the wrong handle becomes unrepresentable.
//!
//! ## Layering
//! Wrappers and their constructors live beside [`Session`] so this
//! module is the single typed entry point for session state
//! transitions. Constructors are crate-visible for app-layer flow
//! code; raw transition helpers on [`Session`] remain private where a
//! typed wrapper covers them.
//!
//! ## Cross-phase operations
//! [`ActivePhase`] enum collects every wrapper from which idle-timeout
//! or carrier-loss may fire (`Identifying`, `Authenticating`,
//! `NewUserRegistering`, `Onboarded`, `Menu`). Both transitions
//! consume the enum and return [`LoggingOffSession`], so the wrong
//! handle for these too becomes unrepresentable.

use std::time::SystemTime;

use crate::domain::conference::{Conference, NameType};
use crate::domain::files::flagged::FlaggedFiles;
use crate::domain::session::{
    AcceptConnectionError, AutoRejoinOutcome, ExplicitJoinOutcome, LogonChannel, Session,
    SessionState,
};
use crate::domain::user::User;

/// Build a wrapper from a raw session, asserting (in debug builds) that
/// the underlying state matches the expected phase.
///
/// `from_session` and `into_inner` are uniformly `pub(crate)`
/// so any phase wrapper can be reconstructed by flow code, even if
/// the current driver doesn't exercise that round-trip for every
/// phase yet. `#[allow(dead_code)]` on the impl block suppresses the
/// "never used" warnings for phases where the driver doesn't yet
/// round-trip (e.g. `ConnectingSession::into_inner`,
/// `EndedSession::into_inner`).
macro_rules! impl_constructor {
    ($wrapper:ident, $state:ident) => {
        #[allow(dead_code)]
        impl $wrapper {
            /// Constructs a phase wrapper from an already-transitioned
            /// session. Visible only inside this crate; flow
            /// functions are the only callers.
            #[must_use]
            pub(crate) fn from_session(session: Session) -> Self {
                debug_assert_eq!(
                    session.state(),
                    SessionState::$state,
                    concat!(
                        stringify!($wrapper),
                        " constructed from session not in ",
                        stringify!($state),
                        " state",
                    ),
                );
                Self { session }
            }

            /// Returns the inner session by value. Visible only inside
            /// this crate; flow functions consume wrappers
            /// and rebuild them on transition.
            #[must_use]
            pub(crate) fn into_inner(self) -> Session {
                self.session
            }
        }
    };
}

/// Implements [`record_input`] for an active-phase wrapper.
macro_rules! impl_record_input {
    ($wrapper:ident) => {
        impl $wrapper {
            /// Records a successful read against the inner session's
            /// idle clock. Safe from any active phase — the underlying
            /// `Session::record_input` is unconditional.
            pub(crate) fn record_input(&mut self, at: SystemTime) {
                self.session.record_input(at);
            }
        }
    };
}

/// Wraps a freshly accepted [`Session`] in [`SessionState::Connecting`].
pub(crate) struct ConnectingSession {
    session: Session,
}

impl ConnectingSession {
    /// Accepts a new connection and constructs the wrapper.
    ///
    /// # Errors
    /// Returns [`AcceptConnectionError`] when the underlying domain
    /// constructor rejects the call (the only path is
    /// `AlreadyActiveSession`, but that's surfaced to the listener
    /// rather than being a panic).
    pub(crate) fn accept(
        node_number: u32,
        channel: LogonChannel,
        online_baud: u32,
        connected_at: SystemTime,
    ) -> Result<Self, AcceptConnectionError> {
        let session =
            Session::accept_connection(node_number, channel, online_baud, connected_at, None)?;
        Ok(Self { session })
    }

    /// Moves the session into [`SessionState::Identifying`]. The
    /// underlying transition is total from `Connecting`, so this can't
    /// fail at runtime.
    #[must_use]
    pub(crate) fn prompt_for_name(mut self) -> IdentifyingSession {
        self.session
            .prompt_for_name()
            .expect("Connecting -> Identifying is total");
        IdentifyingSession {
            session: self.session,
        }
    }
}

impl_constructor!(ConnectingSession, Connecting);

/// Wraps a [`Session`] in [`SessionState::Identifying`].
pub(crate) struct IdentifyingSession {
    session: Session,
}

impl IdentifyingSession {
    /// Wraps `self` in the [`ActivePhase`] enum so cross-phase
    /// operations (idle timeout, carrier loss) can dispatch.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::Identifying(self)
    }
}

impl_constructor!(IdentifyingSession, Identifying);
impl_record_input!(IdentifyingSession);

/// Wraps a [`Session`] in [`SessionState::Authenticating`].
pub(crate) struct AuthenticatingSession {
    session: Session,
}

impl AuthenticatingSession {
    /// Wraps `self` in the [`ActivePhase`] enum.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::Authenticating(self)
    }
}

impl_constructor!(AuthenticatingSession, Authenticating);
impl_record_input!(AuthenticatingSession);

/// Wraps a [`Session`] in [`SessionState::NewUserRegistering`].
pub(crate) struct NewUserRegisteringSession {
    session: Session,
}

impl NewUserRegisteringSession {
    /// Wraps `self` in the [`ActivePhase`] enum.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::NewUserRegistering(self)
    }
}

impl_constructor!(NewUserRegisteringSession, NewUserRegistering);
impl_record_input!(NewUserRegisteringSession);

/// Wraps a [`Session`] in [`SessionState::Onboarded`]. Authentication
/// has succeeded; the on-logon screens haven't run yet. The normal
/// path moves through it directly into `Menu`, but forced password
/// reset reads a new password while still onboarded.
pub(crate) struct OnboardedSession {
    session: Session,
}

impl OnboardedSession {
    /// Resolves the auto-rejoin path of
    /// `conferences.allium:JoinConference` (Slice 30) and reports
    /// the outcome back as a typed transition. Mirrors the spec's
    /// `requires: session.state in {onboarded, menu}` precondition
    /// — total from this phase.
    #[must_use]
    pub(crate) fn auto_rejoin_conference(
        mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> AutoRejoinTransition {
        let outcome = self
            .session
            .auto_rejoin_conference(conferences, now)
            .expect("Onboarded -> auto_rejoin is total per spec requires-clause");
        match outcome {
            AutoRejoinOutcome::Joined {
                conference_number,
                msgbase_number,
                show_bulletin,
                name_type_promoted_to,
            } => AutoRejoinTransition::Joined {
                session: Self {
                    session: self.session,
                },
                conference_number,
                msgbase_number,
                show_bulletin,
                name_type_promoted_to,
            },
            AutoRejoinOutcome::NoAccess => AutoRejoinTransition::NoAccess(LoggingOffSession {
                session: self.session,
            }),
        }
    }

    /// Returns the bound user. Always `Some` in this phase.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn user(&self) -> &User {
        self.session
            .user()
            .expect("Onboarded phase always has a bound user")
    }

    /// Wraps `self` in the [`ActivePhase`] enum.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::Onboarded(self)
    }
}

impl_constructor!(OnboardedSession, Onboarded);
impl_record_input!(OnboardedSession);

/// Wraps a [`Session`] in [`SessionState::Menu`]. The user is at the
/// conference command prompt.
pub(crate) struct MenuSession {
    session: Session,
}

impl MenuSession {
    /// Returns the bound user. Always `Some` in this phase.
    #[must_use]
    pub(crate) fn user(&self) -> &User {
        self.session
            .user()
            .expect("Menu phase always has a bound user")
    }

    /// Returns a mutable reference to the bound user. The menu use
    /// cases hand this directly to the `domain::messaging` rules,
    /// which take `&mut User` rather than routing through
    /// `MenuSession` methods — the session typing stays focused on
    /// the state machine rather than acting as a command registry.
    pub(crate) fn user_mut(&mut self) -> &mut User {
        self.session
            .phase
            .user_mut()
            .expect("Menu phase always has a bound user")
    }

    /// Returns the conference number the session is currently
    /// attached to (Slice 34a). Always `Some` after a successful
    /// auto-rejoin; `None` for the (defensive) case where a Menu
    /// session reaches this method without an open visit, in which
    /// case the listener can render the system-wide menu.
    #[must_use]
    pub(crate) fn current_conference_number(&self) -> Option<u32> {
        self.session
            .current_visit()
            .map(crate::domain::conference_visit::ConferenceVisit::conference_number)
    }

    /// Returns the `(conference_number, msgbase_number)` pair for
    /// the session's open visit (Slice 39). Used by the `R` / `M` /
    /// `N` menu commands to locate the active message base.
    #[must_use]
    pub(crate) fn current_msgbase(&self) -> Option<(u32, u32)> {
        let visit = self.session.current_visit()?;
        Some((visit.conference_number(), visit.msgbase_number()))
    }

    /// Returns the per-call time the session has left (Slice 14),
    /// read by the menu prompt's `(<n> mins. left)` display
    /// (Tier A quickwin A4).
    #[must_use]
    pub(crate) fn time_remaining(&self) -> std::time::Duration {
        self.session.time_remaining()
    }

    /// Whether this session is a quick logon (spec
    /// `session.allium:Session.quick_logon`). The logon conference scan
    /// is skipped for quick logons, mirroring the legacy `confScan`
    /// gate.
    #[must_use]
    pub(crate) fn quick_logon(&self) -> bool {
        self.session.quick_logon()
    }

    /// Points the open visit at `(conference_number, msgbase_number)`
    /// so the read flow targets that base. The `MS` read-it-now path
    /// attaches the base it found mail in, runs the read sub-prompt,
    /// then re-attaches the caller's home coordinate — the legacy
    /// transient `currentConf:=cn ... :=oldcn`
    /// (`amiexpress/express.e:11750-11758`).
    pub(crate) fn attach_read_visit(
        &mut self,
        conference_number: u32,
        msgbase_number: u32,
        now: SystemTime,
    ) {
        self.session
            .attach_visit(conference_number, msgbase_number, now);
    }

    /// Toggles the session's quiet-mode flag and returns the new
    /// value. Implements the `Q` menu command's mutation step
    /// (Tier A quickwin A9, `amiexpress/express.e:25506`'s
    /// `quietFlag := Not(quietFlag)`).
    pub(crate) fn toggle_quiet_mode(&mut self) -> bool {
        let new_value = !self.session.quiet_mode();
        self.session.set_quiet_mode(new_value);
        new_value
    }

    /// The session's flagged-file set — the lister reborrows it
    /// immutably to mark flagged rows, and the `F`/`R` pager verbs
    /// flag listed files into it (slice D2f).
    pub(crate) fn flagged_files_mut(&mut self) -> &mut FlaggedFiles {
        self.session.flagged_files_mut()
    }

    /// Toggles the bound user's expert-mode flag and returns the new
    /// value. Implements the `X` menu command's mutation step
    /// (Tier A quickwin A6, `amiexpress/express.e:26114`'s
    /// `expert := IF expert="X" THEN "N" ELSE "X"`). The flip is
    /// persisted with the user record when the session logs off.
    pub(crate) fn toggle_expert_mode(&mut self) -> bool {
        let new_value = !self.user().expert_mode();
        self.user_mut().set_expert_mode(new_value);
        new_value
    }

    /// `session.allium:UserRequestsLogoff` from the menu — the only
    /// total transition out of `Menu` the driver invokes. Consumes
    /// `self` and yields a [`LoggingOffSession`].
    #[must_use]
    pub(crate) fn user_requests_logoff(mut self) -> LoggingOffSession {
        self.session
            .user_requests_logoff()
            .expect("Menu -> LoggingOff is total via UserRequestsLogoff");
        LoggingOffSession {
            session: self.session,
        }
    }

    /// Resolves the explicit-join path of
    /// `conferences.allium:JoinConference` (Slice 32 / Tier C C2 /
    /// C4a) for a `J <num>` — or message-base-targeted `J <a>.<b>` /
    /// `JM <n>`, where `requested_msgbase_number` is `Some(_)` —
    /// command typed at the menu (`None` lands on the conference's
    /// primary base; an unknown base defensively resets to it,
    /// `amiexpress/express.e:4995`). A denied request returns the
    /// session unchanged — explicit join never logs the user off
    /// (`amiexpress/express.e:25156-25158`).
    #[must_use]
    pub(crate) fn explicit_join_conference(
        mut self,
        target_conference_number: u32,
        requested_msgbase_number: Option<u32>,
        conferences: &[Conference],
        now: SystemTime,
    ) -> ExplicitJoinTransition {
        let outcome = self
            .session
            .explicit_join_conference(
                target_conference_number,
                requested_msgbase_number,
                conferences,
                now,
            )
            .expect("Menu -> explicit_join is total per spec requires-clause");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                msgbase_number,
                show_bulletin,
                name_type_promoted_to,
            } => ExplicitJoinTransition::Joined {
                session: Self {
                    session: self.session,
                },
                conference_number,
                msgbase_number,
                show_bulletin,
                name_type_promoted_to,
            },
            ExplicitJoinOutcome::Denied => ExplicitJoinTransition::Denied(Self {
                session: self.session,
            }),
        }
    }

    /// Wraps `self` in the [`ActivePhase`] enum.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::Menu(self)
    }
}

impl_constructor!(MenuSession, Menu);
impl_record_input!(MenuSession);

/// Wraps a [`Session`] in [`SessionState::LoggingOff`]. Ready for
/// `finalise_logoff`.
pub(crate) struct LoggingOffSession {
    session: Session,
}

impl_constructor!(LoggingOffSession, LoggingOff);

/// Wraps a [`Session`] in [`SessionState::Ended`]. Terminal phase; no
/// further transitions are valid.
pub(crate) struct EndedSession {
    #[allow(dead_code)] // domain-side data lives on the session
    session: Session,
}

impl_constructor!(EndedSession, Ended);

/// Any phase from which an idle timeout or carrier loss may fire and
/// from which the driver actually reads input. Collecting these into
/// one enum lets the driver hand off whichever active wrapper it
/// currently owns and get back a [`LoggingOffSession`] without
/// `match`ing inline at every read site.
///
pub(crate) enum ActivePhase {
    /// Wrapped [`IdentifyingSession`].
    Identifying(IdentifyingSession),
    /// Wrapped [`AuthenticatingSession`].
    Authenticating(AuthenticatingSession),
    /// Wrapped [`NewUserRegisteringSession`].
    NewUserRegistering(NewUserRegisteringSession),
    /// Wrapped [`OnboardedSession`].
    Onboarded(OnboardedSession),
    /// Wrapped [`MenuSession`].
    Menu(MenuSession),
}

impl ActivePhase {
    /// Applies `session.allium:CarrierLost`. Total from every active
    /// phase per the spec's transition table.
    #[must_use]
    pub(crate) fn apply_carrier_loss(self) -> LoggingOffSession {
        let mut session = self.into_inner();
        session
            .apply_carrier_loss()
            .expect("ActivePhase guarantees carrier-loss is permitted");
        LoggingOffSession { session }
    }

    /// Applies `session.allium:IdleTimeout`. Total from every active
    /// phase per the spec's transition table.
    #[must_use]
    pub(crate) fn apply_idle_timeout(self, treat_as_logoff: bool) -> LoggingOffSession {
        let mut session = self.into_inner();
        session
            .apply_idle_timeout(treat_as_logoff)
            .expect("ActivePhase guarantees idle-timeout is permitted");
        LoggingOffSession { session }
    }

    fn into_inner(self) -> Session {
        match self {
            Self::Identifying(s) => s.session,
            Self::Authenticating(s) => s.session,
            Self::NewUserRegistering(s) => s.session,
            Self::Onboarded(s) => s.session,
            Self::Menu(s) => s.session,
        }
    }
}

/// Outcome of [`OnboardedSession::auto_rejoin_conference`] expressed
/// as next-phase ownership (Slice 30 / Slice 31 bulletin / Slice 34
/// name-type).
///
/// Some fields are not yet read by the driver (Slice 34a wires only
/// the JOINED-screen and name-type-promotion paths) but are part of
/// the contract — kept on the enum so future slices can pin them
/// down without changing the type.
#[allow(dead_code)]
pub(crate) enum AutoRejoinTransition {
    /// The session is attached to a conference and may proceed into
    /// the menu.
    Joined {
        /// The session, still in [`SessionState::Onboarded`].
        session: OnboardedSession,
        /// 1-indexed number of the conference attached to.
        conference_number: u32,
        /// 1-indexed number of the message base within that
        /// conference.
        msgbase_number: u32,
        /// Whether the listener should render the conference
        /// bulletin after the join (suppressed under
        /// `quick_logon` or scan-in-progress).
        show_bulletin: bool,
        /// `Some(new_type)` when the join changed the session's
        /// `display_name_type`; the listener renders
        /// `SCREEN_REALNAMES` / `SCREEN_INTERNETNAMES` accordingly.
        name_type_promoted_to: Option<NameType>,
    },
    /// The user has no granted membership for any conference; the
    /// session has moved to [`SessionState::LoggingOff`] with
    /// [`crate::domain::session::LogoffReason::NoConferenceAccess`].
    NoAccess(LoggingOffSession),
}

/// Outcome of [`MenuSession::explicit_join_conference`] expressed as
/// next-phase ownership (Slice 32 / Tier C C2). The session stays in
/// [`SessionState::Menu`] on both arms — explicit join never logs
/// the user off.
///
/// `show_bulletin` is reserved for the forthcoming slice that
/// renders conference bulletins driver-side; the present driver
/// needs the joined coordinates and `name_type_promoted_to`.
#[allow(dead_code)]
pub(crate) enum ExplicitJoinTransition {
    /// The session reattached to the requested conference.
    Joined {
        /// The session, still in [`SessionState::Menu`].
        session: MenuSession,
        /// 1-indexed number of the conference attached to.
        conference_number: u32,
        /// 1-indexed number of the message base within that
        /// conference.
        msgbase_number: u32,
        /// Whether the listener should render the conference
        /// bulletin after the join.
        show_bulletin: bool,
        /// Mirrors [`AutoRejoinTransition::Joined::name_type_promoted_to`].
        name_type_promoted_to: Option<NameType>,
    },
    /// The requested conference is not accessible (legacy
    /// `checkConfAccess` failure,
    /// `amiexpress/express.e:25156-25158`). The session is returned
    /// unchanged; the caller writes the no-access notice and stays
    /// at the menu.
    Denied(MenuSession),
}

/// Outcome of the name-typed flow expressed as next-phase ownership.
pub(crate) enum NameTypedTransition {
    /// Handle resolved to a known user; collect the password next.
    Authenticated(AuthenticatingSession),
    /// Handle did not match; stay in [`SessionState::Identifying`] for
    /// a retry.
    Identifying(IdentifyingSession),
    /// User typed `NEW`; run the registration sub-flow.
    NewUserRegistering {
        /// The handle that resolved to a registration request.
        session: NewUserRegisteringSession,
        /// `true` when the new-user password gate must pass before
        /// completion.
        password_required: bool,
    },
    /// User typed `NEW` but the gate disallows registration; the
    /// session has moved to [`SessionState::LoggingOff`] with
    /// [`crate::domain::session::LogoffReason::NewUserRejected`].
    Disallowed(LoggingOffSession),
    /// Five not-found strikes in a row. Terminal.
    Ended(EndedSession),
}

/// Outcome of the password-verification flow expressed as next-phase
/// ownership.
pub(crate) enum VerifyPasswordTransition {
    /// Credentials matched; the post-onboarded cluster has run.
    Onboarded(OnboardedSession),
    /// Credentials did not match; stay in
    /// [`SessionState::Authenticating`] for a retry.
    Authenticating(AuthenticatingSession),
    /// The post-auth cluster rejected the logon (account lockout,
    /// excessive password failures, or
    /// `RejectLockedOrInsufficientAccess`); ready for
    /// `finalise_logoff`.
    LoggingOff {
        /// The session ready to finalise.
        session: LoggingOffSession,
        /// Why the logon was rejected; lets the driver pick a wire
        /// message without inspecting the session.
        reason: VerifyPasswordRejectionReason,
    },
}

/// Why [`VerifyPasswordTransition::LoggingOff`] fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifyPasswordRejectionReason {
    /// `user.invalid_attempts` reached the configured limit; the
    /// account is locked.
    AccountLocked,
    /// The per-session retry count reached the configured limit.
    TooManyFailures,
    /// `RejectLockedOrInsufficientAccess` short-circuited the
    /// post-onboarded cluster (Slice 16).
    LogonRejected,
}

/// Outcome of the new-user registration flow expressed as next-phase
/// ownership. The post-onboarded rule cluster may move the session to
/// [`SessionState::LoggingOff`] on a fresh registration whose ratio /
/// access tier triggers `RejectLockedOrInsufficientAccess`.
pub(crate) enum NewUserRegistrationResult {
    /// The new user was created and the post-onboarded cluster ran
    /// clean.
    Onboarded(OnboardedSession),
    /// `RejectLockedOrInsufficientAccess` short-circuited the
    /// post-onboarded cluster.
    LoggingOff(LoggingOffSession),
}

/// Outcome of the new-user password gate expressed as next-phase
/// ownership.
pub(crate) enum NewUserPasswordTransition {
    /// Gate match. Stay in [`SessionState::NewUserRegistering`] with
    /// `password_verified` set; the registration form follows.
    Verified(NewUserRegisteringSession),
    /// Gate mismatch; the attempt counter has been bumped. Stay in
    /// [`SessionState::NewUserRegistering`] for another try.
    Mismatch(NewUserRegisteringSession),
    /// Attempt counter reached the configured limit; the session is
    /// in [`SessionState::LoggingOff`] with
    /// [`crate::domain::session::LogoffReason::NewUserRejected`].
    TooManyFailures(LoggingOffSession),
}
