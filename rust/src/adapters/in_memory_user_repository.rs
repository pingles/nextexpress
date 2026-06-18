//! In-memory [`UserRepository`] backed by a [`Vec`] of [`User`].
//!
//! This adapter is the only repository implementation in Phase 1. A
//! file-backed adapter is deferred until the on-disk format is firmed up
//! (see [`SLICES.md`](../../../SLICES.md)).

use std::sync::Mutex;

use crate::domain::user::{NewUserDraft, User};
use crate::domain::user_repository::{
    NameLookupResult, UserCreationError, UserRepository, UserRepositoryError,
};

/// In-memory adapter seeded from a static [`Vec<User>`].
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: Mutex<Vec<User>>,
}

impl InMemoryUserRepository {
    /// Constructs a repository pre-populated with `users`.
    #[must_use]
    pub fn new(users: Vec<User>) -> Self {
        Self {
            users: Mutex::new(users),
        }
    }
}

impl UserRepository for InMemoryUserRepository {
    fn find_by_handle(&self, typed: &str) -> Result<NameLookupResult, UserRepositoryError> {
        let users = self.users.lock().expect("user repository mutex");
        if let Some(user) = users.iter().find(|u| u.handle() == typed) {
            Ok(NameLookupResult::Found(Box::new(user.clone())))
        } else {
            Ok(NameLookupResult::NotFound)
        }
    }

    fn find_sysop(&self) -> Result<NameLookupResult, UserRepositoryError> {
        let users = self.users.lock().expect("user repository mutex");
        if let Some(user) = users.iter().find(|u| u.is_sysop()) {
            Ok(NameLookupResult::Found(Box::new(user.clone())))
        } else {
            Ok(NameLookupResult::NotFound)
        }
    }

    fn save(&self, user: User) -> Result<(), UserRepositoryError> {
        let mut users = self.users.lock().expect("user repository mutex");
        let Some(existing) = users.iter_mut().find(|u| u.handle() == user.handle()) else {
            return Err(UserRepositoryError::UserNotFound {
                handle: user.handle().to_string(),
            });
        };
        *existing = user;
        Ok(())
    }

    fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError> {
        let mut users = self.users.lock().expect("user repository mutex");
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::SystemTime;

    use super::*;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::user::RatioMode;

    fn user_with_handle(slot: u32, handle: &str) -> User {
        User::new(
            slot,
            handle.to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    fn draft_with_handle(handle: &str) -> NewUserDraft {
        NewUserDraft {
            handle: handle.to_string(),
            location: None,
            phone_number: None,
            email: None,
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 0,
            ansi_colour: false,
            flags: BTreeSet::new(),
            ratio_mode: RatioMode::Disabled,
            ratio_value: 0,
            now: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn existing_handle_returns_found() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(user) => assert_eq!(user.handle(), "alice"),
            NameLookupResult::NotFound => panic!("expected found, got not-found"),
        }
    }

    #[test]
    fn unknown_handle_returns_not_found() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("bob"),
            Ok(NameLookupResult::NotFound)
        ));
    }

    #[test]
    fn sysop_lookup_returns_found_for_slot_one() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(1, "sysop")]);
        assert!(matches!(repo.find_sysop(), Ok(NameLookupResult::Found(_))));
    }

    #[test]
    fn sysop_lookup_returns_not_found_when_absent() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(repo.find_sysop(), Ok(NameLookupResult::NotFound)));
    }

    #[test]
    fn literal_new_keyword_is_pure_storage_lookup() {
        // The `NEW` registration literal is recognised by the login
        // flow before reaching the repository, so the port treats it
        // as any other handle.
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("NEW"),
            Ok(NameLookupResult::NotFound)
        ));
    }

    #[test]
    fn wildcard_input_does_not_glob_match() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("a*"),
            Ok(NameLookupResult::NotFound)
        ));
    }

    #[test]
    fn save_updates_matching_user() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        let mut user = user_with_handle(2, "alice");
        user.bump_times_called();
        repo.save(user).expect("save");

        match repo.find_by_handle("alice").expect("lookup") {
            NameLookupResult::Found(user) => assert_eq!(user.times_called(), 1),
            NameLookupResult::NotFound => panic!("expected found, got not-found"),
        }
    }

    #[test]
    fn save_unknown_user_errors() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        let error = repo
            .save(user_with_handle(3, "bob"))
            .expect_err("unknown user should error");
        assert_eq!(
            error,
            UserRepositoryError::UserNotFound {
                handle: "bob".to_string()
            }
        );
    }

    #[test]
    fn create_user_starts_at_slot_one_when_empty() {
        let repo = InMemoryUserRepository::default();
        let user = repo
            .create_user(draft_with_handle("alice"))
            .expect("create");
        assert_eq!(user.slot_number(), 1);
    }

    #[test]
    fn create_user_returns_one_above_max_used() {
        let repo = InMemoryUserRepository::new(vec![
            user_with_handle(1, "sysop"),
            user_with_handle(7, "alice"),
            user_with_handle(3, "bob"),
        ]);
        let user = repo
            .create_user(draft_with_handle("carol"))
            .expect("create");
        assert_eq!(user.slot_number(), 8);
    }

    #[test]
    fn create_user_persists_user() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(1, "sysop")]);
        repo.create_user(draft_with_handle("alice"))
            .expect("create");
        assert!(matches!(
            repo.find_by_handle("alice"),
            Ok(NameLookupResult::Found(_))
        ));
    }

    #[test]
    fn create_user_rejects_duplicate_handle() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(1, "alice")]);
        let err = repo
            .create_user(draft_with_handle("alice"))
            .expect_err("duplicate should error");
        assert_eq!(
            err,
            UserCreationError::DuplicateUser {
                handle: "alice".to_string()
            }
        );
    }

    #[test]
    fn create_user_propagates_register_new_failure() {
        let repo = InMemoryUserRepository::default();
        let mut draft = draft_with_handle("alice");
        // PBKDF2 without a salt: rejected by `User::register_new`.
        draft.password_salt = None;
        let err = repo
            .create_user(draft)
            .expect_err("build failure should propagate");
        assert!(matches!(err, UserCreationError::Build(_)));
        // No user should have been persisted.
        assert!(matches!(
            repo.find_by_handle("alice"),
            Ok(NameLookupResult::NotFound)
        ));
    }
}
