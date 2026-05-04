//! Application layer: composition root and entry point.
//!
//! Wires [`crate::adapters`] into [`crate::domain`] and drives the
//! BBS. The binary entry point [`main`] starts the tokio runtime and
//! returns immediately while later slices have nothing to run.

/// Boots the tokio multi-thread runtime and exits cleanly.
///
/// Phase 0 has no behaviour to run; later slices populate this entry
/// point with the supervisor and listener tasks. The function is the
/// hand-off point from the binary's `main`.
#[tokio::main]
pub async fn main() {}
