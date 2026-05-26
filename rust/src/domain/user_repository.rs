//! [`UserRepository`] port (spec: `session.allium` user lookup and
//! persistence rules).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::{NewUserDraft, User, UserError};

/// Outcome of looking a typed handle up in the user database.
///
/// Mirrors `session.allium:NameLookupResult`. The `Found` variant
/// boxes the [`User`] payload so the enum stays small as new optional
/// fields land on `User`; this matters because the enum is returned
/// by every name lookup the BBS performs.
///
/// Application-level command literals such as the `NEW` registration
/// keyword are recognised by the login flow before the lookup reaches
/// the repository, so the port stays pure storage.
#[derive(Debug, Clone)]
pub enum NameLookupResult {
    /// A user with that handle exists. The repository returns the
    /// resolved record with the lookup result to avoid a second
    /// lookup/use race.
    Found(Box<User>),
    /// No user matches the typed handle.
    NotFound,
}

/// Errors returned by [`UserRepository::save`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserRepositoryError {
    /// The caller tried to save a user the repository does not know.
    #[error("user not found: {handle}")]
    UserNotFound {
        /// Handle on the user record that could not be saved.
        handle: String,
    },
}

/// Errors returned by [`UserRepository::create_user`].
///
/// Separates the two distinct failure modes — the domain constructor
/// rejecting the draft vs. the repository's own consistency checks —
/// so adapter implementations have an explicit contract for both.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserCreationError {
    /// The supplied draft failed [`User::register_new`]'s invariants
    /// (for example, a PBKDF2 hash kind paired with a missing salt).
    /// The repository performs no insertion in this case.
    #[error(transparent)]
    Build(#[from] UserError),
    /// A user with the same handle is already stored.
    #[error("user already exists: {handle}")]
    DuplicateUser {
        /// Handle that collided with an existing record.
        handle: String,
    },
}

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

    /// Returns the sysop record (slot 1; spec invariant
    /// `User.is_sysop: slot_number = 1`), or
    /// [`NameLookupResult::NotFound`] if no slot-1 user exists
    /// (e.g. an empty fresh-install store).
    ///
    /// Used by Slice 44's `C` (comment to sysop) command to resolve
    /// the addressee without leaning on the sysop's freely-renamable
    /// handle.
    fn find_sysop(&self) -> NameLookupResult;

    /// Persists a changed [`User`] record.
    ///
    /// # Errors
    /// Returns [`UserRepositoryError`] when the record cannot be saved.
    fn save(&self, user: User) -> Result<(), UserRepositoryError>;

    /// Atomically allocates the next unused slot number, constructs a
    /// [`User`] from `draft` via
    /// [`User::register_new`][crate::domain::user::User::register_new],
    /// and inserts the result.
    ///
    /// Implementations must hold whatever lock/transaction the
    /// underlying store requires so two concurrent registrations do not
    /// observe the same free slot.
    ///
    /// Returns the freshly persisted [`User`] on success.
    ///
    /// # Errors
    /// - [`UserCreationError::Build`] when the domain constructor
    ///   rejects `draft` (the repository has not modified its state).
    /// - [`UserCreationError::DuplicateUser`] when the draft's handle
    ///   is already taken.
    fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError>;
}
