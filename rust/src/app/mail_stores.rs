//! Application-level registry for per-msgbase [`MailStore`] handles.
//!
//! [`MailStore`] is the domain persistence port for a single message
//! base; the async mutex and registry used by the interactive runtime
//! are application infrastructure that coordinate concurrent menu tasks
//! but are not part of the domain contract. Per the spec's
//! `lock_msgbase(msgbase)` predicate
//! (`messaging.allium:PostMail`), holding the registry's mutex is the
//! in-process equivalent of the legacy `MailLock` sentinel file.
//!
//! Callers ask the registry to lock a store and receive an opaque
//! [`MailStoreGuard`]; the raw `Arc<tokio::sync::Mutex<_>>` is never
//! exposed across the port boundary. The two-store entry point
//! ([`MailStores::lock_pair`]) centralises lock ordering and detects
//! same-store requests before acquiring a second lock, ruling out the
//! self-deadlock that a naive "lock source then lock target" path can
//! hit when source and target resolve to the same mutex.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;

use tokio::sync::OwnedMutexGuard;

use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail_store::MailStore;

/// Future returned by [`MailStores::lock`].
pub type MailStoreLockFut<'a> = Pin<Box<dyn Future<Output = Option<MailStoreGuard>> + Send + 'a>>;

/// Future returned by [`MailStores::lock_pair`].
pub type MailStorePairLockFut<'a> =
    Pin<Box<dyn Future<Output = MailStorePairLockOutcome> + Send + 'a>>;

/// Owned guard over a locked [`MailStore`]. Implements [`Deref`] and
/// [`DerefMut`] to `dyn MailStore`, so menu use cases work directly
/// against the domain port without seeing the registry's `Arc<Mutex<_>>`
/// internals.
///
/// The guard is `'static`-lifetime (built from
/// [`tokio::sync::Mutex::lock_owned`]); the underlying `Arc` is kept
/// alive for the guard's lifetime so the menu task can move it across
/// `.await` points freely.
pub struct MailStoreGuard {
    inner: OwnedMutexGuard<Box<dyn MailStore + Send>>,
}

impl MailStoreGuard {
    /// Wraps a tokio owned guard. Internal — only the registry adapter
    /// constructs these.
    pub(crate) fn new(inner: OwnedMutexGuard<Box<dyn MailStore + Send>>) -> Self {
        Self { inner }
    }
}

impl Deref for MailStoreGuard {
    type Target = dyn MailStore;

    fn deref(&self) -> &Self::Target {
        &**self.inner
    }
}

impl DerefMut for MailStoreGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut **self.inner
    }
}

/// Outcome of [`MailStores::lock_pair`].
///
/// The `Locked` variant carries the requested stores labelled as
/// `source` and `target`; the registry is free to acquire the
/// underlying mutexes in any deadlock-safe order before handing them
/// back labelled.
pub enum MailStorePairLockOutcome {
    /// No store is registered for the source msgbase.
    MissingSource,
    /// No store is registered for the target msgbase.
    MissingTarget,
    /// Source and target resolved to the same store; no locks were
    /// acquired. Callers should treat this as a domain-level rejection
    /// (e.g. `MoveMail::SameMsgbase`) rather than as a lookup failure.
    SameStore,
    /// Both stores were locked and are ready to use.
    Locked {
        /// Guard over the source store.
        source: MailStoreGuard,
        /// Guard over the target store.
        target: MailStoreGuard,
    },
}

/// Registry of [`MailStore`] handles keyed by [`MessageBaseRef`].
///
/// The composition root opens one store per known message base at
/// startup and serves them via this app-layer service. The trait
/// returns `None` / `MissingSource` / `MissingTarget` when the caller
/// asks for a base that has no configured store; menu flows surface
/// this as "no message base for this conference" rather than
/// constructing one on the fly.
pub trait MailStores: Send + Sync {
    /// Locks the store bound to `msgbase`. Future resolves to
    /// `Some(guard)` with the [`MailStore`] locked when a handle is
    /// registered, `None` when the registry has no store for that
    /// coordinate.
    fn lock(&self, msgbase: MessageBaseRef) -> MailStoreLockFut<'_>;

    /// Locks both `source` and `target` so a multi-store domain rule
    /// (currently only `messaging.allium:MoveMail`) can run against
    /// them.
    ///
    /// Detects `source == target` *before* acquiring any lock and
    /// returns [`MailStorePairLockOutcome::SameStore`] — preventing the
    /// self-deadlock that a naive lock-source-then-lock-target call
    /// path hits when both coordinates resolve to the same mutex.
    fn lock_pair(&self, source: MessageBaseRef, target: MessageBaseRef)
        -> MailStorePairLockFut<'_>;
}
