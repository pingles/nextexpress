//! Runtime composition root.
//!
//! Wires the driven adapters (user store, password hasher, caller log,
//! screen and conference repositories), the configuration-derived
//! policy values (session policy, ratio defaults, new-user gate) and
//! the [`NodePool`] into a single value the transport adapters consume.
//!
//! Transport adapters such as
//! [`crate::adapters::telnet_listener::TelnetListener`] used to derive
//! these values themselves from a [`Config`]; moving the wiring here
//! keeps the listener focused on telnet transport, makes the
//! composition visible in one place, and lets future transports
//! (SSH, local console, websocket, in-process test transports) share
//! the same setup without duplicating it.

use std::sync::Arc;

use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::app::config::Config;
use crate::app::node_pool::NodePool;
use crate::app::services::{
    AppServices, SharedCallerLog, SharedConferences, SharedHasher, SharedScreens, SharedUserRepo,
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
    /// Wires `config` and the supplied driven adapters into a runtime.
    ///
    /// The screen repository is constructed as a
    /// [`FileScreenRepository`] rooted at `config.bbs_path`. Use
    /// [`Self::with_screens`] when an injected screen source is
    /// required (currently exposed for symmetry; not used in tests
    /// today).
    #[must_use]
    pub fn from_config(
        config: &Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        conferences: SharedConferences,
    ) -> Self {
        let screens: SharedScreens = Arc::new(FileScreenRepository::new(config.bbs_path.clone()));
        Self::with_screens(config, user_repo, hasher, caller_log, screens, conferences)
    }

    /// Variant of [`Self::from_config`] that takes a pre-constructed
    /// screen repository. Future tests that swap the on-disk fixture
    /// for an in-memory one will drive this entry point.
    #[must_use]
    pub fn with_screens(
        config: &Config,
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        screens: SharedScreens,
        conferences: SharedConferences,
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
            config.session_policy(),
            default_ratio,
            new_user_gate,
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
