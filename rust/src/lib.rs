//! `NextExpress`: a Rust port of the `AmiExpress` BBS.
//!
//! The crate is organised in four top-level modules following a
//! hexagonal (ports and adapters) layout:
//!
//! - [`domain`] — pure behaviour and entity definitions distilled from
//!   the Allium specs in `specs/`. It must not depend on [`adapters`],
//!   [`app`] or [`bootstrap`]; the [`tests/architecture.rs`]
//!   integration test enforces this.
//! - [`adapters`] — concrete implementations of the ports the domain
//!   exposes (storage, transports, hashes). Free to depend on the
//!   domain.
//! - [`app`] — application layer: ports, services, flows, and
//!   transport-agnostic drivers. Free to depend on the domain. Forbidden
//!   from importing [`adapters`] in production code so the hexagonal
//!   boundary stays clean; `tests/architecture.rs` enforces this.
//! - [`bootstrap`] — composition root. The only module allowed to
//!   construct concrete adapters and wire them into the application
//!   layer.

pub mod adapters;
pub mod app;
pub mod bootstrap;
pub mod domain;
