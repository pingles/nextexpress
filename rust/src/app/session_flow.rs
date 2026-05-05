//! Application use cases for driving a [`Session`].
//!
//! This module owns orchestration across domain ports. The [`Session`]
//! entity applies state changes and returns domain values; these
//! functions call repositories, hashers and logs around those pure
//! transitions.

use std::time::SystemTime;

use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::PasswordHasher;
use crate::domain::session::{
    EnterMenuError, NameTypedError, NameTypedOutcome, Session, SessionState,
    SessionTransitionError, VerifyPasswordError, VerifyPasswordOutcome,
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
/// entries when credentials do not match.
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
    max_password_failures: u32,
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
        let outcome = session
            .apply_password_match(now)
            .map_err(VerifyPasswordFlowError::Session)?;
        save_bound_user(session, user_repo).map_err(VerifyPasswordFlowError::Save)?;
        Ok(outcome)
    } else {
        let (outcome, entry) = session
            .apply_password_mismatch(max_password_failures, now)
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

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
            3,
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
            3,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        enter_menu(&mut session, &repo, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(session.state(), SessionState::Menu);
        assert!(log.entries().iter().any(|e| e.text.contains("Logon:")));
        assert_eq!(repo.find_saved("alice").times_called(), 1);
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
            3,
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
