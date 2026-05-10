//! [`UserRepository`] port (spec: `session.allium` user lookup and
//! persistence rules).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::{User, UserError};

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

/// Errors returned by [`UserRepository::allocate_slot_and_create`].
///
/// Separates the two distinct failure modes — the domain constructor
/// rejecting the record vs. the repository's own consistency checks —
/// so adapter implementations have an explicit contract for both.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserCreationError {
    /// The supplied build callback rejected the inputs (for example,
    /// a PBKDF2 hash kind paired with a missing salt). The repository
    /// performs no insertion in this case.
    #[error(transparent)]
    Build(#[from] UserError),
    /// A user with the same handle is already stored.
    #[error("user already exists: {handle}")]
    DuplicateUser {
        /// Handle that collided with an existing record.
        handle: String,
    },
    /// The slot number returned by the constructor is already used by
    /// another record. The repository allocates the slot itself, so
    /// this fires only when a constructor overrides the chosen slot or
    /// the adapter has a consistency bug.
    #[error("slot already in use: {slot}")]
    DuplicateSlot {
        /// Slot number that collided with an existing record.
        slot: u32,
    },
}

/// Callback handed to [`UserRepository::allocate_slot_and_create`].
///
/// Receives the slot number the repository has reserved and returns a
/// fully-constructed [`User`] (or a [`UserError`] if domain invariants
/// reject the inputs). Boxed so the trait stays object-safe — the
/// runtime composes the repository behind `Arc<dyn UserRepository>`.
pub type BuildUserFn<'a> = Box<dyn FnOnce(u32) -> Result<User, UserError> + Send + 'a>;

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

    /// Atomically allocates the next unused slot number, constructs a
    /// [`User`] by invoking `build_user(slot)`, and inserts the result.
    ///
    /// Implementations must hold whatever lock/transaction the
    /// underlying store requires so two concurrent registrations do not
    /// observe the same free slot. This replaces the previous
    /// `next_free_slot()` + `create()` pair, which exposed a race-prone
    /// contract.
    ///
    /// Returns the freshly persisted [`User`] on success.
    ///
    /// # Errors
    /// - [`UserCreationError::Build`] when `build_user` rejects the
    ///   inputs (the repository has not modified its state).
    /// - [`UserCreationError::DuplicateUser`] when the constructed
    ///   handle is already taken.
    /// - [`UserCreationError::DuplicateSlot`] when the constructor
    ///   returned a user whose `slot_number` collides with an existing
    ///   record (callers should normally take the slot offered to the
    ///   callback unchanged).
    fn allocate_slot_and_create(
        &self,
        build_user: BuildUserFn<'_>,
    ) -> Result<User, UserCreationError>;
}
