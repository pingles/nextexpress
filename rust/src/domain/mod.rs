//! Domain layer: pure behaviour and entities.
//!
//! Per the hexagonal layout enforced by `tests/architecture.rs`, this
//! module must not import from [`crate::adapters`] or [`crate::app`].
//! Domain types and rules grow slice-by-slice as the Allium specs in
//! `specs/` demand them.

pub mod caller_log;
pub mod node;
pub mod password;
pub mod session;
pub mod user;
pub mod user_repository;
