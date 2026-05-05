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
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// Handles `session.allium:NameTyped`.
///
/// Looks up `typed` through `repo`, then applies the matching
/// [`Session`] transition.
///
/// # Errors
/// Returns [`NameTypedError::WrongState`] when `session` is not in
/// [`SessionState::Identifying`], or [`NameTypedError::UserDisappeared`]
/// when the repository reports a found user but cannot return the
/// corresponding record.
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

    match repo.lookup_name(typed) {
        NameLookupResult::Found => {
            let user = repo
                .user_for_name(typed)
                .ok_or(NameTypedError::UserDisappeared)?;
            session.record_identified_user(typed, user)
        }
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
pub fn verify_password<H, L>(
    session: &mut Session,
    candidate: &str,
    hasher: &H,
    caller_log: &L,
    max_password_failures: u32,
    now: SystemTime,
) -> Result<VerifyPasswordOutcome, VerifyPasswordError>
where
    H: PasswordHasher + ?Sized,
    L: CallerLogAppender + ?Sized,
{
    if session.state() != SessionState::Authenticating {
        return Err(VerifyPasswordError::WrongState(session.state()));
    }
    let user = session.user().ok_or(VerifyPasswordError::UserMissing)?;
    let matches = hasher
        .verify_password(user, candidate)
        .map_err(VerifyPasswordError::HashKindUnsupported)?;

    if matches {
        session.apply_password_match(now)
    } else {
        let (outcome, entry) = session.apply_password_mismatch(max_password_failures, now)?;
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
pub fn enter_menu<L>(
    session: &mut Session,
    caller_log: &L,
    now: SystemTime,
) -> Result<(), EnterMenuError>
where
    L: CallerLogAppender + ?Sized,
{
    let entry = session.enter_menu(now)?;
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
pub fn finalise_logoff<L>(
    session: &mut Session,
    caller_log: &L,
    now: SystemTime,
) -> Result<(), SessionTransitionError>
where
    L: CallerLogAppender + ?Sized,
{
    let entry = session.finalise_logoff(now)?;
    caller_log.append(entry);
    Ok(())
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
        users: Vec<User>,
        disappear_on_fetch: bool,
    }

    impl UserRepository for TestRepo {
        fn lookup_name(&self, typed: &str) -> NameLookupResult {
            if typed == "NEW" {
                return NameLookupResult::UserTypedNew;
            }
            if self.users.iter().any(|u| u.handle() == typed) {
                NameLookupResult::Found
            } else {
                NameLookupResult::NotFound
            }
        }

        fn user_for_name(&self, handle: &str) -> Option<User> {
            if self.disappear_on_fetch {
                return None;
            }
            self.users.iter().find(|u| u.handle() == handle).cloned()
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
        let repo = TestRepo {
            users: vec![alice()],
            disappear_on_fetch: false,
        };
        let mut session = session_identifying();
        let outcome = name_typed(&mut session, "alice", &repo, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(outcome, NameTypedOutcome::Authenticated);
        assert_eq!(session.state(), SessionState::Authenticating);
        assert_eq!(session.user().map(|u| u.handle()), Some("alice"));
    }

    #[test]
    fn name_typed_detects_user_disappeared() {
        let repo = TestRepo {
            users: vec![alice()],
            disappear_on_fetch: true,
        };
        let mut session = session_identifying();
        let error = name_typed(&mut session, "alice", &repo, SystemTime::UNIX_EPOCH)
            .expect_err("missing user should error");

        assert_eq!(error, NameTypedError::UserDisappeared);
    }

    #[test]
    fn verify_password_mismatch_appends_password_failure_log() {
        let mut session = session_authenticating();
        let log = TestLog::default();
        let outcome = verify_password(
            &mut session,
            "wrong",
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
    }

    #[test]
    fn enter_menu_appends_logon_entry() {
        let mut session = session_authenticating();
        let log = TestLog::default();
        verify_password(
            &mut session,
            "secret",
            &good_hasher(),
            &log,
            3,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        enter_menu(&mut session, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(session.state(), SessionState::Menu);
        assert!(log.entries().iter().any(|e| e.text.contains("Logon:")));
    }

    #[test]
    fn finalise_logoff_appends_logoff_entry() {
        let mut session = session_authenticating();
        let log = TestLog::default();
        verify_password(
            &mut session,
            "secret",
            &good_hasher(),
            &log,
            3,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        enter_menu(&mut session, &log, SystemTime::UNIX_EPOCH).unwrap();
        session.user_requests_logoff().unwrap();

        finalise_logoff(&mut session, &log, SystemTime::UNIX_EPOCH).unwrap();

        assert_eq!(session.state(), SessionState::Ended);
        assert!(log.entries().iter().any(|e| e.text.contains("Logoff:")));
    }
}
