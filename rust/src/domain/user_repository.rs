//! [`UserRepository`] port (spec: `session.allium` user lookup and
//! persistence rules).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::{AuthOutcome, NewUserDraft, PasswordChange, User, UserError, UserPatch};

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

/// Errors returned by the [`UserRepository`] write commands.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserRepositoryError {
    /// The caller tried to save a user the repository does not know.
    #[error("user not found: {handle}")]
    UserNotFound {
        /// Handle on the user record that could not be saved.
        handle: String,
    },
    /// The backing store failed while running a repository operation.
    #[error("user repository storage error during {context}: {message}")]
    Storage {
        /// Short operation label for logs and tests.
        context: &'static str,
        /// Backend error message stripped of backend-specific types.
        message: String,
    },
}

impl UserRepositoryError {
    /// Builds a storage error from an adapter-specific error value.
    ///
    /// # Parameters
    /// - `context`: short operation label such as `"lookup"` or `"save"`.
    /// - `error`: backend error being translated at the adapter boundary.
    ///
    /// # Returns
    /// A repository error that preserves the failure as an operational
    /// storage fault rather than a normal domain outcome.
    #[must_use]
    pub fn storage(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Storage {
            context,
            message: error.to_string(),
        }
    }
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
    /// The backing store failed while allocating or persisting the user.
    #[error("user creation storage error during {context}: {message}")]
    Storage {
        /// Short operation label for logs and tests.
        context: &'static str,
        /// Backend error message stripped of backend-specific types.
        message: String,
    },
}

impl UserCreationError {
    /// Builds a creation storage error from an adapter-specific error
    /// value.
    ///
    /// # Parameters
    /// - `context`: short operation label such as `"allocate slot"` or
    ///   `"insert user"`.
    /// - `error`: backend error being translated at the adapter boundary.
    ///
    /// # Returns
    /// A creation error that preserves the failure as an operational
    /// storage fault rather than a domain-constructor failure.
    #[must_use]
    pub fn storage(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Storage {
            context,
            message: error.to_string(),
        }
    }
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
    ///
    /// # Errors
    /// Returns [`UserRepositoryError`] when the backing store cannot be
    /// queried or the stored row cannot be decoded into a [`User`].
    fn find_by_handle(&self, typed: &str) -> Result<NameLookupResult, UserRepositoryError>;

    /// Returns the sysop record (slot 1; spec invariant
    /// `User.is_sysop: slot_number = 1`), or
    /// [`NameLookupResult::NotFound`] if no slot-1 user exists
    /// (e.g. an empty fresh-install store).
    ///
    /// Used by Slice 44's `C` (comment to sysop) command to resolve
    /// the addressee without leaning on the sysop's freely-renamable
    /// handle.
    /// # Errors
    /// Returns [`UserRepositoryError`] when the backing store cannot be
    /// queried or the stored row cannot be decoded into a [`User`].
    fn find_sysop(&self) -> Result<NameLookupResult, UserRepositoryError>;

    /// Applies the persistent consequences of one password-verification
    /// attempt (spec: `session.allium:VerifyPassword` plus the
    /// post-onboarded rule cluster).
    ///
    /// Merge semantics are defined by
    /// [`AuthOutcome::apply_to`]: additive
    /// `invalid_attempts` bumps on a mismatch, an absolute clear on a
    /// match, one-way `account_locked` / `force_password_reset` flag
    /// sets, and the daily-counter reset/bump carried by the
    /// [`AuthOutcome::Matched`] `daily` field. Implementations must
    /// apply the whole outcome atomically.
    ///
    /// # Parameters
    /// - `slot`: stable account id of the user the attempt targeted.
    /// - `outcome`: which verification branch ran and what it decided.
    ///
    /// # Errors
    /// [`UserRepositoryError::UserNotFound`] when no user occupies
    /// `slot` (the handle is reported as `"slot N"`);
    /// [`UserRepositoryError::Storage`] when the backing store fails.
    fn record_auth_outcome(
        &self,
        slot: u32,
        outcome: &AuthOutcome,
    ) -> Result<(), UserRepositoryError>;

    /// Replaces the stored credential triple
    /// (spec: `session.allium:CompletePasswordReset`).
    ///
    /// Security fields are immediate authoritative writes
    /// (designs/USERS.md): the hash/salt/kind triple and
    /// `password_last_updated` overwrite, and `force_password_reset`
    /// clears. Merge semantics are defined by
    /// [`PasswordChange::apply_to`].
    ///
    /// # Parameters
    /// - `slot`: stable account id of the user changing password.
    /// - `change`: the freshly computed credential triple.
    ///
    /// # Errors
    /// [`UserRepositoryError::UserNotFound`] when no user occupies
    /// `slot`; [`UserRepositoryError::Storage`] when the backing store
    /// fails.
    fn record_password_change(
        &self,
        slot: u32,
        change: &PasswordChange,
    ) -> Result<(), UserRepositoryError>;

    /// Applies a delta/patch write covering the session-mutable state
    /// families (counters, read pointers, scan flags, conference
    /// position, display preferences, `last_call`).
    ///
    /// Merge semantics are defined by [`UserPatch::apply_to`]:
    /// additive counters, monotonic `MAX` merges for `last_call` and
    /// pointer rows, last-writer-wins preference overwrites. The
    /// command shape makes concurrent same-account sessions compose
    /// instead of silently reverting each other — broad whole-row
    /// session saves are forbidden (designs/USERS.md).
    /// Implementations must apply the whole patch atomically: a patch
    /// that fails partway (e.g. a pointer row referencing a missing
    /// membership) must leave the store unchanged.
    ///
    /// # Parameters
    /// - `slot`: stable account id of the patched user.
    /// - `patch`: the not-yet-persisted changes of one session window.
    ///
    /// # Errors
    /// [`UserRepositoryError::UserNotFound`] when no user occupies
    /// `slot`; [`UserRepositoryError::Storage`] when the backing store
    /// fails or rejects part of the patch.
    fn apply_user_patch(&self, slot: u32, patch: &UserPatch) -> Result<(), UserRepositoryError>;

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
    /// - [`UserCreationError::Storage`] when the backing store cannot
    ///   allocate or persist the row.
    fn create_user(&self, draft: NewUserDraft) -> Result<User, UserCreationError>;
}
