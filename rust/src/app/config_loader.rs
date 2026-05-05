//! Loads a [`Config`] from disk for the binary's composition root
//! (Slice 13a).
//!
//! Lives in [`crate::app`] because reading files is a side effect, not
//! pure domain behaviour. The pure parsing step lives on
//! [`Config::from_toml_str`].

use std::path::Path;

use crate::app::config::{Config, ConfigError};

/// Errors returned by [`load_config`].
#[derive(Debug)]
pub enum LoadConfigError {
    /// `path` couldn't be read (missing, permissions, IO).
    Read {
        /// The path that was attempted.
        path: std::path::PathBuf,
        /// The underlying [`std::io::Error`].
        error: std::io::Error,
    },
    /// `path` was read but didn't deserialise into [`Config`].
    Parse {
        /// The path that was attempted.
        path: std::path::PathBuf,
        /// The underlying [`ConfigError`] from [`Config::from_toml_str`].
        error: ConfigError,
    },
}

impl std::fmt::Display for LoadConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, error } => {
                write!(f, "couldn't read config {}: {error}", path.display())
            }
            Self::Parse { path, error } => {
                write!(f, "couldn't parse config {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { error, .. } => Some(error),
            Self::Parse { error, .. } => Some(error),
        }
    }
}

/// Loads a [`Config`] from `path`, or returns [`Config::default`] when
/// `path` is `None`.
///
/// # Errors
/// - [`LoadConfigError::Read`] if `path` is `Some` and reading the
///   file fails (missing, permissions, IO).
/// - [`LoadConfigError::Parse`] if the file was read but didn't
///   deserialise into a [`Config`].
pub fn load_config(path: Option<&Path>) -> Result<Config, LoadConfigError> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let text = std::fs::read_to_string(path).map_err(|error| LoadConfigError::Read {
        path: path.to_path_buf(),
        error,
    })?;
    Config::from_toml_str(&text).map_err(|error| LoadConfigError::Parse {
        path: path.to_path_buf(),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_path_returns_defaults() {
        let config = load_config(None).expect("load defaults");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn missing_file_returns_read_error() {
        let path = Path::new("/no/such/path/nextexpress.toml");
        let error = load_config(Some(path)).expect_err("read should fail");
        assert!(
            matches!(error, LoadConfigError::Read { .. }),
            "got: {error:?}"
        );
        assert!(error.to_string().contains("/no/such/path/nextexpress.toml"));
    }

    #[test]
    fn malformed_toml_returns_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "port = \"oops\"\n").unwrap();
        let error = load_config(Some(&path)).expect_err("parse should fail");
        assert!(
            matches!(error, LoadConfigError::Parse { .. }),
            "got: {error:?}"
        );
    }

    #[test]
    fn valid_file_returns_parsed_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "port = 9999\nmax_nodes = 4\n").unwrap();
        let config = load_config(Some(&path)).expect("load");
        assert_eq!(config.port, 9999);
        assert_eq!(config.max_nodes, 4);
    }
}
