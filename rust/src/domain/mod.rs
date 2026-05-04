//! Domain layer: pure behaviour and entities.
//!
//! Per the hexagonal layout enforced by `tests/architecture.rs`, this
//! module must not import from [`crate::adapters`] or [`crate::app`].
//! Domain types and rules grow slice-by-slice as the Allium specs in
//! `specs/` demand them; this file is intentionally empty until then.
