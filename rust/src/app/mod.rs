//! Application layer: ports, services, flows, and transport-agnostic
//! drivers.
//!
//! `app` is the BBS's behaviour expressed in terms of trait-object
//! ports. It owns the per-connection driver
//! ([`session_driver::SessionDriver`]), the login/registration/menu
//! sub-flows, the menu use-case modules, the application-port traits
//! ([`terminal::Terminal`], [`screens::ScreenRepository`],
//! [`mail_stores::MailStores`]), the services container
//! ([`services::AppServices`]) and the runtime composition value
//! ([`runtime::Runtime`]).
//!
//! Adapter construction and process wiring live one layer further out
//! in [`crate::bootstrap`]; this module is forbidden from naming
//! `crate::adapters` in production code, and that boundary is
//! enforced by `tests/architecture.rs`.

pub mod colour_terminal;
pub mod config;
pub mod config_loader;
pub mod login_flow;
pub mod mail_scan_on_join;
pub mod mail_stores;
pub(crate) mod menu;
pub mod menu_command;
pub mod menu_flow;
pub mod node_pool;
pub mod registration_flow;
pub mod runtime;
pub mod screens;
pub mod seed;
pub mod services;
pub mod session_driver;
pub mod session_flow;
pub mod session_presenter;
pub mod terminal;
pub mod wire_text;
