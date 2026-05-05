//! In-memory [`UserRepository`] backed by a [`Vec`] of [`User`].
//!
//! This adapter is the only repository implementation in Phase 1. A
//! file-backed adapter is deferred until the on-disk format is firmed up
//! (see [`SLICES.md`](../../../SLICES.md)).

use crate::domain::user::User;
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// In-memory adapter seeded from a static [`Vec<User>`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryUserRepository {
    users: Vec<User>,
}

impl InMemoryUserRepository {
    /// Constructs a repository pre-populated with `users`.
    pub fn new(users: Vec<User>) -> Self {
        Self { users }
    }
}

impl UserRepository for InMemoryUserRepository {
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
        self.users.iter().find(|u| u.handle() == handle).cloned()
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
        assert_eq!(repo.lookup_name("alice"), NameLookupResult::Found);
    }

    #[test]
    fn unknown_handle_returns_not_found() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert_eq!(repo.lookup_name("bob"), NameLookupResult::NotFound);
    }

    #[test]
    fn literal_new_keyword_returns_user_typed_new() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert_eq!(repo.lookup_name("NEW"), NameLookupResult::UserTypedNew);
    }

    #[test]
    fn wildcard_input_does_not_glob_match() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert_eq!(repo.lookup_name("a*"), NameLookupResult::NotFound);
    }

    #[test]
    fn user_for_name_returns_matching_user() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        let user = repo.user_for_name("alice").expect("user present");
        assert_eq!(user.handle(), "alice");
    }

    #[test]
    fn user_for_name_returns_none_for_unknown() {
        let repo = InMemoryUserRepository::new(vec![user_with_handle(2, "alice")]);
        assert!(repo.user_for_name("bob").is_none());
    }
}
