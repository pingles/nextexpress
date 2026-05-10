//! Transport-agnostic application driver for an interactive BBS session.
//!
//! Driving adapters provide a [`Terminal`] implementation (see
//! [`crate::app::terminal`]). The driver owns the BBS workflow:
//! accepting the session, prompting for login, optional new-user
//! registration, authentication, menu entry and logoff finalisation.
//!
//! Wire-format byte constants live in [`crate::app::wire_text`].
//!
//! ## Phase types
//! Each step of the workflow consumes and returns a phase wrapper from
//! [`crate::app::typed_session`]. The wrong handle for a given
//! transition becomes unrepresentable at compile time; the driver no
//! longer needs to assert "session is in X" after every call.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_flow::{
    self, NewUserProfile, NewUserRegistrationFlow, NEW_USER_REGISTRATION_LITERAL,
};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::typed_session::{
    AuthenticatingSession, AutoRejoinTransition, ConnectingSession, EndedSession,
    ExplicitJoinTransition, IdentifyingSession, LoggingOffSession, MenuSession,
    NameTypedTransition, NewUserPasswordTransition, NewUserRegisteringSession,
    NewUserRegistrationResult, OnboardedSession, VerifyPasswordRejectionReason,
    VerifyPasswordTransition,
};
use crate::app::wire_text::{
    ACCOUNT_LOCKED_LINE, ANSI_PROMPT, AUTHENTICATED_LINE, COPYRIGHT_LINES, EMAIL_PROMPT,
    GOODBYE_LINE, HANDLE_TAKEN_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_LINE_LENGTH_LINE, JOIN_REQUIRES_NUMBER_LINE, LINE_LENGTH_PROMPT, LOCATION_PROMPT,
    LOGON_REJECTED_LINE, MENU_PROMPT, NAME_PROMPT, NEW_USER_EXCESSIVE_FAILURES_LINE,
    NEW_USER_INVALID_PASSWORD_LINE, NEW_USER_PASSWORD_OK_LINE, NEW_USER_PASSWORD_PROMPT,
    NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, NO_CONFERENCE_ACCESS_LINE, PASSWORDS_DO_NOT_MATCH_LINE,
    PASSWORD_PROMPT, PHONE_PROMPT, REGISTRATION_COMPLETE_LINE, REGISTRATION_HANDLE_PROMPT,
    REGISTRATION_PASSWORD_CONFIRM_PROMPT, REGISTRATION_PASSWORD_PROMPT,
    REGISTRATION_RETRIES_EXHAUSTED_LINE, TOO_MANY_PASSWORD_FAILURES_LINE, TOO_MANY_RETRIES_LINE,
    UNKNOWN_COMMAND_LINE, UNKNOWN_USER_LINE, WRONG_PASSWORD_LINE,
};
use crate::domain::conference::NameType;
use crate::domain::session::LogonChannel;
use crate::domain::user_repository::NameLookupResult;

/// Maximum handle attempts during registration before the session
/// bails. Mirrors the original `AmiExpress` `doNewUser` retry budget at
/// `amiexpress/express.e:30150`.
const MAX_REGISTRATION_HANDLE_ATTEMPTS: u32 = 5;

/// App-layer session workflow over a terminal port.
///
/// The driver does not hold a [`crate::domain::session::Session`]
/// field; phase wrappers are stack-local as they thread through
/// [`Self::run`].
pub(crate) struct SessionDriver<T>
where
    T: Terminal,
{
    terminal: T,
    services: AppServices,
    node_number: u32,
    channel: LogonChannel,
}

/// Outcome of the sign-in chain (handle + password / registration).
/// Lets [`SessionDriver::run`] decide how to enter the menu vs. how to
/// finalise.
enum SignInResult {
    /// Sign-in produced an authenticated, fully-onboarded session.
    Onboarded(OnboardedSession),
    /// Sign-in moved the session into `LoggingOff` (rejection,
    /// timeout, carrier loss, exhausted retries, ...).
    LoggingOff(LoggingOffSession),
    /// Sign-in ended the session outright (handle retry budget
    /// exhausted moves straight to `Ended`).
    Ended(EndedSession),
}

/// Outcome of the password-verification loop. Lets the caller proceed
/// to the menu or skip straight to logoff finalisation.
enum AuthResult {
    /// Credentials matched and the post-auth cluster ran clean.
    Onboarded(OnboardedSession),
    /// Logon rejected (lockout, retries, post-auth gate). The driver
    /// has already written the wire message.
    LoggingOff(LoggingOffSession),
}

/// Outcome of [`SessionDriver::auto_rejoin`]. Mirrors the spec's
/// `JoinConference` two-branch consequent (resolved vs.
/// `no_conference_access`).
enum AutoRejoinResult {
    /// The session attached to a conference and may proceed into the
    /// menu loop.
    Joined(OnboardedSession),
    /// The user has no granted membership; the session has moved to
    /// `LoggingOff` with `LogoffReason::NoConferenceAccess`.
    NoAccess(LoggingOffSession),
}

/// Outcome of [`SessionDriver::handle_explicit_join`]. The success
/// branch returns the still-Menu-state session so the menu loop
/// continues; failure terminates with `LogoffReason::NoConferenceAccess`.
enum ExplicitJoinResult {
    /// The user is now attached to a (possibly fallback) conference.
    Joined(MenuSession),
    /// The user lost their last membership; the session is closing.
    NoAccess(LoggingOffSession),
}

/// Parsed shape of a `J <number>` command. Returned by
/// [`parse_join_command`].
enum JoinArg {
    /// `J <n>` where `<n>` parsed as a `u32`.
    Number(u32),
    /// `J` (or `J ` / `J\t`) with no number.
    Missing,
    /// `J <token>` where `<token>` could not be parsed as a `u32`.
    Invalid,
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline auto-rejoin announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5071-5073`). Returns just the
/// conference-name segment when the lookup fails, which is
/// defensive — `auto_rejoin_conference` only reports
/// `conference_number`s that came from the catalogue.
fn format_auto_rejoin_line(
    conferences: &[crate::domain::conference::Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    crate::app::wire_text::auto_rejoin_line(conference_number, conference_name, msgbase_name)
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline explicit-join announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5079-5083`).
fn format_explicit_join_line(
    conferences: &[crate::domain::conference::Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    crate::app::wire_text::explicit_join_line(conference_name, msgbase_name)
}

/// Resolves `(conference_name, msgbase_name)` for the wire-format
/// helpers. The `msgbase_name` is `Some(_)` only when the
/// conference holds more than one message base, mirroring the
/// `getConfMsgBaseCount(conf)>1` branch in legacy `joinConf`.
fn resolve_conference_strings(
    conferences: &[crate::domain::conference::Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> (&str, Option<&str>) {
    let Some(conference) = conferences.iter().find(|c| c.number() == conference_number) else {
        return ("?", None);
    };
    let msgbase_name = if conference.msgbases().len() > 1 {
        conference
            .msgbases()
            .iter()
            .find(|m| m.number() == msgbase_number)
            .map(crate::domain::conference::MessageBase::name)
    } else {
        None
    };
    (conference.name(), msgbase_name)
}

/// Recognises the Phase-4 `J` / `J <num>` menu command. Returns
/// `None` for any other typed line so the menu loop can fall
/// through to its existing dispatch (currently only `G`). Mirrors
/// the legacy parsing in `amiexpress/express.e:25140` modulo the
/// `getInverse` macro, which Phase 4 doesn't model yet.
fn parse_join_command(line: &str) -> Option<JoinArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("J") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(JoinArg::Missing);
    };
    if tokens.next().is_some() {
        // Extra trailing tokens are treated as a malformed argument
        // rather than silently ignored.
        return Some(JoinArg::Invalid);
    }
    match arg.parse::<u32>() {
        Ok(n) => Some(JoinArg::Number(n)),
        Err(_) => Some(JoinArg::Invalid),
    }
}

impl<T> SessionDriver<T>
where
    T: Terminal,
{
    /// Constructs a driver for a newly accepted connection. The
    /// session itself is not constructed until [`Self::run`] starts.
    #[must_use]
    pub(crate) fn new(
        terminal: T,
        node_number: u32,
        channel: LogonChannel,
        services: AppServices,
    ) -> Self {
        Self {
            terminal,
            services,
            node_number,
            channel,
        }
    }

    /// Runs the BBS workflow until the terminal closes or the session
    /// reaches a final logoff path.
    pub(crate) async fn run(&mut self) -> Result<(), T::Error> {
        let connecting =
            ConnectingSession::accept(self.node_number, self.channel, 0, SystemTime::now())
                .expect("freshly allocated node has no existing session");
        let identifying = self.start(connecting).await?;
        let signed_in = self.identify(identifying).await?;

        let logging_off = match signed_in {
            SignInResult::Onboarded(onboarded) => match self.auto_rejoin(onboarded).await? {
                AutoRejoinResult::Joined(onboarded) => {
                    let menu = self.enter_menu(onboarded);
                    self.run_menu(menu).await?
                }
                AutoRejoinResult::NoAccess(logging_off) => logging_off,
            },
            SignInResult::LoggingOff(logging_off) => logging_off,
            SignInResult::Ended(_ended) => return Ok(()),
        };

        self.finalise(logging_off);
        Ok(())
    }

    /// Resolves `conferences.allium:JoinConference` for the
    /// auto-rejoin path (Slice 30) and renders the JOINED screen and
    /// any name-type promotion screen (Slices 31 / 34). On
    /// `NoAccess` the listener writes the no-access line so the user
    /// understands why their session is closing — the
    /// caller-log finalise entry will already record the underlying
    /// `LogoffReason::NoConferenceAccess`.
    async fn auto_rejoin(
        &mut self,
        onboarded: OnboardedSession,
    ) -> Result<AutoRejoinResult, T::Error> {
        let conferences = self.services.conferences();
        match onboarded.auto_rejoin_conference(conferences, SystemTime::now()) {
            AutoRejoinTransition::Joined {
                session,
                conference_number,
                msgbase_number,
                show_bulletin: _,
                name_type_promoted_to,
            } => {
                let line = format_auto_rejoin_line(conferences, conference_number, msgbase_number);
                self.write_and_flush(&line).await?;
                self.render_name_type_promotion(name_type_promoted_to)
                    .await?;
                Ok(AutoRejoinResult::Joined(session))
            }
            AutoRejoinTransition::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(AutoRejoinResult::NoAccess(logging_off))
            }
        }
    }

    /// Renders `SCREEN_REALNAMES` / `SCREEN_INTERNETNAMES` when a
    /// join promoted the session's `display_name_type` (Slice 34).
    async fn render_name_type_promotion(
        &mut self,
        promoted: Option<NameType>,
    ) -> Result<(), T::Error> {
        let bytes = match promoted {
            Some(NameType::RealName) => self.services.screens().realnames_screen().await,
            Some(NameType::InternetName) => self.services.screens().internetnames_screen().await,
            Some(NameType::Handle) | None => return Ok(()),
        };
        self.terminal.write(&bytes).await?;
        self.terminal.flush().await
    }

    /// Returns the terminal after the driver has finished. Intended
    /// for tests and adapter-owned cleanup.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn into_terminal(self) -> T {
        self.terminal
    }

    async fn start(
        &mut self,
        connecting: ConnectingSession,
    ) -> Result<IdentifyingSession, T::Error> {
        let banner = self.services.screens().banner().await;
        self.terminal.write(&banner).await?;
        self.terminal.write(COPYRIGHT_LINES).await?;
        Ok(connecting.prompt_for_name())
    }

    async fn identify(
        &mut self,
        mut session: IdentifyingSession,
    ) -> Result<SignInResult, T::Error> {
        loop {
            let read = self
                .read_prompted(NAME_PROMPT, TerminalEcho::Visible)
                .await?;
            let line = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(SignInResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(SignInResult::LoggingOff(logoff));
                }
            };
            let trimmed = line.trim();
            let transition = session_flow::typed::name_typed(
                session,
                trimmed,
                self.services.user_repo(),
                self.services.new_user_gate(),
                SystemTime::now(),
            );
            match transition {
                NameTypedTransition::Authenticated(authenticating) => {
                    return self
                        .authenticate(authenticating)
                        .await
                        .map(|auth| match auth {
                            AuthResult::Onboarded(s) => SignInResult::Onboarded(s),
                            AuthResult::LoggingOff(s) => SignInResult::LoggingOff(s),
                        });
                }
                NameTypedTransition::Identifying(retry) => {
                    self.terminal.write(UNKNOWN_USER_LINE).await?;
                    session = retry;
                }
                NameTypedTransition::NewUserRegistering {
                    session: registering,
                    password_required,
                } => {
                    return self
                        .run_new_user_registration(registering, password_required)
                        .await;
                }
                NameTypedTransition::Disallowed(logging_off) => {
                    let screen = self.services.screens().no_new_users().await;
                    self.terminal.write(&screen).await?;
                    self.terminal.flush().await?;
                    return Ok(SignInResult::LoggingOff(logging_off));
                }
                NameTypedTransition::Ended(ended) => {
                    self.terminal.write(TOO_MANY_RETRIES_LINE).await?;
                    self.terminal.flush().await?;
                    return Ok(SignInResult::Ended(ended));
                }
            }
        }
    }

    async fn authenticate(
        &mut self,
        mut session: AuthenticatingSession,
    ) -> Result<AuthResult, T::Error> {
        loop {
            let read = self
                .read_prompted(PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let password = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(AuthResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(AuthResult::LoggingOff(logoff));
                }
            };
            let transition = session_flow::typed::verify_password(
                session,
                password.trim(),
                self.services.user_repo(),
                self.services.hasher(),
                self.services.caller_log(),
                self.services.session_policy(),
                SystemTime::now(),
            )
            .expect("AuthenticatingSession guarantees Authenticating + bound user");
            match transition {
                VerifyPasswordTransition::Onboarded(onboarded) => {
                    self.write_and_flush(AUTHENTICATED_LINE).await?;
                    return Ok(AuthResult::Onboarded(onboarded));
                }
                VerifyPasswordTransition::Authenticating(retry) => {
                    self.write_and_flush(WRONG_PASSWORD_LINE).await?;
                    session = retry;
                }
                VerifyPasswordTransition::LoggingOff {
                    session: logging_off,
                    reason,
                } => {
                    let line: &[u8] = match reason {
                        VerifyPasswordRejectionReason::AccountLocked => ACCOUNT_LOCKED_LINE,
                        VerifyPasswordRejectionReason::TooManyFailures => {
                            TOO_MANY_PASSWORD_FAILURES_LINE
                        }
                        VerifyPasswordRejectionReason::LogonRejected => LOGON_REJECTED_LINE,
                    };
                    self.write_and_flush(line).await?;
                    return Ok(AuthResult::LoggingOff(logging_off));
                }
            }
        }
    }

    fn enter_menu(&mut self, onboarded: OnboardedSession) -> MenuSession {
        session_flow::typed::enter_menu(
            onboarded,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("OnboardedSession with no force_password_reset enters menu cleanly")
    }

    async fn run_menu(&mut self, mut session: MenuSession) -> Result<LoggingOffSession, T::Error> {
        loop {
            let access_level = session.user().access_level();
            let menu_bytes = match session.current_conference_number() {
                Some(conf) => {
                    self.services
                        .screens()
                        .conference_menu(conf, access_level)
                        .await
                }
                None => self.services.screens().default_menu(access_level).await,
            };
            self.terminal.write(&menu_bytes).await?;
            let read = self
                .read_prompted(MENU_PROMPT, TerminalEcho::Visible)
                .await?;
            let line = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => return Ok(session.into_active().apply_carrier_loss()),
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(logoff);
                }
            };
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("G") {
                let logging_off = session.user_requests_logoff();
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(logging_off);
            }
            if let Some(arg) = parse_join_command(trimmed) {
                match arg {
                    JoinArg::Number(n) => {
                        session = match self.handle_explicit_join(session, n).await? {
                            ExplicitJoinResult::Joined(menu) => menu,
                            ExplicitJoinResult::NoAccess(logging_off) => return Ok(logging_off),
                        };
                    }
                    JoinArg::Missing => {
                        self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?;
                    }
                    JoinArg::Invalid => {
                        self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
                    }
                }
                continue;
            }
            self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
        }
    }

    /// Handles a `J <num>` command from the menu (Slice 32). Writes
    /// the legacy "no access" notice when the resolver fell through,
    /// the inline `Joining Conference: <name>` announcement on
    /// success, and any name-type promotion screen (Slice 34).
    async fn handle_explicit_join(
        &mut self,
        session: MenuSession,
        target_conference_number: u32,
    ) -> Result<ExplicitJoinResult, T::Error> {
        let conferences = self.services.conferences();
        let outcome = session.explicit_join_conference(
            target_conference_number,
            conferences,
            SystemTime::now(),
        );
        match outcome {
            ExplicitJoinTransition::Joined {
                session,
                conference_number,
                msgbase_number,
                matched_request,
                name_type_promoted_to,
                ..
            } => {
                // Compute the announcement bytes up-front so the
                // immutable borrow on `self.services.conferences()`
                // doesn't overlap the mutable borrows below.
                let line =
                    format_explicit_join_line(conferences, conference_number, msgbase_number);
                if !matched_request {
                    self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                        .await?;
                }
                self.write_and_flush(&line).await?;
                self.render_name_type_promotion(name_type_promoted_to)
                    .await?;
                Ok(ExplicitJoinResult::Joined(session))
            }
            ExplicitJoinTransition::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(ExplicitJoinResult::NoAccess(logging_off))
            }
        }
    }

    fn finalise(&mut self, logging_off: LoggingOffSession) -> EndedSession {
        session_flow::typed::finalise_logoff(
            logging_off,
            self.services.user_repo(),
            self.services.caller_log(),
            SystemTime::now(),
        )
        .expect("LoggingOffSession finalises cleanly when persistence succeeds")
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        self.terminal.write(prompt).await?;
        self.terminal.flush().await?;
        let timeout = self.services.session_policy().input_timeout();
        self.terminal.read_line(echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        self.terminal.write(bytes).await?;
        self.terminal.flush().await
    }

    async fn run_new_user_registration(
        &mut self,
        session: NewUserRegisteringSession,
        password_required: bool,
    ) -> Result<SignInResult, T::Error> {
        let screen = self.services.screens().new_user_password().await;
        self.terminal.write(&screen).await?;
        let session = if password_required {
            match self.run_new_user_password_gate(session).await? {
                GateResult::Verified(s) => s,
                GateResult::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
            }
        } else {
            session
        };

        let (session, handle) = match self.read_registration_handle(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, location) = match self.read_optional_field(session, LOCATION_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, phone_number) = match self.read_optional_field(session, PHONE_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, email) = match self.read_optional_field(session, EMAIL_PROMPT).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, password) = match self.read_registration_password(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, line_length) = match self.read_line_length(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };
        let (session, ansi_colour) = match self.read_ansi_colour(session).await? {
            ReadField::Got(s, v) => (s, v),
            ReadField::LoggingOff(s) => return Ok(SignInResult::LoggingOff(s)),
        };

        let profile = NewUserProfile {
            handle,
            location,
            phone_number,
            email,
            password,
            line_length,
            ansi_colour,
            flags: BTreeSet::new(),
        };
        self.complete_new_user_registration(session, profile).await
    }

    async fn run_new_user_password_gate(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<GateResult, T::Error> {
        loop {
            let read = self
                .read_prompted(NEW_USER_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => {
                    return Ok(GateResult::LoggingOff(
                        session.into_active().apply_carrier_loss(),
                    ));
                }
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(GateResult::LoggingOff(logoff));
                }
            };
            let transition = session_flow::typed::verify_new_user_password(
                session,
                typed.trim(),
                self.services.new_user_gate(),
                self.services.caller_log(),
                SystemTime::now(),
            )
            .expect("NewUserRegisteringSession + configured gate guarantees flow ok");
            match transition {
                NewUserPasswordTransition::Verified(s) => {
                    self.write_and_flush(NEW_USER_PASSWORD_OK_LINE).await?;
                    return Ok(GateResult::Verified(s));
                }
                NewUserPasswordTransition::Mismatch(s) => {
                    self.write_and_flush(NEW_USER_INVALID_PASSWORD_LINE).await?;
                    session = s;
                }
                NewUserPasswordTransition::TooManyFailures(s) => {
                    self.write_and_flush(NEW_USER_EXCESSIVE_FAILURES_LINE)
                        .await?;
                    return Ok(GateResult::LoggingOff(s));
                }
            }
        }
    }

    async fn read_registration_handle(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<String>, T::Error> {
        let mut attempts: u32 = 0;
        loop {
            if attempts >= MAX_REGISTRATION_HANDLE_ATTEMPTS {
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                return Ok(ReadField::LoggingOff(
                    session.into_active().apply_carrier_loss(),
                ));
            }
            let read = self
                .read_prompted(REGISTRATION_HANDLE_PROMPT, TerminalEcho::Visible)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            let trimmed = typed.trim();
            let available = !trimmed.is_empty()
                && trimmed != NEW_USER_REGISTRATION_LITERAL
                && matches!(
                    self.services.user_repo().find_by_handle(trimmed),
                    NameLookupResult::NotFound
                );
            if available {
                return Ok(ReadField::Got(session, trimmed.to_string()));
            }
            self.terminal.write(HANDLE_TAKEN_LINE).await?;
            attempts += 1;
        }
    }

    async fn read_optional_field(
        &mut self,
        mut session: NewUserRegisteringSession,
        prompt: &[u8],
    ) -> Result<ReadField<Option<String>>, T::Error> {
        let read = self.read_prompted(prompt, TerminalEcho::Visible).await?;
        let typed = match read {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                line
            }
            other => return self.handle_interrupt_registering(session, other).await,
        };
        let trimmed = typed.trim();
        let value = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        Ok(ReadField::Got(session, value))
    }

    async fn read_registration_password(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<String>, T::Error> {
        loop {
            let read = self
                .read_prompted(REGISTRATION_PASSWORD_PROMPT, TerminalEcho::Masked)
                .await?;
            let password = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            if password.trim().is_empty() {
                continue;
            }
            let confirm_read = self
                .read_prompted(REGISTRATION_PASSWORD_CONFIRM_PROMPT, TerminalEcho::Masked)
                .await?;
            let confirmed = match confirm_read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            if password == confirmed {
                return Ok(ReadField::Got(session, password));
            }
            self.terminal.write(PASSWORDS_DO_NOT_MATCH_LINE).await?;
        }
    }

    async fn read_line_length(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<u32>, T::Error> {
        loop {
            let read = self
                .read_prompted(LINE_LENGTH_PROMPT, TerminalEcho::Visible)
                .await?;
            let typed = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                other => return self.handle_interrupt_registering(session, other).await,
            };
            let trimmed = typed.trim();
            if trimmed.is_empty() {
                return Ok(ReadField::Got(session, 0));
            }
            match trimmed.parse::<u32>() {
                Ok(value) if value <= 255 => return Ok(ReadField::Got(session, value)),
                _ => {
                    self.terminal.write(INVALID_LINE_LENGTH_LINE).await?;
                }
            }
        }
    }

    async fn read_ansi_colour(
        &mut self,
        mut session: NewUserRegisteringSession,
    ) -> Result<ReadField<bool>, T::Error> {
        let read = self
            .read_prompted(ANSI_PROMPT, TerminalEcho::Visible)
            .await?;
        let typed = match read {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                line
            }
            other => return self.handle_interrupt_registering(session, other).await,
        };
        let value = !typed.trim().eq_ignore_ascii_case("N");
        Ok(ReadField::Got(session, value))
    }

    /// Common handler for `Eof` / `IdleTimedOut` while collecting a
    /// registration field. Consumes the session, applies the
    /// appropriate domain transition, and emits the wire message.
    async fn handle_interrupt_registering<TVal>(
        &mut self,
        session: NewUserRegisteringSession,
        outcome: TerminalRead,
    ) -> Result<ReadField<TVal>, T::Error> {
        let logoff = match outcome {
            TerminalRead::Eof => session.into_active().apply_carrier_loss(),
            TerminalRead::IdleTimedOut => {
                let logoff = session
                    .into_active()
                    .apply_idle_timeout(self.services.session_policy().treat_timeout_as_logoff());
                self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                logoff
            }
            TerminalRead::Line(_) => unreachable!("interrupt path is for non-Line outcomes"),
        };
        Ok(ReadField::LoggingOff(logoff))
    }

    async fn complete_new_user_registration(
        &mut self,
        session: NewUserRegisteringSession,
        profile: NewUserProfile,
    ) -> Result<SignInResult, T::Error> {
        let flow = NewUserRegistrationFlow::new(
            self.services.user_repo(),
            self.services.hasher(),
            self.services.caller_log(),
            self.services.default_ratio(),
            self.services.session_policy(),
        );
        match flow.complete_typed(session, profile, SystemTime::now()) {
            Ok(NewUserRegistrationResult::Onboarded(onboarded)) => {
                self.write_and_flush(REGISTRATION_COMPLETE_LINE).await?;
                Ok(SignInResult::Onboarded(onboarded))
            }
            Ok(NewUserRegistrationResult::LoggingOff(logging_off)) => {
                // Post-onboarded RejectLockedOrInsufficientAccess
                // short-circuited the cluster. Wire-message parity with
                // the legacy path: tell the user the logon was rejected
                // and let `finalise` close the session.
                self.write_and_flush(LOGON_REJECTED_LINE).await?;
                Ok(SignInResult::LoggingOff(logging_off))
            }
            Err(boxed) => {
                let (session, _error) = *boxed;
                // Hash, repo, or constructor error. The session is
                // unchanged (still NewUserRegistering); apply
                // carrier-loss so finalise can close it cleanly.
                self.write_and_flush(REGISTRATION_RETRIES_EXHAUSTED_LINE)
                    .await?;
                let logoff = session.into_active().apply_carrier_loss();
                Ok(SignInResult::LoggingOff(logoff))
            }
        }
    }
}

/// Result of reading a single registration field. The success arm
/// returns the session by value alongside the field — the caller
/// continues with both. The interrupt arm carries the session-now-
/// logging-off so the caller bails up to [`SignInResult::LoggingOff`].
enum ReadField<TVal> {
    Got(NewUserRegisteringSession, TVal),
    LoggingOff(LoggingOffSession),
}

/// Result of the new-user password gate. Either it passed (returns
/// the wrapper for the caller to continue with) or it failed with a
/// terminal outcome.
enum GateResult {
    Verified(NewUserRegisteringSession),
    LoggingOff(LoggingOffSession),
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::screens::{ScreenFuture, ScreenRepository};
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::{LogonChannel, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};

    use super::{resolve_conference_strings, SessionDriver};

    #[test]
    fn resolve_conference_strings_returns_name_only_for_single_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = false` branch in
        // legacy `joinConf` (`amiexpress/express.e:5072`): the
        // announcement omits the `[<msgbase>]` segment.
        use crate::domain::conference::{Conference, MessageBase};
        let confs = vec![Conference::new(
            7,
            "Solo".to_string(),
            vec![MessageBase::new(7, 1, "main".to_string())],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 7, 1);
        assert_eq!(name, "Solo");
        assert!(
            mb.is_none(),
            "single-msgbase conferences should not include a msgbase name"
        );
    }

    #[test]
    fn resolve_conference_strings_emits_msgbase_for_multi_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = true` branch in
        // legacy `joinConf` (`amiexpress/express.e:5070`): the
        // announcement carries `[<msgbase>]`.
        use crate::domain::conference::{Conference, MessageBase};
        let confs = vec![Conference::new(
            3,
            "Tech-and-misc".to_string(),
            vec![
                MessageBase::new(3, 1, "main".to_string()),
                MessageBase::new(3, 2, "tech".to_string()),
            ],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 3, 2);
        assert_eq!(name, "Tech-and-misc");
        assert_eq!(mb, Some("tech"));
    }

    #[test]
    fn resolve_conference_strings_returns_question_mark_for_unknown_conference() {
        // Defensive fallback: a conference number that's not in the
        // catalogue produces "?". Today this is unreachable (the
        // resolver only reports numbers that came from the
        // catalogue) but the helper has to be total.
        let (name, mb) = resolve_conference_strings(&[], 99, 1);
        assert_eq!(name, "?");
        assert!(mb.is_none());
    }

    struct FakeTerminal {
        inputs: VecDeque<TerminalRead>,
        output: Vec<u8>,
        echo_modes: Vec<TerminalEcho>,
    }

    impl FakeTerminal {
        fn new(inputs: impl IntoIterator<Item = TerminalRead>) -> Self {
            Self {
                inputs: inputs.into_iter().collect(),
                output: Vec::new(),
                echo_modes: Vec::new(),
            }
        }

        fn output(&self) -> &[u8] {
            &self.output
        }
    }

    impl Terminal for FakeTerminal {
        type Error = Infallible;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            Box::pin(async move {
                self.output.extend_from_slice(bytes);
                Ok(())
            })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(
            &mut self,
            echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async move {
                self.echo_modes.push(echo);
                Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof))
            })
        }
    }

    struct StaticScreens;

    impl ScreenRepository for StaticScreens {
        fn banner(&self) -> ScreenFuture<'_> {
            bytes(b"BANNER\r\n")
        }

        fn default_menu(&self, _access_level: u8) -> ScreenFuture<'_> {
            bytes(b"MENU\r\n")
        }

        fn conference_menu(&self, _conference_number: u32, _access_level: u8) -> ScreenFuture<'_> {
            bytes(b"CONFMENU\r\n")
        }

        fn new_user_password(&self) -> ScreenFuture<'_> {
            bytes(b"NEW USER\r\n")
        }

        fn no_new_users(&self) -> ScreenFuture<'_> {
            bytes(b"NO NEW USERS\r\n")
        }

        fn joinconf_screen(&self) -> ScreenFuture<'_> {
            bytes(b"JOINCONF\r\n")
        }

        fn realnames_screen(&self) -> ScreenFuture<'_> {
            bytes(b"REALNAMES\r\n")
        }

        fn internetnames_screen(&self) -> ScreenFuture<'_> {
            bytes(b"INTERNETNAMES\r\n")
        }
    }

    fn bytes(value: &'static [u8]) -> ScreenFuture<'static> {
        Box::pin(async move { value.to_vec() })
    }

    fn alice_with_password(password: &str) -> User {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash(password, PasswordHashKind::Pbkdf210000)
            .expect("compute");
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    #[tokio::test]
    async fn driver_runs_signin_menu_and_logoff_without_a_telnet_transport() {
        use crate::domain::conference::{Conference, MessageBase};
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid")];
        let mut alice = alice_with_password("secret");
        crate::app::seed::grant_all_memberships(&mut alice, &conferences);
        let repo = Arc::new(InMemoryUserRepository::new(vec![alice]));
        let hasher = Arc::new(Pbkdf2PasswordHasher::new());
        let caller_log = Arc::new(InMemoryCallerLog::new());
        let screens = Arc::new(StaticScreens);
        let gate = NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        };
        let ratio = DefaultRatio {
            mode: RatioMode::ByFiles,
            value: 3,
        };
        let services = AppServices::new(
            repo,
            hasher,
            caller_log.clone(),
            screens,
            Arc::new(conferences),
            SessionPolicy::default(),
            ratio,
            gate,
        );
        let terminal = FakeTerminal::new([
            TerminalRead::Line("alice".to_string()),
            TerminalRead::Line("secret".to_string()),
            TerminalRead::Line("G".to_string()),
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        assert!(output.windows(b"BANNER".len()).any(|w| w == b"BANNER"));
        assert!(output
            .windows(b"PassWord: ".len())
            .any(|w| w == b"PassWord: "));
        assert!(output
            .windows(b"Authenticated".len())
            .any(|w| w == b"Authenticated"));
        assert!(output.windows(b"MENU".len()).any(|w| w == b"MENU"));
        assert!(output.windows(b"Goodbye".len()).any(|w| w == b"Goodbye"));
        assert_eq!(
            terminal.echo_modes,
            vec![
                TerminalEcho::Visible,
                TerminalEcho::Masked,
                TerminalEcho::Visible
            ]
        );
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logon:") && entry.text.contains("alice")));
        assert!(caller_log
            .entries()
            .iter()
            .any(|entry| entry.text.contains("Logoff:") && entry.text.contains("alice")));
    }

    #[tokio::test]
    async fn registration_handle_prompt_rejects_new_literal() {
        // After moving the NEW literal out of UserRepository the
        // registration handle prompt has to explicitly reject the
        // command word; otherwise a user could register themselves
        // under the same name the login flow uses to trigger
        // registration in the first place.
        let repo = Arc::new(InMemoryUserRepository::new(vec![]));
        let hasher = Arc::new(Pbkdf2PasswordHasher::new());
        let caller_log = Arc::new(InMemoryCallerLog::new());
        let screens = Arc::new(StaticScreens);
        let gate = NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        };
        let ratio = DefaultRatio {
            mode: RatioMode::ByFiles,
            value: 3,
        };
        let services = AppServices::new(
            repo,
            hasher,
            caller_log,
            screens,
            Arc::new(vec![]),
            SessionPolicy::default(),
            ratio,
            gate,
        );
        let terminal = FakeTerminal::new([
            TerminalRead::Line("NEW".to_string()),
            // First registration handle attempt — should be rejected.
            TerminalRead::Line("NEW".to_string()),
            // EOF ends the run; the test only cares that we
            // see HANDLE_TAKEN_LINE after the NEW attempt.
            TerminalRead::Eof,
        ]);
        let mut driver = SessionDriver::new(terminal, 1, LogonChannel::Remote, services);

        driver.run().await.expect("driver completes");

        let terminal = driver.into_terminal();
        let output = terminal.output();
        let taken = b"That name is taken.";
        assert!(
            output.windows(taken.len()).any(|w| w == taken),
            "expected handle-taken line to appear after typing NEW at the registration prompt",
        );
    }
}
