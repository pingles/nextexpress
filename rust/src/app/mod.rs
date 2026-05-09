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

use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::adapters::telnet_listener::TelnetListener;
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::password::PasswordHasher;
use crate::domain::user_repository::UserRepository;

pub mod config;
pub mod config_loader;
pub mod node_pool;
pub mod screens;
pub mod seed;
pub mod session_driver;
pub mod session_flow;

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
    let seeded = seed::default_sysop(hasher.as_ref())?;
    eprintln!(
        "WARNING: seeded default sysop credentials (handle=sysop, password=sysop). \
         Configure a real user store before production use."
    );
    let repo: Arc<dyn UserRepository + Send + Sync> =
        Arc::new(InMemoryUserRepository::new(vec![seeded]));
    let log: Arc<dyn CallerLogAppender + Send + Sync> = Arc::new(InMemoryCallerLog::new());

    let listen_addr = format!("127.0.0.1:{}", config.port);
    let listener = TelnetListener::bind(&listen_addr, config, repo, hasher, log).await?;
    println!("Listening on {}", listener.local_addr()?);
    listener.run().await?;
    Ok(())
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
