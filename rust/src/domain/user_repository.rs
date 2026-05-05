//! [`UserRepository`] port (spec: `session.allium` black-box helpers
//! `lookup_name` and `user_for_name`).
//!
//! The port is a domain-side abstraction; concrete implementations live
//! in [`crate::adapters`].

use crate::domain::user::User;

/// Outcome of looking a typed handle up in the user database.
///
/// Mirrors `session.allium:NameLookupResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameLookupResult {
    /// A user with that handle exists.
    Found,
    /// No user matches and the input was not the new-user literal.
    NotFound,
    /// The literal `NEW` — request to register a new account.
    UserTypedNew,
}

/// Port over the user database.
///
/// Phase 1 only requires read access; mutation of `User` records lands
/// in Slice 11 once lockout state is introduced. Implementations are
/// expected to reject inputs containing wildcards (the legacy AmiExpress
/// code rejects `'*'` early) by returning [`NameLookupResult::NotFound`].
pub trait UserRepository {
    /// Resolves `typed` to a [`NameLookupResult`].
    ///
    /// # Parameters
    /// - `typed`: handle exactly as the user typed it.
    fn lookup_name(&self, typed: &str) -> NameLookupResult;

    /// Returns the user record for `handle`, or `None` if no such user
    /// exists. Only valid when [`Self::lookup_name`] returned
    /// [`NameLookupResult::Found`] for the same input.
    fn user_for_name(&self, handle: &str) -> Option<User>;
}
