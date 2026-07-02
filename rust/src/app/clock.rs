//! Application clock port.
//!
//! Flows resolve "now" through this port instead of calling
//! [`std::time::SystemTime::now`] directly, so tests can pin the
//! current instant (exact-value time assertions, the `N` date scan's
//! "(-X) Days" form, daily-cap rollovers). The domain is unaffected —
//! its rules already take `now: SystemTime` as a parameter; this port
//! only governs where the *application layer* obtains that value.
//! `tests/architecture.rs` rejects direct `SystemTime::now()` calls in
//! `src/app/` production code.

use std::time::SystemTime;

/// Source of the current wall-clock instant.
pub trait Clock {
    /// Returns the current instant.
    fn now(&self) -> SystemTime;
}
