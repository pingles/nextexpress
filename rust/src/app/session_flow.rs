//! Application use cases for driving a [`Session`].
//!
//! This module owns orchestration across domain ports. The [`Session`]
//! entity applies state changes and returns domain values; these
//! functions call repositories, hashers and logs around those pure
//! transitions.

use std::time::SystemTime;

use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::{meets_password_strength, PasswordError, PasswordHasher};
use crate::domain::session::{
    CompletePasswordResetError, EnterMenuError, NameTypedError, NameTypedOutcome, Session,
    SessionPolicy, SessionState, SessionTransitionError, VerifyPasswordError,
    VerifyPasswordOutcome,
};
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
/// [`Session`] transition.
///
/// # Errors
/// Returns [`NameTypedError::WrongState`] when `session` is not in
/// [`SessionState::Identifying`].
pub fn name_typed<R>(
    session: &mut Session,
    typed: &str,
    repo: &R,
    now: SystemTime,
) -> Result<NameTypedOutcome, NameTypedError>
where
    R: UserRepository + ?Sized,
{
    if session.state() != SessionState::Identifying {
        return Err(NameTypedError::WrongState(session.state()));
    }

    match repo.find_by_handle(typed) {
        NameLookupResult::Found(user) => session.record_identified_user(typed, user),
        NameLookupResult::NotFound => session.record_unknown_name(now),
        NameLookupResult::UserTypedNew => session.reject_new_user_request(),
    }
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

    use crate::domain::caller_log::CallerLog;
    use crate::domain::password::{ComputedHash, PasswordError, PasswordHashKind};
    use crate::domain::session::LogonChannel;
    use crate::domain::user::User;

    use super::*;

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
                NameLookupResult::Found(user.clone())
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

    #[test]
    fn name_typed_found_binds_user() {
        let repo = TestRepo::new(vec![alice()]);
        let mut session = session_identifying();
        let outcome = name_typed(&mut session, "alice", &repo, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(outcome, NameTypedOutcome::Authenticated);
        assert_eq!(session.state(), SessionState::Authenticating);
        assert_eq!(session.user().map(|u| u.handle()), Some("alice"));
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
