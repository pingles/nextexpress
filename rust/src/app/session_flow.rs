//! Application use cases for driving a [`Session`].
//!
//! This module owns orchestration across domain ports. The [`Session`]
//! entity applies state changes and returns domain values; these
//! functions call repositories, hashers and logs around those pure
//! transitions.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::{
    meets_password_strength, PasswordError, PasswordHashKind, PasswordHasher,
};
use crate::domain::session::{
    CompleteNewUserRegistrationError, CompletePasswordResetError, EnterMenuError, NameTypedError,
    NameTypedOutcome, NewUserPasswordOutcome, NewUserRequestOutcome, Session, SessionPolicy,
    SessionState, SessionTransitionError, VerifyNewUserPasswordError, VerifyPasswordError,
    VerifyPasswordOutcome,
};
use crate::domain::user::{NewUserRegistration, RatioMode, User, UserError, UserFlag};
use crate::domain::user_repository::{NameLookupResult, UserRepository, UserRepositoryError};

/// Errors returned by [`verify_password`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyPasswordFlowError {
    /// The underlying session rule failed.
    Session(VerifyPasswordError),
    /// The changed user record could not be persisted.
    Save(UserRepositoryError),
}

impl std::fmt::Display for VerifyPasswordFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for VerifyPasswordFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Save(error) => Some(error),
        }
    }
}

/// Errors returned by [`enter_menu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterMenuFlowError {
    /// The underlying session rule failed.
    Session(EnterMenuError),
    /// The changed user record could not be persisted.
    Save(UserRepositoryError),
}

impl std::fmt::Display for EnterMenuFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EnterMenuFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Save(error) => Some(error),
        }
    }
}

/// Errors returned by [`finalise_logoff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinaliseLogoffFlowError {
    /// The underlying session rule failed.
    Session(SessionTransitionError),
    /// The changed user record could not be persisted.
    Save(UserRepositoryError),
}

impl std::fmt::Display for FinaliseLogoffFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FinaliseLogoffFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Save(error) => Some(error),
        }
    }
}

/// Handles `session.allium:NameTyped`.
///
/// Looks up `typed` through `repo`, then applies the matching
/// [`Session`] transition. The `user_typed_NEW` branch additionally
/// fires the on-enter cluster for `new_user_registering`
/// (`RejectDisallowedRegistration` and `InitialiseNewUserGate`,
/// Slice 20a) using `gate`.
///
/// # Errors
/// Returns [`NameTypedError::WrongState`] when `session` is not in
/// [`SessionState::Identifying`].
pub fn name_typed<R>(
    session: &mut Session,
    typed: &str,
    repo: &R,
    gate: &NewUserGateConfig,
    now: SystemTime,
) -> Result<NameTypedOutcome, NameTypedError>
where
    R: UserRepository + ?Sized,
{
    if session.state() != SessionState::Identifying {
        return Err(NameTypedError::WrongState(session.state()));
    }

    match repo.find_by_handle(typed) {
        NameLookupResult::Found(user) => session.record_identified_user(typed, *user),
        NameLookupResult::NotFound => session.record_unknown_name(now),
        NameLookupResult::UserTypedNew => {
            let outcome = session.record_new_user_request(
                gate.allow_new_users,
                gate.new_user_password.is_some(),
                now,
            )?;
            Ok(match outcome {
                NewUserRequestOutcome::Initialised { password_required } => {
                    NameTypedOutcome::NewUserRegistering { password_required }
                }
                NewUserRequestOutcome::Rejected => NameTypedOutcome::NewUserRegistrationDisallowed,
            })
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyNewUserPasswordFlowError {
    /// The underlying session rule failed.
    Session(VerifyNewUserPasswordError),
    /// The gate configuration is missing — the caller invoked the
    /// gate flow even though `core/config.new_user_password` is
    /// `None`. The listener should never reach here in production;
    /// returning the error rather than silently passing protects
    /// against logic bugs.
    GateNotConfigured,
}

impl std::fmt::Display for VerifyNewUserPasswordFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::GateNotConfigured => {
                write!(f, "verify_new_user_password called with no gate configured")
            }
        }
    }
}

impl std::error::Error for VerifyNewUserPasswordFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::GateNotConfigured => None,
        }
    }
}

/// Handles `session.allium:VerifyNewUserPassword` (Slice 20a).
///
/// Compares `candidate` against `gate.new_user_password` under the
/// case-insensitive equality the legacy AmiExpress source uses
/// (`StriCmp` at `amiexpress/express.e:30027`), then applies the
/// resulting [`Session`] transition. On a mismatch the caller-log
/// "New-user password failure" entry is appended to `caller_log`; the
/// session may move to [`SessionState::LoggingOff`] when the attempt
/// counter reaches `gate.max_new_user_password_attempts`.
///
/// # Errors
/// Returns [`VerifyNewUserPasswordFlowError::Session`] when the
/// underlying session rule fails (wrong state, already verified) or
/// [`VerifyNewUserPasswordFlowError::GateNotConfigured`] when the
/// caller invoked the gate flow without a configured password.
pub fn verify_new_user_password<L>(
    session: &mut Session,
    candidate: &str,
    gate: &NewUserGateConfig,
    caller_log: &L,
    now: SystemTime,
) -> Result<NewUserPasswordOutcome, VerifyNewUserPasswordFlowError>
where
    L: CallerLogAppender + ?Sized,
{
    let secret = gate
        .new_user_password
        .as_deref()
        .ok_or(VerifyNewUserPasswordFlowError::GateNotConfigured)?;
    let matches = matches_new_user_password(candidate, secret);
    let (outcome, entry) = session
        .apply_new_user_password_attempt(matches, gate.max_new_user_password_attempts, now)
        .map_err(VerifyNewUserPasswordFlowError::Session)?;
    if let Some(entry) = entry {
        caller_log.append(entry);
    }
    Ok(outcome)
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
/// Returns [`VerifyPasswordError::WrongState`] when `session` is not in
/// [`SessionState::Authenticating`], [`VerifyPasswordError::UserMissing`]
/// when no user is bound, or
/// [`VerifyPasswordError::HashKindUnsupported`] when the hasher rejects
/// the stored password kind.
pub fn verify_password<R, H, L>(
    session: &mut Session,
    candidate: &str,
    user_repo: &R,
    hasher: &H,
    caller_log: &L,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<VerifyPasswordOutcome, VerifyPasswordFlowError>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    if session.state() != SessionState::Authenticating {
        return Err(VerifyPasswordFlowError::Session(
            VerifyPasswordError::WrongState(session.state()),
        ));
    }
    let user = session.user().ok_or(VerifyPasswordFlowError::Session(
        VerifyPasswordError::UserMissing,
    ))?;
    let matches = hasher.verify_password(user, candidate).map_err(|error| {
        VerifyPasswordFlowError::Session(VerifyPasswordError::HashKindUnsupported(error))
    })?;

    if matches {
        let (outcome, rejection) = session
            .apply_password_match(policy, now)
            .map_err(VerifyPasswordFlowError::Session)?;
        save_bound_user(session, user_repo).map_err(VerifyPasswordFlowError::Save)?;
        if let Some(entry) = rejection {
            caller_log.append(entry);
        }
        Ok(outcome)
    } else {
        let (outcome, entry) = session
            .apply_password_mismatch(policy, now)
            .map_err(VerifyPasswordFlowError::Session)?;
        save_bound_user(session, user_repo).map_err(VerifyPasswordFlowError::Save)?;
        caller_log.append(entry);
        Ok(outcome)
    }
}

/// Handles `session.allium:EnterMenu`.
///
/// Applies the domain transition and appends the resulting logon caller
/// log entry.
///
/// # Errors
/// Returns [`EnterMenuError`] when the session cannot enter the menu.
pub fn enter_menu<R, L>(
    session: &mut Session,
    user_repo: &R,
    caller_log: &L,
    now: SystemTime,
) -> Result<(), EnterMenuFlowError>
where
    R: UserRepository + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    let entry = session
        .enter_menu(now)
        .map_err(EnterMenuFlowError::Session)?;
    save_bound_user(session, user_repo).map_err(EnterMenuFlowError::Save)?;
    caller_log.append(entry);
    Ok(())
}

/// Handles `session.allium:FinaliseLogoff`.
///
/// Applies the domain transition and appends the resulting logoff caller
/// log entry.
///
/// # Errors
/// Returns [`SessionTransitionError`] when the session is not logging off.
pub fn finalise_logoff<R, L>(
    session: &mut Session,
    user_repo: &R,
    caller_log: &L,
    now: SystemTime,
) -> Result<(), FinaliseLogoffFlowError>
where
    R: UserRepository + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    let entry = session
        .finalise_logoff(now)
        .map_err(FinaliseLogoffFlowError::Session)?;
    save_bound_user(session, user_repo).map_err(FinaliseLogoffFlowError::Save)?;
    caller_log.append(entry);
    Ok(())
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
/// [`complete_new_user_registration`] from `password`; the slot number
/// is allocated from the user repository.
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

/// Errors returned by [`complete_new_user_registration`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteNewUserRegistrationFlowError {
    /// The session is not in
    /// [`SessionState::NewUserRegistering`].
    Session(CompleteNewUserRegistrationError),
    /// The hasher failed to compute a hash for the supplied password.
    Hash(PasswordError),
    /// `User::register_new` rejected the constructed record (e.g. a
    /// PBKDF2 hash kind without a salt — should never happen for the
    /// configured default).
    User(UserError),
    /// The repository couldn't allocate or persist the new account
    /// (e.g. the chosen handle is already taken).
    Save(UserRepositoryError),
}

impl std::fmt::Display for CompleteNewUserRegistrationFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::Hash(error) => write!(f, "{error}"),
            Self::User(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CompleteNewUserRegistrationFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::Hash(error) => Some(error),
            Self::User(error) => Some(error),
            Self::Save(error) => Some(error),
        }
    }
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

/// Handles `session.allium:CompleteNewUserRegistration` (Slice 20).
///
/// Allocates a slot from the user repository, hashes the supplied
/// password under the spec's default
/// [`PasswordHashKind`], constructs a [`User`] via
/// [`User::register_new`], persists it, and applies the corresponding
/// session transition through
/// [`Session::complete_new_user_registration`]. The resulting
/// `RejectLockedOrInsufficientAccess` caller-log entry (if any) is
/// appended to `caller_log` so the post-onboarded cluster matches the
/// password-match path's behaviour.
///
/// # Errors
/// Returns [`CompleteNewUserRegistrationFlowError`] when the session
/// is in the wrong state, the hasher errors, the chosen handle
/// collides with an existing user, or the constructed record fails
/// [`User::register_new`]'s invariants.
#[allow(clippy::too_many_arguments)]
pub fn complete_new_user_registration<R, H, L>(
    session: &mut Session,
    profile: NewUserProfile,
    user_repo: &R,
    hasher: &H,
    caller_log: &L,
    default_ratio: DefaultRatio,
    policy: SessionPolicy,
    now: SystemTime,
) -> Result<(), CompleteNewUserRegistrationFlowError>
where
    R: UserRepository + ?Sized,
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    if session.state() != SessionState::NewUserRegistering {
        return Err(CompleteNewUserRegistrationFlowError::Session(
            CompleteNewUserRegistrationError::WrongState(session.state()),
        ));
    }
    let kind = PasswordHashKind::Pbkdf210000;
    let computed = hasher
        .compute_password_hash(&profile.password, kind)
        .map_err(CompleteNewUserRegistrationFlowError::Hash)?;
    let user = User::register_new(NewUserRegistration {
        slot_number: user_repo.next_free_slot(),
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
    })
    .map_err(CompleteNewUserRegistrationFlowError::User)?;
    user_repo
        .create(user.clone())
        .map_err(CompleteNewUserRegistrationFlowError::Save)?;
    let rejection = session
        .complete_new_user_registration(user, policy, now)
        .map_err(CompleteNewUserRegistrationFlowError::Session)?;
    if let Some(entry) = rejection {
        caller_log.append(entry);
    }
    Ok(())
}

/// Errors returned by [`complete_password_reset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletePasswordResetFlowError {
    /// The session is not at [`SessionState::Onboarded`], no user is
    /// bound, or `force_password_reset` isn't set.
    Session(CompletePasswordResetError),
    /// The candidate password doesn't satisfy the configured length
    /// or category thresholds.
    WeakPassword,
    /// The candidate matches the user's current password. The spec
    /// requires the new password to differ from the old one.
    SameAsCurrent,
    /// The hasher rejected the user's stored hash kind, or refused
    /// to compute a fresh hash for the spec's default kind.
    Hash(PasswordError),
    /// The changed user record could not be persisted.
    Save(UserRepositoryError),
}

impl std::fmt::Display for CompletePasswordResetFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "{error}"),
            Self::WeakPassword => write!(f, "candidate password is too weak"),
            Self::SameAsCurrent => write!(f, "new password must differ from old"),
            Self::Hash(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CompletePasswordResetFlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::WeakPassword | Self::SameAsCurrent => None,
            Self::Hash(error) => Some(error),
            Self::Save(error) => Some(error),
        }
    }
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
        return Err(CompletePasswordResetFlowError::Session(
            CompletePasswordResetError::WrongState(session.state()),
        ));
    }
    let user = session
        .user()
        .ok_or(CompletePasswordResetFlowError::Session(
            CompletePasswordResetError::UserMissing,
        ))?;
    if !user.force_password_reset() {
        return Err(CompletePasswordResetFlowError::Session(
            CompletePasswordResetError::ResetNotPending,
        ));
    }
    if !meets_password_strength(
        candidate,
        policy.min_password_length(),
        policy.min_password_categories(),
    ) {
        return Err(CompletePasswordResetFlowError::WeakPassword);
    }
    let same_as_current = hasher
        .verify_password(user, candidate)
        .map_err(CompletePasswordResetFlowError::Hash)?;
    if same_as_current {
        return Err(CompletePasswordResetFlowError::SameAsCurrent);
    }

    // Re-hash under the spec's current default kind, irrespective of
    // the user's previous storage kind. The legacy / lower-round
    // PBKDF2 variants will land in Slice 64 with their migration
    // story; for now there is exactly one supported kind.
    let kind = crate::domain::password::PasswordHashKind::Pbkdf210000;
    let computed = hasher
        .compute_password_hash(candidate, kind)
        .map_err(CompletePasswordResetFlowError::Hash)?;

    session
        .apply_password_change(computed.hash, computed.salt, kind, now)
        .map_err(CompletePasswordResetFlowError::Session)?;
    save_bound_user(session, user_repo).map_err(CompletePasswordResetFlowError::Save)?;
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
        fn find_by_handle(&self, typed: &str) -> NameLookupResult {
            if typed == "NEW" {
                return NameLookupResult::UserTypedNew;
            }
            let users = self.users.lock().unwrap();
            if let Some(user) = users.iter().find(|u| u.handle() == typed) {
                NameLookupResult::Found(Box::new(user.clone()))
            } else {
                NameLookupResult::NotFound
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

        fn next_free_slot(&self) -> u32 {
            let users = self.users.lock().unwrap();
            users.iter().map(User::slot_number).max().unwrap_or(0) + 1
        }

        fn create(&self, user: User) -> Result<(), UserRepositoryError> {
            let mut users = self.users.lock().unwrap();
            if users.iter().any(|u| u.handle() == user.handle()) {
                return Err(UserRepositoryError::DuplicateUser {
                    handle: user.handle().to_string(),
                });
            }
            users.push(user);
            Ok(())
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

    #[test]
    fn name_typed_found_binds_user() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_identifying();
        let outcome = name_typed(
            &mut session,
            "alice",
            &repo,
            &open_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert_eq!(outcome, NameTypedOutcome::Authenticated);
        assert_eq!(session.state(), SessionState::Authenticating);
        assert_eq!(session.user().map(|u| u.handle()), Some("alice"));
    }

    #[test]
    fn name_typed_new_with_open_gate_returns_initialised_no_password() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_identifying();
        let outcome = name_typed(
            &mut session,
            "NEW",
            &repo,
            &open_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(
            outcome,
            NameTypedOutcome::NewUserRegistering {
                password_required: false
            }
        );
        assert!(session.new_user_password_verified());
    }

    #[test]
    fn name_typed_new_with_locked_gate_returns_disallowed() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_identifying();
        let outcome = name_typed(
            &mut session,
            "NEW",
            &repo,
            &locked_gate(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(outcome, NameTypedOutcome::NewUserRegistrationDisallowed);
        assert_eq!(session.state(), SessionState::LoggingOff);
    }

    #[test]
    fn name_typed_new_with_password_gate_returns_initialised_required() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_identifying();
        let outcome = name_typed(
            &mut session,
            "NEW",
            &repo,
            &password_gate("letmein"),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(
            outcome,
            NameTypedOutcome::NewUserRegistering {
                password_required: true
            }
        );
        assert!(!session.new_user_password_verified());
    }

    #[test]
    fn verify_new_user_password_match_marks_verified() {
        let mut session = session_identifying();
        name_typed(
            &mut session,
            "NEW",
            &TestRepo::new(vec![]),
            &password_gate("letmein"),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let log = TestLog::default();
        let outcome = verify_new_user_password(
            &mut session,
            "LETMEIN", // case-insensitive parity with StriCmp
            &password_gate("letmein"),
            &log,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Verified);
        assert!(log.entries().is_empty());
    }

    #[test]
    fn verify_new_user_password_mismatch_logs_and_re_prompts() {
        let mut session = session_identifying();
        name_typed(
            &mut session,
            "NEW",
            &TestRepo::new(vec![]),
            &password_gate("letmein"),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let log = TestLog::default();
        let outcome = verify_new_user_password(
            &mut session,
            "wrong",
            &password_gate("letmein"),
            &log,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Mismatch);
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].text.contains("New-user password failure"));
        assert!(entries[0].is_password_failure);
    }

    #[test]
    fn verify_new_user_password_three_failures_logs_off() {
        let mut session = session_identifying();
        name_typed(
            &mut session,
            "NEW",
            &TestRepo::new(vec![]),
            &password_gate("letmein"),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let log = TestLog::default();
        for _ in 0..3 {
            verify_new_user_password(
                &mut session,
                "wrong",
                &password_gate("letmein"),
                &log,
                SystemTime::UNIX_EPOCH,
            )
            .unwrap();
        }
        assert_eq!(session.state(), SessionState::LoggingOff);
        assert_eq!(log.entries().len(), 3);
    }

    #[test]
    fn verify_password_mismatch_appends_password_failure_log() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_authenticating();
        let log = TestLog::default();
        let outcome = verify_password(
            &mut session,
            "wrong",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert_eq!(outcome, VerifyPasswordOutcome::NotMatching);
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_password_failure);
        assert_eq!(repo.find_saved("alice").invalid_attempts(), 1);
    }

    #[test]
    fn enter_menu_appends_logon_entry() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_authenticating();
        let log = TestLog::default();
        verify_password(
            &mut session,
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        enter_menu(&mut session, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(session.state(), SessionState::Menu);
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

        let outcome = verify_password(
            &mut session,
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert_eq!(outcome, VerifyPasswordOutcome::LogonRejected);
        assert_eq!(session.state(), SessionState::LoggingOff);
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
        alice_with_limits
            .set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        let repo = TestRepo::new(vec![alice_with_limits.clone()]);
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().unwrap();
        session
            .record_identified_user("alice", alice_with_limits)
            .unwrap();
        let log = TestLog::default();

        verify_password(
            &mut session,
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert_eq!(session.time_remaining(), Duration::from_secs(30 * 60));
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
        s.apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .unwrap();
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
        session
            .apply_password_match(SessionPolicy::default(), SystemTime::UNIX_EPOCH)
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

    #[test]
    fn complete_new_user_registration_creates_user_and_onboards() {
        let repo = TestRepo::new(vec![]);
        let log = TestLog::default();
        let mut session = session_at_new_user_registering();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        complete_new_user_registration(
            &mut session,
            registration_profile(),
            &repo,
            &good_hasher(),
            &log,
            default_ratio(),
            SessionPolicy::default(),
            now,
        )
        .expect("registration");

        assert_eq!(session.state(), SessionState::Onboarded);
        assert_eq!(session.user().map(|u| u.handle()), Some("newbie"));
        assert!(session.user().unwrap().is_new_user());
        assert_eq!(session.user().unwrap().slot_number(), 1);
        assert_eq!(session.user().unwrap().access_level(), 2);
        assert_eq!(session.user().unwrap().ratio_mode(), RatioMode::ByFiles);
        assert_eq!(session.time_remaining(), Duration::from_secs(30 * 60));

        // Repository carries the new account.
        match repo.find_by_handle("newbie") {
            NameLookupResult::Found(user) => {
                assert!(user.is_new_user());
                assert_eq!(user.location(), Some("Townsville"));
                assert_eq!(user.email(), Some("newbie@example.com"));
            }
            other => panic!("expected found, got {other:?}"),
        }
        // Fresh new user is not rejected, so no caller-log entry.
        assert!(log.entries().is_empty());
    }

    #[test]
    fn complete_new_user_registration_allocates_next_slot_above_existing_max() {
        let repo = TestRepo::new(vec![alice()]); // alice is slot 2
        let log = TestLog::default();
        let mut session = session_at_new_user_registering();
        complete_new_user_registration(
            &mut session,
            registration_profile(),
            &repo,
            &good_hasher(),
            &log,
            default_ratio(),
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("registration");
        assert_eq!(session.user().unwrap().slot_number(), 3);
    }

    #[test]
    fn complete_new_user_registration_rejects_duplicate_handle() {
        let repo = TestRepo::new(vec![alice()]);
        let log = TestLog::default();
        let mut session = session_at_new_user_registering();
        let mut profile = registration_profile();
        profile.handle = "alice".to_string();
        let err = complete_new_user_registration(
            &mut session,
            profile,
            &repo,
            &good_hasher(),
            &log,
            default_ratio(),
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("duplicate handle should error");
        assert!(matches!(
            err,
            CompleteNewUserRegistrationFlowError::Save(UserRepositoryError::DuplicateUser { .. })
        ));
        // Session stays in NewUserRegistering so the caller can re-prompt.
        assert_eq!(session.state(), SessionState::NewUserRegistering);
    }

    #[test]
    fn complete_new_user_registration_outside_new_user_registering_errors() {
        let repo = TestRepo::new(vec![]);
        let log = TestLog::default();
        let mut session = session_identifying();
        let err = complete_new_user_registration(
            &mut session,
            registration_profile(),
            &repo,
            &good_hasher(),
            &log,
            default_ratio(),
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("must be in new_user_registering");
        assert!(matches!(
            err,
            CompleteNewUserRegistrationFlowError::Session(
                CompleteNewUserRegistrationError::WrongState(_)
            )
        ));
    }

    #[test]
    fn finalise_logoff_appends_logoff_entry() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_authenticating();
        let log = TestLog::default();
        verify_password(
            &mut session,
            "secret",
            &repo,
            &good_hasher(),
            &log,
            SessionPolicy::new(3),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        enter_menu(&mut session, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();
        session.user_requests_logoff().unwrap();

        finalise_logoff(&mut session, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(session.state(), SessionState::Ended);
        assert!(log.entries().iter().any(|e| e.text.contains("Logoff:")));
        assert_eq!(
            repo.find_saved("alice").last_call(),
            Some(SystemTime::UNIX_EPOCH)
        );
    }
}
