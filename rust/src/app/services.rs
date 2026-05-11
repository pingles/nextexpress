//! Shared application services container.
//!
//! Carries the trait-object ports the BBS workflow drives — user
//! repository, password hasher, caller log, screen repository — plus
//! the policy/configuration values they consume. Adapters clone this
//! per accepted session; cloning is cheap because every port is held
//! behind an [`Arc`].
//!
//! Replacing the previous borrow-bag with a [`Clone`]able container
//! removes the lifetime parameters that were threading through every
//! driver and flow signature, and lets adapters move the services
//! into spawned futures without lifetime gymnastics.

use std::sync::Arc;

use crate::app::screens::ScreenRepository;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::conference::Conference;
use crate::domain::mail_store::MailStores;
use crate::domain::password::PasswordHasher;
use crate::domain::session::SessionPolicy;
use crate::domain::user_repository::UserRepository;

/// Shared user repository handle.
pub type SharedUserRepo = Arc<dyn UserRepository + Send + Sync + 'static>;
/// Shared password hasher handle.
pub type SharedHasher = Arc<dyn PasswordHasher + Send + Sync + 'static>;
/// Shared caller-log appender handle.
pub type SharedCallerLog = Arc<dyn CallerLogAppender + Send + Sync + 'static>;
/// Shared screen repository handle.
pub type SharedScreens = Arc<dyn ScreenRepository + Send + Sync + 'static>;
/// Shared, immutable conference catalogue handle (Slice 34a).
pub type SharedConferences = Arc<Vec<Conference>>;
/// Shared mail-store registry handle (Slice 39 / 41a).
pub type SharedMailStores = Arc<dyn MailStores + Send + Sync + 'static>;

/// Container for the trait-object ports and policy values an
/// interactive BBS session reads. Cheap to clone (one `Arc` bump per
/// port).
#[derive(Clone)]
pub struct AppServices {
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
    screens: SharedScreens,
    conferences: SharedConferences,
    mail_stores: SharedMailStores,
    session_policy: SessionPolicy,
    default_ratio: DefaultRatio,
    new_user_gate: Arc<NewUserGateConfig>,
}

impl AppServices {
    /// Constructs a services container from the supplied ports and
    /// configuration.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user_repo: SharedUserRepo,
        hasher: SharedHasher,
        caller_log: SharedCallerLog,
        screens: SharedScreens,
        conferences: SharedConferences,
        mail_stores: SharedMailStores,
        session_policy: SessionPolicy,
        default_ratio: DefaultRatio,
        new_user_gate: NewUserGateConfig,
    ) -> Self {
        Self {
            user_repo,
            hasher,
            caller_log,
            screens,
            conferences,
            mail_stores,
            session_policy,
            default_ratio,
            new_user_gate: Arc::new(new_user_gate),
        }
    }

    /// Returns the user repository as a `&dyn` trait object suitable
    /// for the generic `?Sized + UserRepository` flow signatures.
    #[must_use]
    pub fn user_repo(&self) -> &(dyn UserRepository + Send + Sync) {
        self.user_repo.as_ref()
    }

    /// Returns the password hasher as a `&dyn` trait object.
    #[must_use]
    pub fn hasher(&self) -> &(dyn PasswordHasher + Send + Sync) {
        self.hasher.as_ref()
    }

    /// Returns the caller-log appender as a `&dyn` trait object.
    #[must_use]
    pub fn caller_log(&self) -> &(dyn CallerLogAppender + Send + Sync) {
        self.caller_log.as_ref()
    }

    /// Returns the screen repository as a `&dyn` trait object.
    #[must_use]
    pub fn screens(&self) -> &(dyn ScreenRepository + Send + Sync) {
        self.screens.as_ref()
    }

    /// Returns the conference catalogue (Slice 34a). Sorted by
    /// conference number per the
    /// [`crate::domain::conference_repository::ConferenceRepository`]
    /// contract.
    #[must_use]
    pub fn conferences(&self) -> &[Conference] {
        self.conferences.as_ref()
    }

    /// Returns the mail-store registry (Slice 39 / 41a).
    #[must_use]
    pub fn mail_stores(&self) -> &(dyn MailStores + Send + Sync) {
        self.mail_stores.as_ref()
    }

    /// Returns the configured [`SessionPolicy`] (Copy).
    #[must_use]
    pub fn session_policy(&self) -> SessionPolicy {
        self.session_policy
    }

    /// Returns the configured [`DefaultRatio`] (Copy).
    #[must_use]
    pub fn default_ratio(&self) -> DefaultRatio {
        self.default_ratio
    }

    /// Returns the configured new-user gate as a borrowed reference.
    #[must_use]
    pub fn new_user_gate(&self) -> &NewUserGateConfig {
        &self.new_user_gate
    }
}
