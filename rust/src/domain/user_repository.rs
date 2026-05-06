//! [`UserRepository`] port (spec: `session.allium` user lookup and
//! persistence rules).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::User;

/// Outcome of looking a typed handle up in the user database.
///
/// Mirrors `session.allium:NameLookupResult`. The `Found` variant
/// boxes the [`User`] payload so the enum stays small as new optional
/// fields land on `User`; this matters because the enum is returned
/// by every name lookup the BBS performs.
#[derive(Debug, Clone)]
pub enum NameLookupResult {
    /// A user with that handle exists. The repository returns the
    /// resolved record with the lookup result to avoid a second
    /// lookup/use race.
    Found(Box<User>),
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
    /// The caller tried to create a user whose handle (or slot) is
    /// already taken.
    DuplicateUser {
        /// Handle that collided with an existing record.
        handle: String,
    },
}

impl std::fmt::Display for UserRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserNotFound { handle } => write!(f, "user not found: {handle}"),
            Self::DuplicateUser { handle } => write!(f, "user already exists: {handle}"),
        }
    }
}

impl std::error::Error for UserRepositoryError {}

/// Port over the user database.
///
/// Implementations are expected to reject inputs containing wildcards
/// (the legacy `AmiExpress` code rejects `'*'` early) by returning
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

    /// Allocates the next unused slot number
    /// (`session.allium:next_free_slot`).
    ///
    /// Returns one greater than the highest currently in-use slot, or
    /// `1` for a fresh repository. Callers use the returned slot
    /// immediately in a [`Self::create`] call; concurrent allocation
    /// is the implementation's concern.
    fn next_free_slot(&self) -> u32;

    /// Inserts a freshly registered [`User`] into the repository.
    ///
    /// Used by `session.allium:CompleteNewUserRegistration` (Slice 20)
    /// to persist the brand-new account.
    ///
    /// # Errors
    /// Returns [`UserRepositoryError::DuplicateUser`] when a user with
    /// the same handle is already present.
    fn create(&self, user: User) -> Result<(), UserRepositoryError>;
}
