//! Shared `#[cfg(test)]` fixtures for the `menu_flow` test modules.
//!
//! The default [`AppServices`] fixture was copy-pasted verbatim across a
//! dozen test modules; this is the single source of truth. Tests that
//! need a non-default port build on top of it:
//! `let mut s = test_services(); s.field = ...; s`.

use std::sync::Arc;

use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::app::services::AppServices;
use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
use crate::domain::session::SessionPolicy;
use crate::domain::user::RatioMode;

/// The default [`AppServices`] test fixture: in-memory ports, an empty
/// conference catalogue, empty file/mail/flag stores, new users allowed,
/// and ratio accounting disabled. Override individual fields on the
/// returned value for tests that need a non-default port.
pub(crate) fn test_services() -> AppServices {
    AppServices {
        user_repo: Arc::new(InMemoryUserRepository::default()),
        hasher: Arc::new(Pbkdf2PasswordHasher::new()),
        caller_log: Arc::new(InMemoryCallerLog::new()),
        screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
        conferences: Arc::new(Vec::new()),
        mail_stores: Arc::new(InMemoryMailStores::new()),
        file_repo: Arc::new(InMemoryFileRepository::new(Vec::new(), Vec::new())),
        flagged_store: Arc::new(InMemoryFlaggedStore::new()),
        session_policy: SessionPolicy::default(),
        default_ratio: DefaultRatio {
            mode: RatioMode::Disabled,
            value: 0,
        },
        new_user_gate: Arc::new(NewUserGateConfig {
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        }),
        bbs_name: Arc::from("Test BBS"),
    }
}
