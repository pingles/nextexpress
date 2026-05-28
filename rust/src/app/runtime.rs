//! Runtime composition value.
//!
//! Carries the driven-port handles (user store, password hasher, caller
//! log, screen and conference repositories), the configuration-derived
//! policy values (session policy, ratio defaults, new-user gate) and
//! the [`NodePool`] in a single value the transport adapters consume.
//!
//! Construction is done by the bootstrap layer (`crate::bootstrap`)
//! which knows the concrete adapters; the runtime stays pure
//! application code. That keeps the listener focused on transport,
//! makes the composition visible in one place, and lets future
//! transports (SSH, local console, websocket, in-process test
//! transports) share the same setup without duplicating it.

use std::sync::Arc;

use crate::app::config::Config;
use crate::app::node_pool::NodePool;
use crate::app::services::{
    AppServices, SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedScreens,
    SharedUserRepo,
};
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};

/// Per-listener runtime: a node pool and the application services that
/// every accepted session shares. Cheap to clone (a handful of
/// `Arc::clone` calls).
#[derive(Clone)]
pub struct Runtime {
    pool: Arc<NodePool>,
    services: AppServices,
}

impl Runtime {
    /// Wires `config`, the supplied driven-port handles and the
    /// screen repository into a runtime.
    ///
    /// The runtime does not know how to build a screen repository; the
    /// bootstrap layer chooses which adapter to use (`FileScreenRepository`
    /// in production, an in-memory fake in tests) and passes the handle
    /// in here.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        screens: SharedScreens,
        conferences: SharedConferences,
        mail_stores: SharedMailStores,
    ) -> Self {
        let pool = Arc::new(NodePool::new(config.max_nodes));
        let default_ratio = DefaultRatio {
            mode: config.default_ratio_mode,
            value: config.default_ratio_value,
        };
        let new_user_gate = NewUserGateConfig {
            allow_new_users: config.allow_new_users,
            new_user_password: config.new_user_password.clone(),
            max_new_user_password_attempts: config.max_new_user_password_attempts,
        };
        let services = AppServices::new(
            user_repo,
            hasher,
            caller_log,
            screens,
            conferences,
            mail_stores,
            config.session_policy(),
            default_ratio,
            new_user_gate,
            config.bbs_name.clone(),
        );
        Self { pool, services }
    }

    /// Returns a clone of the shared [`NodePool`] handle.
    #[must_use]
    pub fn pool(&self) -> Arc<NodePool> {
        self.pool.clone()
    }

    /// Returns the application services container.
    #[must_use]
    pub fn services(&self) -> &AppServices {
        &self.services
    }
}
