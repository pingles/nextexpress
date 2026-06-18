//! Application use cases for driving a [`Session`].
//!
//! This module owns orchestration across domain ports. The [`Session`]
//! entity applies state changes and returns domain values; these
//! functions call repositories, hashers and logs around those pure
//! transitions.
//!
//! Every flow takes a [`crate::domain::session::typed`] phase wrapper
//! by value and returns the appropriate next-phase wrapper or
//! transition enum, so the wrong-state failure mode is unrepresentable
//! at the call sites. (`complete_password_reset` still drives the raw
//! [`Session`]; its driver path lands with the password-reset slice.)

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::{
    meets_password_strength, PasswordError, PasswordHashKind, PasswordHasher,
};
use crate::domain::session::typed::{
    AuthenticatingSession, EndedSession, IdentifyingSession, LoggingOffSession, MenuSession,
    NameTypedTransition, NewUserPasswordTransition, NewUserRegisteringSession,
    NewUserRegistrationResult, OnboardedSession, VerifyPasswordRejectionReason,
    VerifyPasswordTransition,
};
use crate::domain::session::{
    apply_password_change, apply_password_match, apply_password_mismatch,
    CompleteNewUserRegistrationError, CompletePasswordResetError, EnterMenuError, NameTypedOutcome,
    NewUserPasswordOutcome, NewUserRequestOutcome, Session, SessionPolicy, SessionState,
    SessionTransitionError, VerifyNewUserPasswordError, VerifyPasswordError, VerifyPasswordOutcome,
};
use crate::domain::user::{NewUserDraft, RatioMode, UserFlag};
use crate::domain::user_repository::{
    NameLookupResult, UserCreationError, UserRepository, UserRepositoryError,
};

/// Errors returned by [`verify_password`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyPasswordFlowError {
    /// The underlying session rule failed.
    #[error(transparent)]
    Session(#[from] VerifyPasswordError),
    /// The changed user record could not be persisted.
    #[error(transparent)]
    Save(#[from] UserRepositoryError),
}

/// Errors returned by [`enter_menu`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnterMenuFlowError {
    /// The underlying session rule failed.
    #[error(transparent)]
    Session(#[from] EnterMenuError),
    /// The changed user record could not be persisted.
    #[error(transparent)]
    Save(#[from] UserRepositoryError),
}

/// Errors returned by [`finalise_logoff`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FinaliseLogoffFlowError {
    /// The underlying session rule failed.
    #[error(transparent)]
    Session(#[from] SessionTransitionError),
    /// The changed user record could not be persisted.
    #[error(transparent)]
    Save(#[from] UserRepositoryError),
}

/// Login-command literal that triggers the new-user registration
/// branch of `session.allium:NameTyped`. The legacy `AmiExpress`
/// source recognises this at the name prompt; keeping it next to the
/// `name_typed` flow (rather than inside `UserRepository`) means
/// repository adapters stay pure storage.
pub const NEW_USER_REGISTRATION_LITERAL: &str = "NEW";

/// Returns `true` when `typed` is acceptable as a new user's handle
/// during the registration sub-flow: non-empty after trimming, not the
/// reserved [`NEW_USER_REGISTRATION_LITERAL`], and not already taken
/// in `repo`.
///
/// Centralises the three checks the registration driver used to do
/// inline: the empty / reserved gates are format rules and the
/// repository lookup is a uniqueness query — composing them here
/// keeps the driver flow free of domain logic.
///
/// # Errors
/// Returns [`UserRepositoryError`] when the repository cannot check
/// whether the trimmed handle already exists.
pub fn is_handle_available_for_registration<R>(
    repo: &R,
    typed: &str,
) -> Result<bool, UserRepositoryError>
where
    R: UserRepository + ?Sized,
{
    let trimmed = typed.trim();
    if trimmed.is_empty() || trimmed == NEW_USER_REGISTRATION_LITERAL {
        return Ok(false);
    }
    Ok(matches!(
        repo.find_by_handle(trimmed)?,
        NameLookupResult::NotFound
    ))
}

/// Handles `session.allium:NameTyped`.
///
/// Resolves `typed` to one of three branches:
///
/// 1. `NEW_USER_REGISTRATION_LITERAL` — fires the on-enter cluster
///    for `new_user_registering` (`RejectDisallowedRegistration` and
///    `InitialiseNewUserGate`, Slice 20a) using `gate`.
/// 2. Known handle in `repo` — binds the user and moves to
///    authentication.
/// 3. Unknown handle — records the miss for the unknown-name rule.
///
/// The [`IdentifyingSession`] wrapper guarantees the underlying
/// session state, so the rule's wrong-state failure mode is
/// unrepresentable here.
///
/// # Errors
/// Returns [`UserRepositoryError`] when the repository cannot resolve
/// a non-registration handle.
pub(crate) fn name_typed<R>(
    session: IdentifyingSession,
    typed: &str,
    repo: &R,
    gate: &NewUserGateConfig,
    now: SystemTime,
) -> Result<NameTypedTransition, UserRepositoryError>
where
    R: UserRepository + ?Sized,
{
    let mut inner = session.into_inner();
    let outcome = if typed == NEW_USER_REGISTRATION_LITERAL {
        let outcome = inner
            .record_new_user_request(gate.allow_new_users, gate.new_user_password.is_some(), now)
            .expect("IdentifyingSession guarantees Identifying state");
        match outcome {
            NewUserRequestOutcome::Initialised { password_required } => {
                NameTypedOutcome::NewUserRegistering { password_required }
            }
            NewUserRequestOutcome::Rejected => NameTypedOutcome::NewUserRegistrationDisallowed,
        }
    } else {
        match repo.find_by_handle(typed)? {
            NameLookupResult::Found(user) => inner.record_identified_user(typed, *user),
            NameLookupResult::NotFound => inner.record_unknown_name(now),
        }
        .expect("IdentifyingSession guarantees Identifying state")
    };
    let transition = match outcome {
        NameTypedOutcome::Authenticated => {
            NameTypedTransition::Authenticated(AuthenticatingSession::from_session(inner))
        }
        NameTypedOutcome::NotFound => {
            NameTypedTransition::Identifying(IdentifyingSession::from_session(inner))
        }
        NameTypedOutcome::NewUserRegistering { password_required } => {
            NameTypedTransition::NewUserRegistering {
                session: NewUserRegisteringSession::from_session(inner),
                password_required,
            }
        }
        NameTypedOutcome::NewUserRegistrationDisallowed => {
            NameTypedTransition::Disallowed(LoggingOffSession::from_session(inner))
        }
        NameTypedOutcome::SessionEnded => {
            NameTypedTransition::Ended(EndedSession::from_session(inner))
        }
    };
    Ok(transition)
}

/// Configuration for the new-user registration gate, threaded through
/// [`name_typed`] and [`verify_new_user_password`]. Mirrors the
/// `core/config.{allow_new_users, new_user_password,
/// max_new_user_password_attempts}` triple.
#[derive(Debug, Clone)]
pub struct NewUserGateConfig {
    /// Whether the BBS accepts new-user registrations at all
    /// (`core/config.allow_new_users`).
    pub allow_new_users: bool,
    /// Optional sysop-set password gating registration
    /// (`core/config.new_user_password`). `None` disables the gate.
    pub new_user_password: Option<String>,
    /// Retry budget for the gate
    /// (`core/config.max_new_user_password_attempts`).
    pub max_new_user_password_attempts: u32,
}

/// Errors returned by [`verify_new_user_password`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyNewUserPasswordFlowError {
    /// The underlying session rule failed.
    #[error(transparent)]
    Session(#[from] VerifyNewUserPasswordError),
    /// The gate configuration is missing — the caller invoked the
    /// gate flow even though `core/config.new_user_password` is
    /// `None`. The listener should never reach here in production;
    /// returning the error rather than silently passing protects
    /// against logic bugs.
    #[error("verify_new_user_password called with no gate configured")]
    GateNotConfigured,
}

/// Handles `session.allium:VerifyNewUserPassword` (Slice 20a).
///
/// Compares `candidate` against `gate.new_user_password` under the
/// case-insensitive equality the legacy `AmiExpress` source uses
/// (`StriCmp` at `amiexpress/express.e:30027`), then applies the
/// resulting [`Session`] transition. On a mismatch the caller-log
/// "New-user password failure" entry is appended to `caller_log`; the
/// session may move to logging-off when the attempt counter reaches
/// `gate.max_new_user_password_attempts`.
///
/// # Errors
/// Returns [`VerifyNewUserPasswordFlowError::Session`] when the
/// underlying session rule fails (already verified) or
/// [`VerifyNewUserPasswordFlowError::GateNotConfigured`] when the
/// caller invoked the gate flow without a configured password.
pub(crate) fn verify_new_user_password<L>(
    session: NewUserRegisteringSession,
    candidate: &str,
    gate: &NewUserGateConfig,
    caller_log: &L,
    now: SystemTime,
) -> Result<NewUserPasswordTransition, VerifyNewUserPasswordFlowError>
where
    L: CallerLogAppender + ?Sized,
{
    let secret = gate
        .new_user_password
        .as_deref()
        .ok_or(VerifyNewUserPasswordFlowError::GateNotConfigured)?;
    let matches = matches_new_user_password(candidate, secret);
    let mut inner = session.into_inner();
    let (outcome, entry) =
        inner.apply_new_user_password_attempt(matches, gate.max_new_user_password_attempts, now)?;
    if let Some(entry) = entry {
        caller_log.append(entry);
    }
    Ok(match outcome {
        NewUserPasswordOutcome::Verified => {
            NewUserPasswordTransition::Verified(NewUserRegisteringSession::from_session(inner))
        }
        NewUserPasswordOutcome::Mismatch => {
            NewUserPasswordTransition::Mismatch(NewUserRegisteringSession::from_session(inner))
        }
        NewUserPasswordOutcome::TooManyFailures => {
            NewUserPasswordTransition::TooManyFailures(LoggingOffSession::from_session(inner))
        }
    })
}

/// `session.allium:matches_new_user_password` black-box function.
/// Case-insensitive equality, mirroring the legacy `StriCmp` at
/// `amiexpress/express.e:30027`.
fn matches_new_user_password(candidate: &str, secret: &str) -> bool {
    candidate.eq_ignore_ascii_case(secret)
}

/// Handles `session.allium:VerifyPassword`.
///
/// Verifies `candidate` through `hasher`, applies the resulting
/// [`Session`] transition, and appends password-failure caller-log
/// entries when credentials do not match. The supplied
/// [`SessionPolicy`] controls password-failure limits.
///
/// # Errors
/// Returns [`VerifyPasswordError::HashKindUnsupported`] when the
/// hasher rejects the stored password kind, or
/// [`VerifyPasswordFlowError::Save`] when the changed user record
/// cannot be persisted. The wrong-state / user-missing failure modes
/// cannot fire: the [`AuthenticatingSession`] wrapper guarantees both
/// invariants.
pub(crate) fn verify_password<R, H, L>(
    session: AuthenticatingSession,
    candidate: &str,
    user_repo: &R,
    hasher: &H,
    caller_log: &L,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<VerifyPasswordTransition, VerifyPasswordFlowError>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    let mut inner = session.into_inner();
    let user = inner.user().ok_or(VerifyPasswordError::UserMissing)?;
    let matches = hasher
        .verify_password(user, candidate)
        .map_err(VerifyPasswordError::HashKindUnsupported)?;

    let outcome = if matches {
        let (outcome, rejection) = apply_password_match(&mut inner, policy, now)?;
        save_bound_user(&inner, user_repo)?;
        if let Some(entry) = rejection {
            caller_log.append(entry);
        }
        outcome
    } else {
        let (outcome, entry) = apply_password_mismatch(&mut inner, policy, now)?;
        save_bound_user(&inner, user_repo)?;
        caller_log.append(entry);
        outcome
    };
    Ok(match outcome {
        VerifyPasswordOutcome::Authenticated => {
            debug_assert_eq!(inner.state(), SessionState::Onboarded);
            VerifyPasswordTransition::Onboarded(OnboardedSession::from_session(inner))
        }
        VerifyPasswordOutcome::NotMatching => {
            debug_assert_eq!(inner.state(), SessionState::Authenticating);
            VerifyPasswordTransition::Authenticating(AuthenticatingSession::from_session(inner))
        }
        VerifyPasswordOutcome::AccountLocked => VerifyPasswordTransition::LoggingOff {
            session: LoggingOffSession::from_session(inner),
            reason: VerifyPasswordRejectionReason::AccountLocked,
        },
        VerifyPasswordOutcome::TooManyFailures => VerifyPasswordTransition::LoggingOff {
            session: LoggingOffSession::from_session(inner),
            reason: VerifyPasswordRejectionReason::TooManyFailures,
        },
        VerifyPasswordOutcome::LogonRejected => VerifyPasswordTransition::LoggingOff {
            session: LoggingOffSession::from_session(inner),
            reason: VerifyPasswordRejectionReason::LogonRejected,
        },
    })
}

/// Handles `session.allium:EnterMenu`.
///
/// Applies the domain transition and appends the resulting logon caller
/// log entry.
///
/// # Errors
/// Returns [`EnterMenuFlowError`] when the bound user has
/// `force_password_reset` set or persistence fails.
pub(crate) fn enter_menu<R, L>(
    session: OnboardedSession,
    user_repo: &R,
    caller_log: &L,
    now: SystemTime,
) -> Result<MenuSession, EnterMenuFlowError>
where
    R: UserRepository + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    let mut inner = session.into_inner();
    let entry = inner.enter_menu(now)?;
    save_bound_user(&inner, user_repo)?;
    caller_log.append(entry);
    Ok(MenuSession::from_session(inner))
}

/// Handles `session.allium:FinaliseLogoff`.
///
/// Applies the domain transition and appends the resulting logoff caller
/// log entry.
///
/// # Errors
/// Returns [`FinaliseLogoffFlowError`] from persistence; the
/// wrong-state guard in the domain rule cannot fire because
/// [`LoggingOffSession`] guarantees the `LoggingOff` state.
pub(crate) fn finalise_logoff<R, L>(
    session: LoggingOffSession,
    user_repo: &R,
    caller_log: &L,
    now: SystemTime,
) -> Result<EndedSession, FinaliseLogoffFlowError>
where
    R: UserRepository + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    let mut inner = session.into_inner();
    let entry = inner.finalise_logoff(now)?;
    save_bound_user(&inner, user_repo)?;
    caller_log.append(entry);
    Ok(EndedSession::from_session(inner))
}

fn save_bound_user<R>(session: &Session, user_repo: &R) -> Result<(), UserRepositoryError>
where
    R: UserRepository + ?Sized,
{
    if let Some(user) = session.user() {
        user_repo.save(user.clone())
    } else {
        Ok(())
    }
}

/// Profile collected from a user during the new-user registration
/// sub-flow (Slice 20). The hash is computed by
/// [`NewUserRegistrationFlow::complete`] from `password`; the slot
/// number is allocated from the user repository.
#[derive(Debug, Clone)]
pub struct NewUserProfile {
    /// Handle the user typed at the registration prompt.
    pub handle: String,
    /// Free-text "City, State" location.
    pub location: Option<String>,
    /// Phone number.
    pub phone_number: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Plain-text password to be hashed.
    pub password: String,
    /// Preferred terminal width (`0` = auto).
    pub line_length: u32,
    /// Whether the user wants ANSI colour output.
    pub ansi_colour: bool,
    /// Initial preference flags.
    pub flags: BTreeSet<UserFlag>,
}

/// Errors returned by [`NewUserRegistrationFlow::complete`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompleteNewUserRegistrationFlowError {
    /// The session is not in
    /// [`SessionState::NewUserRegistering`].
    #[error(transparent)]
    Session(#[from] CompleteNewUserRegistrationError),
    /// The hasher failed to compute a hash for the supplied password.
    #[error(transparent)]
    Hash(#[from] PasswordError),
    /// The repository couldn't allocate or persist the new account.
    /// Wraps both `User::register_new` validation failures and
    /// repository-side consistency errors (handle/slot collisions).
    #[error(transparent)]
    Create(#[from] UserCreationError),
}

/// Default ratio policy applied to a freshly-registered new account.
///
/// Mirrors `core/config.default_ratio_mode` and `default_ratio_value`
/// — the registration rule reads both at user creation time. The
/// caller threads `core/config` through here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultRatio {
    /// Ratio enforcement mode for the new account.
    pub mode: RatioMode,
    /// Ratio threshold for the new account.
    pub value: u32,
}

/// App-layer use case for `session.allium:CompleteNewUserRegistration`
/// (Slice 20).
///
/// The flow holds the driven ports and configuration so driving
/// adapters do not pass a long parameter list at every call site.
pub struct NewUserRegistrationFlow<'a, R, H, L>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    user_repo: &'a R,
    hasher: &'a H,
    caller_log: &'a L,
    default_ratio: DefaultRatio,
    policy: SessionPolicy,
}

impl<'a, R, H, L> NewUserRegistrationFlow<'a, R, H, L>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    /// Constructs a new registration-completion flow.
    ///
    /// # Parameters
    /// - `user_repo`: repository used to allocate and persist the new
    ///   account.
    /// - `hasher`: password hasher used for the supplied plaintext
    ///   registration password.
    /// - `caller_log`: log sink for any post-onboarded rejection entry.
    /// - `default_ratio`: ratio policy applied to the new account.
    /// - `policy`: session policy used by the post-onboarded rule
    ///   cluster.
    #[must_use]
    pub fn new(
        user_repo: &'a R,
        hasher: &'a H,
        caller_log: &'a L,
        default_ratio: DefaultRatio,
        policy: SessionPolicy,
    ) -> Self {
        Self {
            user_repo,
            hasher,
            caller_log,
            default_ratio,
            policy,
        }
    }

    /// Handles `session.allium:CompleteNewUserRegistration` (Slice 20).
    ///
    /// Allocates a slot from the user repository, hashes the supplied
    /// password under the spec's default
    /// [`PasswordHashKind`], constructs a [`User`] via
    /// [`User::register_new`], persists it, and applies the corresponding
    /// session transition through
    /// [`Session::complete_new_user_registration`]. The resulting
    /// `RejectLockedOrInsufficientAccess` caller-log entry (if any) is
    /// appended to the configured caller log so the post-onboarded
    /// cluster matches the password-match path's behaviour. Returns
    /// either an [`OnboardedSession`] (clean post-onboarded cluster) or
    /// a [`LoggingOffSession`] (post-onboarded rejection).
    ///
    /// # Errors
    /// On error, the still-registering session is returned alongside
    /// the failure so the caller can either retry or finalise via the
    /// carrier-loss / idle paths. Errors come from the hasher, the
    /// user constructor, or the repository.
    ///
    /// [`User`]: crate::domain::user::User
    /// [`User::register_new`]: crate::domain::user::User::register_new
    pub(crate) fn complete(
        &self,
        session: NewUserRegisteringSession,
        profile: NewUserProfile,
        now: SystemTime,
    ) -> Result<
        NewUserRegistrationResult,
        Box<(
            NewUserRegisteringSession,
            CompleteNewUserRegistrationFlowError,
        )>,
    > {
        let mut inner = session.into_inner();
        match self.apply_complete(&mut inner, profile, now) {
            Ok(()) => Ok(match inner.state() {
                SessionState::Onboarded => {
                    NewUserRegistrationResult::Onboarded(OnboardedSession::from_session(inner))
                }
                SessionState::LoggingOff => {
                    NewUserRegistrationResult::LoggingOff(LoggingOffSession::from_session(inner))
                }
                other => unreachable!(
                    "complete_new_user_registration leaves Onboarded or LoggingOff, got {other:?}"
                ),
            }),
            Err(error) => Err(Box::new((
                NewUserRegisteringSession::from_session(inner),
                error,
            ))),
        }
    }

    /// The registration effect over the raw session: hash, create,
    /// persist, transition. Split from [`Self::complete`] so the error
    /// path there can hand the still-registering session back to the
    /// caller.
    fn apply_complete(
        &self,
        session: &mut Session,
        profile: NewUserProfile,
        now: SystemTime,
    ) -> Result<(), CompleteNewUserRegistrationFlowError> {
        let kind = PasswordHashKind::Pbkdf210000;
        let computed = self.hasher.compute_password_hash(&profile.password, kind)?;
        let default_ratio = self.default_ratio;
        let draft = NewUserDraft {
            handle: profile.handle,
            location: profile.location,
            phone_number: profile.phone_number,
            email: profile.email,
            password_hash: computed.hash,
            password_salt: computed.salt,
            password_hash_kind: kind,
            line_length: profile.line_length,
            ansi_colour: profile.ansi_colour,
            flags: profile.flags,
            ratio_mode: default_ratio.mode,
            ratio_value: default_ratio.value,
            now,
        };
        let user = self.user_repo.create_user(draft)?;
        let rejection = session.complete_new_user_registration(user, self.policy, now)?;
        if let Some(entry) = rejection {
            self.caller_log.append(entry);
        }
        Ok(())
    }
}

/// Errors returned by [`complete_password_reset`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompletePasswordResetFlowError {
    /// The session is not at [`SessionState::Onboarded`], no user is
    /// bound, or `force_password_reset` isn't set.
    #[error(transparent)]
    Session(#[from] CompletePasswordResetError),
    /// The candidate password doesn't satisfy the configured length
    /// or category thresholds.
    #[error("candidate password is too weak")]
    WeakPassword,
    /// The candidate matches the user's current password. The spec
    /// requires the new password to differ from the old one.
    #[error("new password must differ from old")]
    SameAsCurrent,
    /// The hasher rejected the user's stored hash kind, or refused
    /// to compute a fresh hash for the spec's default kind.
    #[error(transparent)]
    Hash(#[from] PasswordError),
    /// The changed user record could not be persisted.
    #[error(transparent)]
    Save(#[from] UserRepositoryError),
}

/// Handles `session.allium:CompletePasswordReset`.
///
/// Runs the strength check (`min_password_length`,
/// `min_password_categories` from `policy`), verifies the candidate
/// differs from the user's current password via `hasher`, computes a
/// fresh hash with the spec's default
/// [`crate::domain::password::PasswordHashKind`], applies the
/// state mutation through [`Session::apply_password_change`], and
/// saves the updated user.
///
/// # Errors
/// Returns [`CompletePasswordResetFlowError::WeakPassword`] when
/// `candidate` doesn't pass [`meets_password_strength`],
/// [`CompletePasswordResetFlowError::SameAsCurrent`] when it matches
/// the existing password, [`CompletePasswordResetFlowError::Hash`]
/// when the hasher errors,
/// [`CompletePasswordResetFlowError::Session`] when the session
/// guards (state, user, flag) are wrong, or
/// [`CompletePasswordResetFlowError::Save`] when persistence fails.
pub fn complete_password_reset<R, H>(
    session: &mut Session,
    candidate: &str,
    user_repo: &R,
    hasher: &H,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<(), CompletePasswordResetFlowError>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
{
    if session.state() != SessionState::Onboarded {
        return Err(CompletePasswordResetError::WrongState(session.state()).into());
    }
    let user = session
        .user()
        .ok_or(CompletePasswordResetError::UserMissing)?;
    if !user.force_password_reset() {
        return Err(CompletePasswordResetError::ResetNotPending.into());
    }
    if !meets_password_strength(
        candidate,
        policy.min_password_length(),
        policy.min_password_categories(),
    ) {
        return Err(CompletePasswordResetFlowError::WeakPassword);
    }
    if hasher.verify_password(user, candidate)? {
        return Err(CompletePasswordResetFlowError::SameAsCurrent);
    }

    // Re-hash under the spec's current default kind, irrespective of
    // the user's previous storage kind. The legacy / lower-round
    // PBKDF2 variants will land in Slice 64 with their migration
    // story; for now there is exactly one supported kind.
    let kind = crate::domain::password::PasswordHashKind::Pbkdf210000;
    let computed = hasher.compute_password_hash(candidate, kind)?;

    apply_password_change(session, computed.hash, computed.salt, kind, now)?;
    save_bound_user(session, user_repo)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::domain::caller_log::CallerLog;
    use crate::domain::password::{ComputedHash, PasswordError, PasswordHashKind};
    use crate::domain::session::LogonChannel;
    use crate::domain::user::User;

    #[derive(Default)]
    struct TestLog {
        entries: Mutex<Vec<CallerLog>>,
    }

    impl CallerLogAppender for TestLog {
        fn append(&self, entry: CallerLog) {
            self.entries.lock().unwrap().push(entry);
        }
    }

    impl TestLog {
        fn entries(&self) -> Vec<CallerLog> {
            self.entries.lock().unwrap().clone()
        }
    }

    struct TestHasher {
        good_password: String,
    }

    impl PasswordHasher for TestHasher {
        fn verify_password(&self, _user: &User, candidate: &str) -> Result<bool, PasswordError> {
            Ok(candidate == self.good_password)
        }

        fn compute_password_hash(
            &self,
            candidate: &str,
            _kind: PasswordHashKind,
        ) -> Result<ComputedHash, PasswordError> {
            Ok(ComputedHash {
                hash: candidate.to_string(),
                salt: Some("test".to_string()),
            })
        }
    }

    struct TestRepo {
        users: Mutex<Vec<User>>,
    }

    impl UserRepository for TestRepo {
        fn find_by_handle(&self, typed: &str) -> Result<NameLookupResult, UserRepositoryError> {
            let users = self.users.lock().unwrap();
            if let Some(user) = users.iter().find(|u| u.handle() == typed) {
                Ok(NameLookupResult::Found(Box::new(user.clone())))
            } else {
                Ok(NameLookupResult::NotFound)
            }
        }

        fn find_sysop(&self) -> Result<NameLookupResult, UserRepositoryError> {
            let users = self.users.lock().unwrap();
            if let Some(user) = users.iter().find(|u| u.is_sysop()) {
                Ok(NameLookupResult::Found(Box::new(user.clone())))
            } else {
                Ok(NameLookupResult::NotFound)
            }
        }

        fn save(&self, user: User) -> Result<(), UserRepositoryError> {
            let mut users = self.users.lock().unwrap();
            let Some(existing) = users.iter_mut().find(|u| u.handle() == user.handle()) else {
                return Err(UserRepositoryError::UserNotFound {
                    handle: user.handle().to_string(),
                });
            };
            *existing = user;
            Ok(())
        }

        fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError> {
            let mut users = self.users.lock().unwrap();
            if users.iter().any(|u| u.handle() == draft.handle) {
                return Err(UserCreationError::DuplicateUser {
                    handle: draft.handle.clone(),
                });
            }
            let slot = users.iter().map(User::slot_number).max().unwrap_or(0) + 1;
            let user = User::register_new(slot, draft)?;
            users.push(user.clone());
            Ok(user)
        }
    }

    impl TestRepo {
        fn new(users: Vec<User>) -> Self {
            Self {
                users: Mutex::new(users),
            }
        }

        fn find_saved(&self, handle: &str) -> User {
            let users = self.users.lock().unwrap();
            users
                .iter()
                .find(|u| u.handle() == handle)
                .expect("saved user")
                .clone()
        }
    }

    fn alice() -> User {
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    fn session_identifying() -> Session {
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().unwrap();
        session
    }

    fn session_authenticating() -> Session {
        let mut session = session_identifying();
        session
            .record_identified_user("alice", alice())
            .expect("identified");
        session
    }

    fn good_hasher() -> TestHasher {
        TestHasher {
            good_password: "secret".to_string(),
        }
    }

    fn open_gate() -> NewUserGateConfig {
        NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        }
    }

    fn locked_gate() -> NewUserGateConfig {
        NewUserGateConfig {
            allow_new_users: false,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        }
    }

    fn password_gate(secret: &str) -> NewUserGateConfig {
        NewUserGateConfig {
            allow_new_users: true,
            new_user_password: Some(secret.to_string()),
            max_new_user_password_attempts: 3,
        }
    }

    fn identifying() -> IdentifyingSession {
        IdentifyingSession::from_session(session_identifying())
    }

    fn authenticating() -> AuthenticatingSession {
        AuthenticatingSession::from_session(session_authenticating())
    }

    /// A registering-phase wrapper with the new-user gate entered for
    /// `gate` (the `NEW` literal's effect, without the name prompt).
    fn registering_at(gate: &NewUserGateConfig) -> NewUserRegisteringSession {
        let mut s = session_identifying();
        s.record_new_user_request(
            gate.allow_new_users,
            gate.new_user_password.is_some(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("gate entry");
        NewUserRegisteringSession::from_session(s)
    }

    #[test]
    fn name_typed_found_binds_user() {
        let repo = TestRepo::new(vec![alice()]);
        let transition = name_typed(
            identifying(),
            "alice",
            &repo,
            &open_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("name lookup");

        let NameTypedTransition::Authenticated(auth) = transition else {
            panic!("expected Authenticated transition");
        };
        let session = auth.into_inner();
        assert_eq!(session.state(), SessionState::Authenticating);
        assert_eq!(
            session.user().map(crate::domain::user::User::handle),
            Some("alice")
        );
    }

    #[test]
    fn name_typed_new_with_open_gate_returns_initialised_no_password() {
        let repo = TestRepo::new(vec![alice()]);
        let transition = name_typed(
            identifying(),
            "NEW",
            &repo,
            &open_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("name typed");
        let NameTypedTransition::NewUserRegistering {
            session,
            password_required,
        } = transition
        else {
            panic!("expected NewUserRegistering transition");
        };
        assert!(!password_required);
        assert!(session.into_inner().new_user_password_verified());
    }

    #[test]
    fn name_typed_new_with_locked_gate_returns_disallowed() {
        let repo = TestRepo::new(vec![alice()]);
        let transition = name_typed(
            identifying(),
            "NEW",
            &repo,
            &locked_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("name typed");
        let NameTypedTransition::Disallowed(logging_off) = transition else {
            panic!("expected Disallowed transition");
        };
        assert_eq!(logging_off.into_inner().state(), SessionState::LoggingOff);
    }

    #[test]
    fn name_typed_new_with_password_gate_returns_initialised_required() {
        let repo = TestRepo::new(vec![alice()]);
        let transition = name_typed(
            identifying(),
            "NEW",
            &repo,
            &password_gate("letmein"),
            SystemTime::UNIX_EPOCH,
        )
        .expect("name typed");
        let NameTypedTransition::NewUserRegistering {
            session,
            password_required,
        } = transition
        else {
            panic!("expected NewUserRegistering transition");
        };
        assert!(password_required);
        assert!(!session.into_inner().new_user_password_verified());
    }

    #[test]
    fn verify_new_user_password_match_marks_verified() {
        let gate = password_gate("letmein");
        let log = TestLog::default();
        let transition = verify_new_user_password(
            registering_at(&gate),
            "LETMEIN", // case-insensitive parity with StriCmp
            &gate,
            &log,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert!(matches!(transition, NewUserPasswordTransition::Verified(_)));
        assert!(log.entries().is_empty());
    }

    #[test]
    fn verify_new_user_password_mismatch_logs_and_re_prompts() {
        let gate = password_gate("letmein");
        let log = TestLog::default();
        let transition = verify_new_user_password(
            registering_at(&gate),
            "wrong",
            &gate,
            &log,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert!(matches!(transition, NewUserPasswordTransition::Mismatch(_)));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].text.contains("New-user password failure"));
        assert!(entries[0].is_password_failure);
    }

    #[test]
    fn verify_new_user_password_three_failures_logs_off() {
        let gate = password_gate("letmein");
        let log = TestLog::default();
        let mut session = registering_at(&gate);
        for _ in 0..2 {
            session = match verify_new_user_password(
                session,
                "wrong",
                &gate,
                &log,
                SystemTime::UNIX_EPOCH,
            )
            .unwrap()
            {
                NewUserPasswordTransition::Mismatch(next) => next,
                _ => panic!("expected Mismatch before the budget is spent"),
            };
        }
        let transition =
            verify_new_user_password(session, "wrong", &gate, &log, SystemTime::UNIX_EPOCH)
                .unwrap();
        assert!(matches!(
            transition,
            NewUserPasswordTransition::TooManyFailures(_)
        ));
        assert_eq!(log.entries().len(), 3);
    }

    #[test]
    fn verify_password_mismatch_appends_password_failure_log() {
        let repo = TestRepo::new(vec![alice()]);
        let log = TestLog::default();
        let transition = verify_password(
            authenticating(),
            "wrong",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert!(matches!(
            transition,
            VerifyPasswordTransition::Authenticating(_)
        ));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_password_failure);
        assert_eq!(repo.find_saved("alice").invalid_attempts(), 1);
    }

    #[test]
    fn enter_menu_appends_logon_entry() {
        let repo = TestRepo::new(vec![alice()]);
        let log = TestLog::default();
        let VerifyPasswordTransition::Onboarded(onboarded) = verify_password(
            authenticating(),
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap() else {
            panic!("expected Onboarded transition");
        };

        let menu = enter_menu(onboarded, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(menu.into_inner().state(), SessionState::Menu);
        assert!(log.entries().iter().any(|e| e.text.contains("Logon:")));
        assert_eq!(repo.find_saved("alice").times_called(), 1);
    }

    #[test]
    fn verify_password_against_locked_user_appends_rejection_log_and_short_circuits() {
        let kind = PasswordHashKind::Pbkdf210000;
        let computed = good_hasher().compute_password_hash("secret", kind).unwrap();
        let mut user = User::new(
            2,
            "alice".to_string(),
            kind,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .unwrap();
        user.lock_account();
        let repo = TestRepo::new(vec![user]);
        // Bind the locked alice to the session.
        let saved = repo.find_saved("alice");
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", saved).unwrap();
        let log = TestLog::default();

        let transition = verify_password(
            AuthenticatingSession::from_session(session),
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        let VerifyPasswordTransition::LoggingOff { session, reason } = transition else {
            panic!("expected LoggingOff transition");
        };
        assert!(matches!(
            reason,
            VerifyPasswordRejectionReason::LogonRejected
        ));
        assert_eq!(session.into_inner().state(), SessionState::LoggingOff);
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0].text.contains("Logon rejected"),
            "expected rejection entry, got {entries:?}"
        );
    }

    #[test]
    fn verify_password_success_initialises_daily_budget() {
        let mut alice_with_limits = alice();
        alice_with_limits.set_time_limits(Duration::from_mins(30), Duration::from_hours(1));
        let repo = TestRepo::new(vec![alice_with_limits.clone()]);
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().unwrap();
        session
            .record_identified_user("alice", alice_with_limits)
            .unwrap();
        let log = TestLog::default();

        let VerifyPasswordTransition::Onboarded(onboarded) = verify_password(
            AuthenticatingSession::from_session(session),
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap() else {
            panic!("expected Onboarded transition");
        };

        let session = onboarded.into_inner();
        assert_eq!(session.time_remaining(), Duration::from_mins(30));
        // First-call-after-epoch: new-day branch zeroes today counters.
        assert_eq!(session.user().unwrap().times_called_today(), 0);
        assert_eq!(repo.find_saved("alice").times_called_today(), 0);
    }

    fn alice_with_reset_pending() -> User {
        // Build alice via the hasher so the "matches existing" check is
        // exercisable end-to-end against a real stored hash.
        let kind = PasswordHashKind::Pbkdf210000;
        let computed = good_hasher()
            .compute_password_hash("secret", kind)
            .expect("hash");
        let mut user = User::new(
            2,
            "alice".to_string(),
            kind,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user");
        user.set_force_password_reset(true);
        user
    }

    fn session_at_onboarded_with_reset_pending() -> Session {
        // Drive a session into Onboarded for an alice whose stored
        // hash matches the test hasher's "secret" credential and
        // whose force_password_reset flag is set. The default
        // SessionPolicy has password_expiry_days == 0, so the
        // ForcePasswordReset rule preserves the already-set flag
        // without needing to compute expiry.
        let user = alice_with_reset_pending();
        let mut s = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        s
    }

    #[test]
    fn complete_password_reset_happy_path_rotates_credentials() {
        let user = alice_with_reset_pending();
        let repo = TestRepo::new(vec![user.clone()]);
        let session = session_at_onboarded_with_reset_pending();
        // Confirm setup precondition.
        assert!(session.user().unwrap().force_password_reset());

        let mut session = session;
        let policy = SessionPolicy::default()
            .with_min_password_length(6)
            .with_min_password_categories(2);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        complete_password_reset(
            &mut session,
            "Newpass123",
            &repo,
            &good_hasher(),
            policy,
            later,
        )
        .expect("success");

        assert!(!session.user().unwrap().force_password_reset());
        assert_eq!(
            session.user().unwrap().password_last_updated(),
            later,
            "password_last_updated rolls forward to `now`"
        );
        let saved = repo.find_saved("alice");
        assert!(!saved.force_password_reset());
        assert_eq!(saved.password_last_updated(), later);
    }

    #[test]
    fn complete_password_reset_rejects_weak_password() {
        let user = alice_with_reset_pending();
        let repo = TestRepo::new(vec![user]);
        let mut session = session_at_onboarded_with_reset_pending();
        let policy = SessionPolicy::default().with_min_password_length(8);
        let err = complete_password_reset(
            &mut session,
            "short",
            &repo,
            &good_hasher(),
            policy,
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("weak should reject");
        assert!(matches!(err, CompletePasswordResetFlowError::WeakPassword));
        assert!(session.user().unwrap().force_password_reset());
    }

    #[test]
    fn complete_password_reset_rejects_same_as_current() {
        let user = alice_with_reset_pending();
        let repo = TestRepo::new(vec![user]);
        let mut session = session_at_onboarded_with_reset_pending();
        let err = complete_password_reset(
            &mut session,
            "secret",
            &repo,
            &good_hasher(),
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("same as old should reject");
        assert!(matches!(err, CompletePasswordResetFlowError::SameAsCurrent));
        assert!(session.user().unwrap().force_password_reset());
    }

    #[test]
    fn complete_password_reset_errors_when_flag_not_set() {
        let kind = PasswordHashKind::Pbkdf210000;
        let computed = good_hasher().compute_password_hash("secret", kind).unwrap();
        let user = User::new(
            2,
            "alice".to_string(),
            kind,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            100,
        )
        .unwrap();
        let repo = TestRepo::new(vec![user.clone()]);
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", user).unwrap();
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        // Flag NOT set on this user.
        let err = complete_password_reset(
            &mut session,
            "Newpass123",
            &repo,
            &good_hasher(),
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("flag not set");
        assert!(matches!(
            err,
            CompletePasswordResetFlowError::Session(CompletePasswordResetError::ResetNotPending)
        ));
    }

    fn registration_profile() -> NewUserProfile {
        NewUserProfile {
            handle: "newbie".to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555-0123".to_string()),
            email: Some("newbie@example.com".to_string()),
            password: "secret".to_string(),
            line_length: 80,
            ansi_colour: true,
            flags: BTreeSet::new(),
        }
    }

    fn default_ratio() -> DefaultRatio {
        DefaultRatio {
            mode: RatioMode::ByFiles,
            value: 3,
        }
    }

    fn session_at_new_user_registering() -> Session {
        let mut s = session_identifying();
        // Open gate (no password required) — verified set true on entry.
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        s
    }

    fn registration_flow<'a>(
        repo: &'a TestRepo,
        hasher: &'a TestHasher,
        log: &'a TestLog,
    ) -> NewUserRegistrationFlow<'a, TestRepo, TestHasher, TestLog> {
        NewUserRegistrationFlow::new(repo, hasher, log, default_ratio(), SessionPolicy::default())
    }

    #[test]
    fn complete_new_user_registration_creates_user_and_onboards() {
        let repo = TestRepo::new(vec![]);
        let log = TestLog::default();
        let hasher = good_hasher();
        let flow = registration_flow(&repo, &hasher, &log);
        let session = NewUserRegisteringSession::from_session(session_at_new_user_registering());
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let Ok(NewUserRegistrationResult::Onboarded(onboarded)) =
            flow.complete(session, registration_profile(), now)
        else {
            panic!("expected a clean Onboarded registration");
        };

        let session = onboarded.into_inner();
        assert_eq!(session.state(), SessionState::Onboarded);
        assert_eq!(
            session.user().map(crate::domain::user::User::handle),
            Some("newbie")
        );
        assert!(session.user().unwrap().is_new_user());
        assert_eq!(session.user().unwrap().slot_number(), 1);
        assert_eq!(session.user().unwrap().access_level(), 2);
        assert_eq!(session.user().unwrap().ratio_mode(), RatioMode::ByFiles);
        assert_eq!(session.time_remaining(), Duration::from_mins(30));

        // Repository carries the new account.
        match repo.find_by_handle("newbie").expect("lookup") {
            NameLookupResult::Found(user) => {
                assert!(user.is_new_user());
                assert_eq!(user.location(), Some("Townsville"));
                assert_eq!(user.email(), Some("newbie@example.com"));
            }
            NameLookupResult::NotFound => panic!("expected newbie to be created"),
        }
        // Fresh new user is not rejected, so no caller-log entry.
        assert!(log.entries().is_empty());
    }

    #[test]
    fn complete_new_user_registration_allocates_next_slot_above_existing_max() {
        let repo = TestRepo::new(vec![alice()]); // alice is slot 2
        let log = TestLog::default();
        let hasher = good_hasher();
        let flow = registration_flow(&repo, &hasher, &log);
        let session = NewUserRegisteringSession::from_session(session_at_new_user_registering());
        let Ok(NewUserRegistrationResult::Onboarded(onboarded)) =
            flow.complete(session, registration_profile(), SystemTime::UNIX_EPOCH)
        else {
            panic!("expected a clean Onboarded registration");
        };
        assert_eq!(onboarded.into_inner().user().unwrap().slot_number(), 3);
    }

    #[test]
    fn complete_new_user_registration_rejects_duplicate_handle() {
        let repo = TestRepo::new(vec![alice()]);
        let log = TestLog::default();
        let hasher = good_hasher();
        let flow = registration_flow(&repo, &hasher, &log);
        let session = NewUserRegisteringSession::from_session(session_at_new_user_registering());
        let mut profile = registration_profile();
        profile.handle = "alice".to_string();
        let Err(err) = flow.complete(session, profile, SystemTime::UNIX_EPOCH) else {
            panic!("duplicate handle should error");
        };
        let (session, error) = *err;
        assert!(matches!(
            error,
            CompleteNewUserRegistrationFlowError::Create(UserCreationError::DuplicateUser { .. })
        ));
        // The still-registering session comes back so the caller can
        // re-prompt; the wrapper type carries the state invariant.
        assert_eq!(
            session.into_inner().state(),
            SessionState::NewUserRegistering
        );
    }

    #[test]
    fn finalise_logoff_appends_logoff_entry() {
        let repo = TestRepo::new(vec![alice()]);
        let log = TestLog::default();
        let VerifyPasswordTransition::Onboarded(onboarded) = verify_password(
            authenticating(),
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap() else {
            panic!("expected Onboarded transition");
        };
        let menu = enter_menu(onboarded, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();
        let logging_off = menu.user_requests_logoff();

        let ended = finalise_logoff(logging_off, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(ended.into_inner().state(), SessionState::Ended);
        assert!(log.entries().iter().any(|e| e.text.contains("Logoff:")));
        assert_eq!(
            repo.find_saved("alice").last_call(),
            Some(SystemTime::UNIX_EPOCH)
        );
    }
}
