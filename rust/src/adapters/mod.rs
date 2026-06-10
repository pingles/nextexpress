//! Adapters layer: concrete implementations of domain ports.
//!
//! Free to depend on [`crate::domain`]. Storage, transport and crypto
//! adapters land here as their owning slices arrive.

pub mod file_conference_repository;
pub mod file_mail_store;
pub mod file_screen_repository;
pub mod in_memory_caller_log;
pub mod in_memory_file_repository;
pub mod in_memory_mail_stores;
pub mod in_memory_user_repository;
pub mod pbkdf2_password_hasher;
pub mod sqlite_user_repository;
pub(crate) mod telnet_line;
pub mod telnet_listener;
