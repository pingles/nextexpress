//! Application layer: composition root and entry point.
//!
//! Wires [`crate::adapters`] into [`crate::domain`] and drives the BBS.
//! Slice 13a turned the entry point into a real composition root: it
//! reads a [`Config`][crate::app::config::Config] from an optional
//! TOML file, seeds a default sysop user when the in-memory repository
//! is empty, binds the [`TelnetListener`] and runs forever.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use crate::adapters::file_conference_repository::FileConferenceRepository;
use crate::adapters::file_mail_store::FileMailStore;
use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::adapters::telnet_listener::TelnetListener;
use crate::app::runtime::Runtime;
use crate::app::services::{SharedConferences, SharedMailStores};
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::conference_repository::ConferenceRepository;
use crate::domain::mail_store::{MailStore, MailStores};
use crate::domain::password::PasswordHasher;
use crate::domain::user_repository::UserRepository;

pub mod config;
pub mod config_loader;
pub mod login_flow;
pub mod mail_scan_on_join;
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
pub mod typed_session;
pub mod wire_text;

/// Entry point invoked by the binary's `main`.
///
/// Returns `ExitCode::SUCCESS` only if the listener loop exits cleanly
/// (which today only happens on a TCP error since the loop is otherwise
/// infinite). Any error during config load, seeding, bind or accept is
/// printed to stderr and the process exits with `ExitCode::FAILURE`.
///
/// # Panics
/// Panics if Tokio cannot construct the runtime for the async entry
/// point.
#[tokio::main]
#[must_use]
pub async fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nextexpress: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the BBS: load config, seed users, bind, accept forever.
///
/// Split out from [`main`] so failure paths are testable without
/// binding a real socket.
async fn run(args: &[OsString]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config_path: Option<PathBuf> = args.first().map(|s| Path::new(s).to_path_buf());
    let config = config_loader::load_config(config_path.as_deref())?;

    let hasher: Arc<dyn PasswordHasher + Send + Sync> = Arc::new(Pbkdf2PasswordHasher::new());

    let conferences = load_conferences(&config.bbs_path)?;
    if conferences.is_empty() {
        eprintln!(
            "WARNING: no conferences found at {}. Drop a `Conf<NN>/conference.toml` \
             into the bbs_path; sessions will hit no_conference_access at logon \
             until at least one conference is configured.",
            config.bbs_path.display()
        );
    }

    let mut seeded = seed::default_sysop(hasher.as_ref())?;
    seed::grant_all_memberships(&mut seeded, &conferences);
    eprintln!(
        "WARNING: seeded default sysop credentials (handle=sysop, password=sysop). \
         Configure a real user store before production use."
    );
    let repo: Arc<dyn UserRepository + Send + Sync> =
        Arc::new(InMemoryUserRepository::new(vec![seeded]));
    let log: Arc<dyn CallerLogAppender + Send + Sync> = Arc::new(InMemoryCallerLog::new());

    // Slice 41a: open one FileMailStore per known (conference,
    // msgbase) coordinate so the menu's R / M / N commands can
    // resolve a backing store for the session's open visit.
    let mail_stores: SharedMailStores = open_mail_stores(&config.bbs_path, &conferences)?;

    let conferences_handle: SharedConferences = Arc::new(conferences);

    let runtime = Runtime::from_config(&config, repo, hasher, log, conferences_handle, mail_stores);
    let listen_addr = format!("127.0.0.1:{}", config.port);
    let listener = TelnetListener::bind(&listen_addr, runtime).await?;
    println!("Listening on {}", listener.local_addr()?);
    listener.run().await?;
    Ok(())
}

/// Loads the conference catalogue from `bbs_path` (Slice 34a).
///
/// Conferences are configured on disk as `Conf<NN>/conference.toml`
/// files; the repo ships a default `Conf01` so a fresh `cargo run`
/// against the repo root has a working catalogue out of the box.
/// Listeners run with whatever the loader returns — empty included —
/// so deployments wishing to start with no conferences don't have a
/// runtime side-effect to undo.
fn load_conferences(
    bbs_path: &Path,
) -> Result<Vec<Conference>, Box<dyn std::error::Error + Send + Sync>> {
    let repo = FileConferenceRepository::new(bbs_path.to_path_buf());
    Ok(repo.load_all()?)
}

/// Opens one [`FileMailStore`] per known `(conference, msgbase)`
/// coordinate, rooted at `<bbs_path>/Conf<NN>/MsgBase[<M>]/`,
/// and bundles them into an [`InMemoryMailStores`] registry (Slice
/// 41a).
///
/// The `MsgBase` directory has no numeric suffix for `msgbase=1`
/// (matches the legacy `AmiExpress` single-base layout, which only
/// ever needed one directory). Higher-numbered bases append the
/// number: `MsgBase2`, `MsgBase3`, etc.
fn open_mail_stores(
    bbs_path: &Path,
    conferences: &[Conference],
) -> Result<SharedMailStores, Box<dyn std::error::Error + Send + Sync>> {
    let mut registry = InMemoryMailStores::new();
    for conf in conferences {
        for msgbase in conf.msgbases() {
            let dir_name = if msgbase.number() == 1 {
                "MsgBase".to_string()
            } else {
                format!("MsgBase{}", msgbase.number())
            };
            let dir = bbs_path
                .join(format!("Conf{:02}", conf.number()))
                .join(dir_name);
            let coord = MessageBaseRef::new(conf.number(), msgbase.number());
            let store = FileMailStore::open(dir, coord)?;
            let boxed: Box<dyn MailStore + Send> = Box::new(store);
            registry.register(coord, std::sync::Arc::new(tokio::sync::Mutex::new(boxed)));
        }
    }
    let shared: SharedMailStores = Arc::new(registry) as Arc<dyn MailStores + Send + Sync>;
    Ok(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_fails_when_config_path_is_unreadable() {
        let args = vec![OsString::from("/no/such/path/nextexpress.toml")];
        let result = run(&args).await;
        assert!(result.is_err(), "expected error, got {result:?}");
    }

    #[tokio::test]
    async fn run_fails_when_config_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "port = \"oops\"\n").unwrap();
        let args = vec![OsString::from(path.as_os_str())];
        let result = run(&args).await;
        assert!(result.is_err(), "expected error, got {result:?}");
    }
}
