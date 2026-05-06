//! `NextExpress`: a Rust port of the `AmiExpress` BBS.
//!
//! The crate is organised in three top-level modules following a
//! hexagonal (ports and adapters) layout:
//!
//! - [`domain`] — pure behaviour and entity definitions distilled from
//!   the Allium specs in `specs/`. It must not depend on [`adapters`]
//!   or [`app`]; the [`tests/architecture.rs`] integration test
//!   enforces this.
//! - [`adapters`] — concrete implementations of the ports the domain
//!   exposes (storage, transports, hashes). Free to depend on the
//!   domain.
//! - [`app`] — composition root and entry point. Wires adapters into
//!   the domain and runs the BBS supervisor.

pub mod adapters;
pub mod app;
pub mod domain;
