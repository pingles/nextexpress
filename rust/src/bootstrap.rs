//! Composition root.
//!
//! `bootstrap` is the only place in the crate that may construct
//! concrete adapters and wire them into the application layer. It owns:
//!
//! * config loading,
//! * driven-port adapter selection ([`FileScreenRepository`],
//!   [`Pbkdf2PasswordHasher`], [`InMemoryUserRepository`] /
//!   [`SqliteUserRepository`], [`FileConferenceRepository`],
//!   [`FileMailStore`], [`InMemoryMailStores`], [`InMemoryCallerLog`]),
//! * default-sysop seeding when the user store is empty,
//! * runtime construction via [`crate::app::runtime::Runtime::new`], and
//! * listener bind / accept-loop.
//!
//! Everything below `app/` is then pure application or domain code:
//! the per-connection driver, the flows, and the message-base rules
//! never reach for an adapter type. The hexagonal boundary is verified
//! by `tests/architecture.rs`, which forbids `crate::adapters` imports
//! outside this module.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use crate::adapters::file_conference_repository::FileConferenceRepository;
use crate::adapters::file_mail_store::FileMailStore;
use crate::adapters::file_screen_repository::FileScreenRepository;
use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
use crate::adapters::sqlite_user_repository::SqliteUserRepository;
use crate::adapters::telnet_listener::TelnetListener;
use crate::app::config::Config;
use crate::app::config_loader;
use crate::app::mail_stores::MailStores;
use crate::app::runtime::Runtime;
use crate::app::seed;
use crate::app::services::{
    SharedCallerLog, SharedConferences, SharedHasher, SharedMailStores, SharedScreens,
    SharedUserRepo,
};
use crate::domain::caller_log::CallerLogAppender;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::conference_repository::ConferenceRepository;
use crate::domain::messaging::mail_store::MailStore;
use crate::domain::password::PasswordHasher;

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

/// Returns the startup banner line written to the server log on
/// process start. The short git SHA — captured by `build.rs` into
/// `NEXTEXPRESS_GIT_SHA` — is wrapped in parentheses so operators can
/// match a running process back to a specific source commit, mirroring
/// the wire-format banner in [`crate::app::wire_text::COPYRIGHT_LINES`].
fn startup_version_line() -> String {
    format!("NextExpress ({}) starting", env!("NEXTEXPRESS_GIT_SHA"))
}

/// Runs the BBS: load config, seed users, bind, accept forever.
///
/// Split out from [`main`] so failure paths are testable without
/// binding a real socket.
async fn run(args: &[OsString]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("{}", startup_version_line());
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

    let repo = open_user_repository(&config, hasher.as_ref(), &conferences)?;
    let log: Arc<dyn CallerLogAppender + Send + Sync> = Arc::new(InMemoryCallerLog::new());

    // Slice 41a: open one FileMailStore per known (conference,
    // msgbase) coordinate so the menu's R / M / N commands can
    // resolve a backing store for the session's open visit.
    let mail_stores: SharedMailStores = open_mail_stores(&config.bbs_path, &conferences)?;

    let conferences_handle: SharedConferences = Arc::new(conferences);
    let screens: SharedScreens = Arc::new(FileScreenRepository::new(config.bbs_path.clone()));

    let runtime = Runtime::new(
        &config,
        repo,
        hasher,
        log,
        screens,
        conferences_handle,
        mail_stores,
    );
    let listen_addr = format!("127.0.0.1:{}", config.port);
    let listener = TelnetListener::bind(&listen_addr, runtime).await?;
    println!("Listening on {}", listener.local_addr()?);
    listener.run().await?;
    Ok(())
}

/// Constructs the configured [`UserRepository`] adapter and seeds the
/// default sysop when the store is empty.
///
/// `None` for `config.user_storage` selects the in-memory adapter and
/// always seeds (single-process, throwaway data). `Some(path)` opens a
/// `SQLite` database at that path (created on first run) and only seeds
/// when the `users` table is empty — preserving prior data on restart.
fn open_user_repository(
    config: &Config,
    hasher: &(dyn PasswordHasher + Send + Sync),
    conferences: &[Conference],
) -> Result<SharedUserRepo, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(path) = config.user_storage.as_deref() {
        let repo = SqliteUserRepository::open(path)?;
        if repo.is_empty()? {
            let mut seeded = seed::default_sysop(hasher)?;
            seed::grant_all_memberships(&mut seeded, conferences);
            repo.insert_seed(&seeded)?;
            eprintln!(
                "WARNING: seeded default sysop credentials \
                 (handle=sysop, password=sysop) into {}. Change the \
                 sysop password before production use.",
                path.display()
            );
        } else {
            eprintln!("Loaded user database from {}.", path.display());
        }
        return Ok(Arc::new(repo));
    }

    let mut seeded = seed::default_sysop(hasher)?;
    seed::grant_all_memberships(&mut seeded, conferences);
    eprintln!(
        "WARNING: using in-memory user store with seeded default sysop \
         credentials (handle=sysop, password=sysop). User data will be \
         lost on shutdown — set `user_storage` in config to point at a \
         SQLite file for durable storage."
    );
    Ok(Arc::new(InMemoryUserRepository::new(vec![seeded])))
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
            registry.register(coord, Arc::new(tokio::sync::Mutex::new(boxed)));
        }
    }
    let shared: SharedMailStores = Arc::new(registry) as Arc<dyn MailStores + Send + Sync>;
    Ok(shared)
}

/// Builds a [`Runtime`] from the supplied driven-port handles and a
/// [`Config`], constructing a [`FileScreenRepository`] rooted at the
/// configured `bbs_path` for the screen-asset port.
///
/// Test code reaches for this helper when it needs the production
/// screen-loading behaviour but wants to drive the rest of the runtime
/// with in-memory adapters. Production code goes through [`run`].
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_runtime(
    config: &Config,
    user_repo: SharedUserRepo,
    hasher: SharedHasher,
    caller_log: SharedCallerLog,
    conferences: SharedConferences,
    mail_stores: SharedMailStores,
) -> Runtime {
    let screens: SharedScreens = Arc::new(FileScreenRepository::new(config.bbs_path.clone()));
    Runtime::new(
        config,
        user_repo,
        hasher,
        caller_log,
        screens,
        conferences,
        mail_stores,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_version_line_wraps_git_sha_in_parens() {
        // The server log must record the build's source commit on
        // process start so operators can correlate a running process
        // with a specific commit. `build.rs` captures the short SHA
        // into `NEXTEXPRESS_GIT_SHA`.
        let sha = env!("NEXTEXPRESS_GIT_SHA");
        let line = startup_version_line();
        let needle = format!("({sha})");
        assert!(
            line.contains(&needle),
            "expected `{needle}` in startup version line: {line:?}",
        );
        assert!(
            line.starts_with("NextExpress"),
            "startup line must lead with the product name: {line:?}",
        );
    }

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
