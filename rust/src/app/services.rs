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

use crate::app::mail_stores::MailStores;
use crate::app::screens::ScreenRepository;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::conference::Conference;
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
/// port). Constructed as a plain struct literal — the composition
/// root and the test fixtures name every field, so adding a service
/// is one field plus the construction sites, with no positional
/// constructor to keep in sync.
#[derive(Clone)]
pub struct AppServices {
    /// User repository port.
    pub user_repo: SharedUserRepo,
    /// Password hasher port.
    pub hasher: SharedHasher,
    /// Caller-log appender port.
    pub caller_log: SharedCallerLog,
    /// Screen repository port.
    pub screens: SharedScreens,
    /// Conference catalogue (Slice 34a), sorted by conference number
    /// per the
    /// [`crate::domain::conference_repository::ConferenceRepository`]
    /// contract.
    pub conferences: SharedConferences,
    /// Mail-store registry (Slice 39 / 41a).
    pub mail_stores: SharedMailStores,
    /// Session policy values (`Copy`).
    pub session_policy: SessionPolicy,
    /// Ratio defaults applied to fresh registrations (`Copy`).
    pub default_ratio: DefaultRatio,
    /// New-user registration gate configuration.
    pub new_user_gate: Arc<NewUserGateConfig>,
    /// BBS name shown in the menu prompt (legacy `cmds.bbsName`,
    /// Tier A quickwin A4).
    pub bbs_name: Arc<str>,
}
