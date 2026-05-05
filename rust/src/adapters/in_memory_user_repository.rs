//! In-memory [`UserRepository`] backed by a [`Vec`] of [`User`].
//!
//! This adapter is the only repository implementation in Phase 1. A
//! file-backed adapter is deferred until the on-disk format is firmed up
//! (see [`SLICES.md`](../../../SLICES.md)).

use std::sync::Mutex;

use crate::domain::user::User;
use crate::domain::user_repository::{NameLookupResult, UserRepository, UserRepositoryError};

/// In-memory adapter seeded from a static [`Vec<User>`].
#[derive(Debug, Default)]
pub struct InMemoryUserRepository {
    users: Mutex<Vec<User>>,
}

impl InMemoryUserRepository {
    /// Constructs a repository pre-populated with `users`.
    pub fn new(users: Vec<User>) -> Self {
        Self {
            users: Mutex::new(users),
        }
    }
}

impl UserRepository for InMemoryUserRepository {
    fn find_by_handle(&self, typed: &str) -> NameLookupResult {
        if typed == "NEW" {
            return NameLookupResult::UserTypedNew;
        }
        let users = self.users.lock().expect("user repository mutex");
        if let Some(user) = users.iter().find(|u| u.handle() == typed) {
            NameLookupResult::Found(user.clone())
        } else {
            NameLookupResult::NotFound
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
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::domain::password::PasswordHashKind;

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

    #[test]
    fn existing_handle_returns_found() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        match repo.find_by_handle("alice") {
            NameLookupResult::Found(user) => assert_eq!(user.handle(), "alice"),
            other => panic!("expected found, got {other:?}"),
        }
    }

    #[test]
    fn unknown_handle_returns_not_found() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("bob"),
            NameLookupResult::NotFound
        ));
    }

    #[test]
    fn literal_new_keyword_returns_user_typed_new() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("NEW"),
            NameLookupResult::UserTypedNew
        ));
    }

    #[test]
    fn wildcard_input_does_not_glob_match() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(matches!(
            repo.find_by_handle("a*"),
            NameLookupResult::NotFound
        ));
    }

    #[test]
    fn save_updates_matching_user() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        let mut user = user_with_handle(2, "alice");
        user.bump_times_called();
        repo.save(user).expect("save");

        match repo.find_by_handle("alice") {
            NameLookupResult::Found(user) => assert_eq!(user.times_called(), 1),
            other => panic!("expected found, got {other:?}"),
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
}
