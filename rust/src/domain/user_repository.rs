//! [`UserRepository`] port (spec: `session.allium` user lookup and
//! persistence rules).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::User;

/// Outcome of looking a typed handle up in the user database.
///
/// Mirrors `session.allium:NameLookupResult`.
#[derive(Debug, Clone)]
pub enum NameLookupResult {
    /// A user with that handle exists. The repository returns the
    /// resolved record with the lookup result to avoid a second
    /// lookup/use race.
    Found(User),
    /// No user matches and the input was not the new-user literal.
    NotFound,
    /// The literal `NEW` — request to register a new account.
    UserTypedNew,
}

/// Errors returned by [`UserRepository`] implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRepositoryError {
    /// The caller tried to save a user the repository does not know.
    UserNotFound {
        /// Handle on the user record that could not be saved.
        handle: String,
    },
}

impl std::fmt::Display for UserRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserNotFound { handle } => write!(f, "user not found: {handle}"),
        }
    }
}

impl std::error::Error for UserRepositoryError {}

/// Port over the user database.
///
/// Implementations are expected to reject inputs containing wildcards
/// (the legacy AmiExpress code rejects `'*'` early) by returning
/// [`NameLookupResult::NotFound`].
pub trait UserRepository {
    /// Resolves `typed` to a [`NameLookupResult`].
    ///
    /// # Parameters
    /// - `typed`: handle exactly as the user typed it.
    fn find_by_handle(&self, typed: &str) -> NameLookupResult;

    /// Persists a changed [`User`] record.
    ///
    /// # Errors
    /// Returns [`UserRepositoryError`] when the record cannot be saved.
    fn save(&self, user: User) -> Result<(), UserRepositoryError>;
}
