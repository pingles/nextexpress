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
//! Wrappers and their constructors live in [`crate::app`] (this
//! module) so the domain entity stays untouched. Constructors are
//! `pub(in crate::app)` — only flow code may build them. Read-only
//! accessors are `pub(crate)` so the driver can render prompts and
//! logs without unwrapping the inner session.
//!
//! ## Cross-phase operations
//! [`ActivePhase`] enum collects every wrapper from which idle-timeout
//! or carrier-loss may fire (`Identifying`, `Authenticating`,
//! `NewUserRegistering`, `Onboarded`, `Menu`). Both transitions
//! consume the enum and return [`LoggingOffSession`], so the wrong
//! handle for these too becomes unrepresentable.

use std::time::SystemTime;

use crate::domain::session::{AcceptConnectionError, LogonChannel, Session, SessionState};
use crate::domain::user::User;

/// Build a wrapper from a raw session, asserting (in debug builds) that
/// the underlying state matches the expected phase.
///
/// `from_session` and `into_inner` are uniformly `pub(in crate::app)`
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
            /// session. Visible only inside the application layer; flow
            /// functions are the only callers.
            #[must_use]
            pub(in crate::app) fn from_session(session: Session) -> Self {
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
            /// the application layer; flow functions consume wrappers
            /// and rebuild them on transition.
            #[must_use]
            pub(in crate::app) fn into_inner(self) -> Session {
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
/// has succeeded; the on-logon screens haven't run yet. There's no
/// reading step in this phase (the driver moves through it directly
/// into `Menu`), so it doesn't appear in [`ActivePhase`] and doesn't
/// expose `record_input`.
pub(crate) struct OnboardedSession {
    session: Session,
}

impl_constructor!(OnboardedSession, Onboarded);

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
/// `Onboarded` is excluded because the driver passes through it
/// without reading; on-logon screen scripting will revisit this when
/// it lands.
pub(crate) enum ActivePhase {
    /// Wrapped [`IdentifyingSession`].
    Identifying(IdentifyingSession),
    /// Wrapped [`AuthenticatingSession`].
    Authenticating(AuthenticatingSession),
    /// Wrapped [`NewUserRegisteringSession`].
    NewUserRegistering(NewUserRegisteringSession),
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
            Self::Menu(s) => s.session,
        }
    }
}

/// Outcome of [`crate::app::session_flow::name_typed`] expressed as
/// next-phase ownership.
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

/// Outcome of [`crate::app::session_flow::verify_password`] expressed
/// as next-phase ownership.
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

/// Outcome of [`crate::app::session_flow::NewUserRegistrationFlow::complete_typed`]
/// expressed as next-phase ownership. The post-onboarded rule cluster
/// may move the session to [`SessionState::LoggingOff`] on a fresh
/// registration whose ratio / access tier triggers
/// `RejectLockedOrInsufficientAccess`.
pub(crate) enum NewUserRegistrationResult {
    /// The new user was created and the post-onboarded cluster ran
    /// clean.
    Onboarded(OnboardedSession),
    /// `RejectLockedOrInsufficientAccess` short-circuited the
    /// post-onboarded cluster.
    LoggingOff(LoggingOffSession),
}

/// Outcome of [`crate::app::session_flow::verify_new_user_password`]
/// expressed as next-phase ownership.
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
