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

use crate::domain::conference::{
    AllScanScope, AllowedAddressing, Conference, MessageBaseRef, NameType,
};
use crate::domain::messaging::delete_mail::{delete_mail as delete_mail_rule, DeleteMailError};
use crate::domain::messaging::edit_mail_header::{
    edit_mail_header as edit_mail_header_rule, EditMailHeaderError,
};
use crate::domain::messaging::forward_mail::{
    forward_mail as forward_mail_rule, ForwardMailError, ForwardMailRequest,
};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::mail_store::MailStore;
use crate::domain::messaging::move_mail::{move_mail as move_mail_rule, MoveMailError};
use crate::domain::messaging::post_comment_to_sysop::{
    post_comment_to_sysop as post_comment_to_sysop_rule, CommentToSysopDraft,
};
use crate::domain::messaging::post_mail::{
    post_mail as post_mail_rule, PostMailDraft, PostMailError,
};
use crate::domain::messaging::read_mail::{read_mail as read_mail_rule, ReadMailError};
use crate::domain::messaging::reply_to_mail::{
    reply_to_mail as reply_to_mail_rule, ReplyToMailDraft, ReplyToMailError,
};
use crate::domain::messaging::scan_mail::{scan_mail as scan_mail_rule, ScanMailError, ScanResult};
use crate::domain::session::{
    AcceptConnectionError, AutoRejoinOutcome, ExplicitJoinOutcome, LogonChannel, Session,
    SessionState,
};
use crate::domain::user::User;

/// Trait shared by [`OnboardedSession`] and [`MenuSession`] for the
/// auto-scan-on-join helper. Both phases can launch a mail scan
/// (`messaging.allium:ScanMail`'s `requires: session.state in
/// {onboarded, menu}`); the auto-rejoin path runs in `Onboarded`
/// and the explicit-join path in `Menu`.
pub(crate) trait ScanOnJoin {
    /// Returns the `(conference_number, msgbase_number)` pair for
    /// the session's open visit, or `None` when none is open.
    fn current_msgbase(&self) -> Option<(u32, u32)>;

    /// Applies `messaging.allium:ScanMail` to the bound user.
    fn scan_mail(
        &mut self,
        store: &dyn MailStore,
        msgbase: MessageBaseRef,
        scope: AllScanScope,
        from_message: u32,
        now: SystemTime,
    ) -> Result<ScanResult, ScanMailError>;
}

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
/// has succeeded; the on-logon screens haven't run yet. There's no
/// reading step in this phase (the driver moves through it directly
/// into `Menu`), so it doesn't appear in [`ActivePhase`] and doesn't
/// expose `record_input`.
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
}

impl ScanOnJoin for OnboardedSession {
    fn current_msgbase(&self) -> Option<(u32, u32)> {
        let visit = self.session.current_visit()?;
        Some((visit.conference_number(), visit.msgbase_number()))
    }

    fn scan_mail(
        &mut self,
        store: &dyn MailStore,
        msgbase: MessageBaseRef,
        scope: AllScanScope,
        from_message: u32,
        now: SystemTime,
    ) -> Result<ScanResult, ScanMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Onboarded session has a bound user");
        scan_mail_rule(user, store, msgbase, scope, from_message, now)
    }
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

    /// Toggles the session's quiet-mode flag and returns the new
    /// value. Implements the `Q` menu command's mutation step
    /// (Tier A quickwin A9, `amiexpress/express.e:25506`'s
    /// `quietFlag := Not(quietFlag)`).
    pub(crate) fn toggle_quiet_mode(&mut self) -> bool {
        let new_value = !self.session.quiet_mode();
        self.session.set_quiet_mode(new_value);
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
    /// `conferences.allium:JoinConference` (Slice 32) for a `J` /
    /// `J <num>` command typed at the menu.
    #[must_use]
    pub(crate) fn explicit_join_conference(
        mut self,
        target_conference_number: u32,
        conferences: &[Conference],
        now: SystemTime,
    ) -> ExplicitJoinTransition {
        let outcome = self
            .session
            .explicit_join_conference(target_conference_number, conferences, now)
            .expect("Menu -> explicit_join is total per spec requires-clause");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                msgbase_number,
                show_bulletin,
                matched_request,
                name_type_promoted_to,
            } => ExplicitJoinTransition::Joined {
                session: Self {
                    session: self.session,
                },
                conference_number,
                msgbase_number,
                show_bulletin,
                matched_request,
                name_type_promoted_to,
            },
            ExplicitJoinOutcome::NoAccess => ExplicitJoinTransition::NoAccess(LoggingOffSession {
                session: self.session,
            }),
        }
    }

    /// Wraps `self` in the [`ActivePhase`] enum.
    #[must_use]
    pub(crate) fn into_active(self) -> ActivePhase {
        ActivePhase::Menu(self)
    }

    /// Applies `messaging.allium:ReadMail` (Slice 39) to `mail` at
    /// `now`, mutating both the bound user's read pointers and the
    /// mail's `received_at` per the spec's `ensures` block.
    ///
    /// The caller is responsible for persisting the mutated `mail`
    /// back to its [`crate::domain::messaging::mail_store::MailStore`]; the bound
    /// user is flushed at logoff by the session flow.
    ///
    /// # Errors
    /// Returns the matching [`ReadMailError`] variant when the rule
    /// rejects the request.
    pub(crate) fn read_mail(
        &mut self,
        mail: &mut Mail,
        now: SystemTime,
    ) -> Result<(), ReadMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        read_mail_rule(user, mail, now)
    }

    /// Applies `messaging.allium:PostMail` (Slice 42, single-addressee
    /// path) to the bound user, persisting the new mail via `store`
    /// and bumping the user's and per-conference `messages_posted`
    /// counters.
    ///
    /// # Errors
    /// Returns the matching [`PostMailError`] variant when the rule
    /// rejects the request.
    pub(crate) fn post_mail(
        &mut self,
        msgbase: MessageBaseRef,
        allowed_addressing: AllowedAddressing,
        store: &mut dyn MailStore,
        draft: PostMailDraft,
    ) -> Result<Mail, PostMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        post_mail_rule(user, msgbase, allowed_addressing, store, draft)
    }

    /// Applies `messaging.allium:PostCommentToSysop` (Slice 44) to the
    /// bound user, persisting a private message addressed to the sysop
    /// via `store` and bumping the user's and per-conference
    /// `messages_posted` counters.
    ///
    /// # Errors
    /// Returns the matching [`PostMailError`] variant when the rule
    /// rejects the request.
    pub(crate) fn post_comment_to_sysop(
        &mut self,
        msgbase: MessageBaseRef,
        allowed_addressing: AllowedAddressing,
        store: &mut dyn MailStore,
        draft: CommentToSysopDraft,
    ) -> Result<Mail, PostMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        post_comment_to_sysop_rule(user, msgbase, allowed_addressing, store, draft)
    }

    /// Applies `messaging.allium:ReplyToMail` (Slice 45) to the bound
    /// user. The caller has already loaded `source` from the store
    /// and the typed session guarantees we're at `state = menu`.
    ///
    /// # Errors
    /// Returns the matching [`ReplyToMailError`] variant when the
    /// rule rejects the request.
    pub(crate) fn reply_to_mail(
        &mut self,
        msgbase: MessageBaseRef,
        allowed_addressing: AllowedAddressing,
        store: &mut dyn MailStore,
        source: &Mail,
        draft: ReplyToMailDraft,
    ) -> Result<Mail, ReplyToMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        reply_to_mail_rule(user, msgbase, allowed_addressing, store, source, draft)
    }

    /// Applies `messaging.allium:ForwardMail` (Slice 46) to the bound
    /// user.
    ///
    /// # Errors
    /// Returns the matching [`ForwardMailError`] variant when the
    /// rule rejects the request.
    pub(crate) fn forward_mail(
        &mut self,
        msgbase: MessageBaseRef,
        allowed_addressing: AllowedAddressing,
        store: &mut dyn MailStore,
        source: &Mail,
        request: ForwardMailRequest,
    ) -> Result<Mail, ForwardMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        forward_mail_rule(user, msgbase, allowed_addressing, store, source, request)
    }

    /// Applies `messaging.allium:DeleteMail` (Slice 49) to the bound
    /// user.
    ///
    /// # Errors
    /// Returns the matching [`DeleteMailError`] variant.
    pub(crate) fn delete_mail(
        &mut self,
        store: &mut dyn MailStore,
        number: u32,
    ) -> Result<(), DeleteMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        delete_mail_rule(user, store, number)
    }

    /// Applies `messaging.allium:MoveMail` (Slice 49) to the bound
    /// user. The caller passes both stores.
    ///
    /// # Errors
    /// Returns the matching [`MoveMailError`] variant.
    pub(crate) fn move_mail(
        &mut self,
        source: &mut dyn MailStore,
        target: &mut dyn MailStore,
        number: u32,
    ) -> Result<Mail, MoveMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        move_mail_rule(user, source, target, number)
    }

    /// Applies `messaging.allium:EditMailHeader` (Slice 49) to the
    /// bound user.
    ///
    /// # Errors
    /// Returns the matching [`EditMailHeaderError`] variant.
    pub(crate) fn edit_mail_header(
        &mut self,
        store: &mut dyn MailStore,
        mail_number: u32,
        new_subject: Option<String>,
        new_to: Option<(String, Option<u32>)>,
    ) -> Result<(), EditMailHeaderError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        edit_mail_header_rule(user, store, mail_number, new_subject, new_to)
    }
}

impl ScanOnJoin for MenuSession {
    fn current_msgbase(&self) -> Option<(u32, u32)> {
        let visit = self.session.current_visit()?;
        Some((visit.conference_number(), visit.msgbase_number()))
    }

    fn scan_mail(
        &mut self,
        store: &dyn MailStore,
        msgbase: MessageBaseRef,
        scope: AllScanScope,
        from_message: u32,
        now: SystemTime,
    ) -> Result<ScanResult, ScanMailError> {
        let user = self
            .session
            .phase
            .user_mut()
            .expect("Menu session has a bound user");
        scan_mail_rule(user, store, msgbase, scope, from_message, now)
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
/// next-phase ownership (Slice 32). The session stays in
/// [`SessionState::Menu`] on success.
///
/// `conference_number`, `msgbase_number` and `show_bulletin` are
/// reserved for forthcoming slices that surface the joined
/// coordinates (e.g. visit-history listings) and the bulletin
/// rendering driver-side; the present driver only needs
/// `matched_request` and `name_type_promoted_to`.
#[allow(dead_code)]
pub(crate) enum ExplicitJoinTransition {
    /// The session reattached to a conference. `matched_request` is
    /// `false` when the resolver fell through to
    /// `first_accessible_conference` because the requested
    /// conference wasn't accessible to the user.
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
        /// `true` when the resolved conference matches what the user
        /// asked for; `false` when the resolver fell through.
        matched_request: bool,
        /// Mirrors [`AutoRejoinTransition::Joined::name_type_promoted_to`].
        name_type_promoted_to: Option<NameType>,
    },
    /// The user has no granted membership anywhere; the session has
    /// moved to [`SessionState::LoggingOff`] with
    /// [`crate::domain::session::LogoffReason::NoConferenceAccess`].
    NoAccess(LoggingOffSession),
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
