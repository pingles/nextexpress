//! Application-level registry for shared mail-store handles.
//!
//! [`MailStore`][crate::domain::mail_store::MailStore] is the domain
//! persistence port for a single message base. The async mutex and
//! registry used by the interactive runtime are application
//! infrastructure: they coordinate concurrent menu tasks, but are not
//! part of the domain contract.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::domain::conference::MessageBaseRef;
use crate::domain::mail_store::MailStore;

/// Thread-safe shared handle to a single-msgbase [`MailStore`]
/// implementation, locked behind a [`tokio::sync::Mutex`] so the menu
/// loop can `lock().await` from inside an async task.
///
/// Cloning a [`SharedMailStore`] bumps the [`Arc`] count; concurrent
/// readers serialise through the mutex. Per the spec's
/// `lock_msgbase(msgbase)` predicate (`messaging.allium:PostMail`),
/// holding the mutex is the in-process equivalent of the legacy
/// `MailLock` sentinel file.
pub type SharedMailStore = Arc<Mutex<Box<dyn MailStore + Send>>>;

/// Registry of [`MailStore`] handles keyed by [`MessageBaseRef`].
///
/// The composition root opens one store per known message base at
/// startup and serves them via this app-layer service. Returning
/// `None` means the caller asked for a base that has no configured
/// store; menu flows surface this as "no message base for this
/// conference" rather than constructing one on the fly.
pub trait MailStores: Send + Sync {
    /// Returns the shared, lockable handle bound to `msgbase`, or
    /// `None` when the registry has no store for that coordinate.
    fn for_msgbase(&self, msgbase: MessageBaseRef) -> Option<SharedMailStore>;
}
