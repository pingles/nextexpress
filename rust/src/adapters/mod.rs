//! Adapters layer: concrete implementations of domain ports.
//!
//! Free to depend on [`crate::domain`]. Storage, transport and crypto
//! adapters land here as their owning slices arrive.

pub mod in_memory_caller_log;
pub mod in_memory_user_repository;
pub mod pbkdf2_password_hasher;
pub mod telnet_listener;
