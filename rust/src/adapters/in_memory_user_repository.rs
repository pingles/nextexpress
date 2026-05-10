//! In-memory [`UserRepository`] backed by a [`Vec`] of [`User`].
//!
//! This adapter is the only repository implementation in Phase 1. A
//! file-backed adapter is deferred until the on-disk format is firmed up
//! (see [`SLICES.md`](../../../SLICES.md)).

use std::sync::Mutex;

use crate::domain::user::User;
use crate::domain::user_repository::{
    BuildUserFn, NameLookupResult, UserCreationError, UserRepository, UserRepositoryError,
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
    fn find_by_handle(&self, typed: &str) -> NameLookupResult {
        if typed == "NEW" {
            return NameLookupResult::UserTypedNew;
        }
        let users = self.users.lock().expect("user repository mutex");
        if let Some(user) = users.iter().find(|u| u.handle() == typed) {
            NameLookupResult::Found(Box::new(user.clone()))
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

    fn allocate_slot_and_create(
        &self,
        build_user: BuildUserFn<'_>,
    ) -> Result<User, UserCreationError> {
        let mut users = self.users.lock().expect("user repository mutex");
        let slot = users.iter().map(User::slot_number).max().unwrap_or(0) + 1;
        let user = build_user(slot)?;
        if users.iter().any(|u| u.handle() == user.handle()) {
            return Err(UserCreationError::DuplicateUser {
                handle: user.handle().to_string(),
            });
        }
        if users.iter().any(|u| u.slot_number() == user.slot_number()) {
            return Err(UserCreationError::DuplicateSlot {
                slot: user.slot_number(),
            });
        }
        users.push(user.clone());
        Ok(user)
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

    fn build_with_handle(handle: &'static str) -> BuildUserFn<'static> {
        Box::new(move |slot| {
            User::new(
                slot,
                handle.to_string(),
                PasswordHashKind::Pbkdf210000,
                "hash".to_string(),
                Some("salt".to_string()),
                SystemTime::UNIX_EPOCH,
                100,
            )
        })
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

    #[test]
    fn allocate_slot_and_create_starts_at_one_when_empty() {
        let repo = InMemoryUserRepository::default();
        let user = repo
            .allocate_slot_and_create(build_with_handle("alice"))
            .expect("create");
        assert_eq!(user.slot_number(), 1);
    }

    #[test]
    fn allocate_slot_and_create_returns_one_above_max_used() {
        let repo = InMemoryUserRepository::new(vec![
            user_with_handle(1, "sysop"),
            user_with_handle(7, "alice"),
            user_with_handle(3, "bob"),
        ]);
        let user = repo
            .allocate_slot_and_create(build_with_handle("carol"))
            .expect("create");
        assert_eq!(user.slot_number(), 8);
    }

    #[test]
    fn allocate_slot_and_create_persists_user() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(1, "sysop")]);
        repo.allocate_slot_and_create(build_with_handle("alice"))
            .expect("create");
        assert!(matches!(
            repo.find_by_handle("alice"),
            NameLookupResult::Found(_)
        ));
    }

    #[test]
    fn allocate_slot_and_create_rejects_duplicate_handle() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(1, "alice")]);
        let err = repo
            .allocate_slot_and_create(build_with_handle("alice"))
            .expect_err("duplicate should error");
        assert_eq!(
            err,
            UserCreationError::DuplicateUser {
                handle: "alice".to_string()
            }
        );
    }

    #[test]
    fn allocate_slot_and_create_propagates_build_failure() {
        let repo = InMemoryUserRepository::default();
        let err = repo
            .allocate_slot_and_create(Box::new(|slot| {
                // PBKDF2 without a salt: should be rejected by `User::new`.
                User::new(
                    slot,
                    "alice".to_string(),
                    PasswordHashKind::Pbkdf210000,
                    "hash".to_string(),
                    None,
                    SystemTime::UNIX_EPOCH,
                    100,
                )
            }))
            .expect_err("build failure should propagate");
        assert!(matches!(err, UserCreationError::Build(_)));
        // No user should have been persisted.
        assert!(matches!(
            repo.find_by_handle("alice"),
            NameLookupResult::NotFound
        ));
    }
}
