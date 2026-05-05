//! BBS configuration (spec: `core.allium:config`).
//!
//! Config keys land here as the slice that first reads them is
//! implemented. Slice 7 introduces `max_nodes`; Slice 8 introduces
//! `bbs_path`; Slice 13a introduces `port` and the on-disk TOML
//! schema parsed by [`Config::from_toml_str`].

use std::path::PathBuf;

use serde::Deserialize;

/// Default TCP port the telnet listener binds on (`core.allium:config.port`).
const DEFAULT_PORT: u16 = 2323;

/// Default number of simultaneous nodes (`core.allium:config.max_nodes`).
const DEFAULT_MAX_NODES: u32 = 32;

/// Default consecutive bad-password attempts before lockout
/// (`core.allium:config.max_password_failures`).
const DEFAULT_MAX_PASSWORD_FAILURES: u32 = 3;

/// Runtime configuration of the BBS.
///
/// Every field corresponds to one of the documented `core.allium:config`
/// keys. The struct deserialises from TOML via [`Config::from_toml_str`];
/// missing fields fall back to [`Config::default`] so a half-written
/// config doesn't surprise an operator with a different default than the
/// one a fresh install would pick.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// TCP port the telnet listener binds on
    /// (spec: `core.allium:config.port`, default `2323`).
    pub port: u16,
    /// Maximum simultaneous nodes the BBS will host
    /// (spec: `core.allium:config.max_nodes`, default `32`).
    pub max_nodes: u32,
    /// Root directory of the BBS installation. Sub-directories like
    /// `Screens/`, `Conf01/`, etc. are resolved relative to this.
    pub bbs_path: PathBuf,
    /// Number of consecutive bad-password attempts before the session
    /// ends and the account is locked
    /// (spec: `core.allium:config.max_password_failures`, default `3`).
    pub max_password_failures: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            max_nodes: DEFAULT_MAX_NODES,
            bbs_path: PathBuf::from("."),
            max_password_failures: DEFAULT_MAX_PASSWORD_FAILURES,
        }
    }
}

/// Errors returned by [`Config::from_toml_str`].
#[derive(Debug)]
pub enum ConfigError {
    /// TOML failed to parse against the [`Config`] schema.
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "couldn't parse config: {error}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
        }
    }
}

impl Config {
    /// Parses a [`Config`] from a TOML string.
    ///
    /// Every field is optional; missing fields fall back to
    /// [`Config::default`]. Unknown fields are rejected so typos in a
    /// config file are caught early rather than silently ignored.
    ///
    /// # Errors
    /// Returns [`ConfigError::Parse`] when the input isn't valid TOML
    /// or doesn't deserialise into [`Config`] (e.g. wrong type for a
    /// known field, or an unknown field).
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        toml::from_str(input).map_err(ConfigError::Parse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_is_2323() {
        assert_eq!(Config::default().port, 2323);
    }

    #[test]
    fn default_max_nodes_is_thirty_two() {
        assert_eq!(Config::default().max_nodes, 32);
    }

    #[test]
    fn default_max_password_failures_is_three() {
        assert_eq!(Config::default().max_password_failures, 3);
    }

    #[test]
    fn from_toml_str_parses_all_fields() {
        let toml = r#"
            port = 9999
            max_nodes = 8
            bbs_path = "/srv/bbs"
            max_password_failures = 5
        "#;
        let config = Config::from_toml_str(toml).expect("parse");
        assert_eq!(config.port, 9999);
        assert_eq!(config.max_nodes, 8);
        assert_eq!(config.bbs_path, PathBuf::from("/srv/bbs"));
        assert_eq!(config.max_password_failures, 5);
    }

    #[test]
    fn from_toml_str_falls_back_to_defaults_for_missing_fields() {
        let toml = "port = 9999\n";
        let config = Config::from_toml_str(toml).expect("parse");
        let defaults = Config::default();
        assert_eq!(config.port, 9999);
        assert_eq!(config.max_nodes, defaults.max_nodes);
        assert_eq!(config.bbs_path, defaults.bbs_path);
        assert_eq!(config.max_password_failures, defaults.max_password_failures);
    }

    #[test]
    fn from_toml_str_accepts_empty_input() {
        let config = Config::from_toml_str("").expect("parse");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn from_toml_str_returns_error_on_wrong_type() {
        let result = Config::from_toml_str("port = \"not a number\"");
        assert!(result.is_err(), "expected parse error, got {result:?}");
    }

    #[test]
    fn from_toml_str_returns_error_on_unknown_field() {
        let result = Config::from_toml_str("nonsense_key = 1");
        assert!(
            result.is_err(),
            "expected unknown-field rejection, got {result:?}"
        );
    }

    #[test]
    fn config_error_display_mentions_parse_failure() {
        let err = Config::from_toml_str("port = \"x\"").unwrap_err();
        assert!(
            err.to_string().contains("couldn't parse config"),
            "got: {err}"
        );
    }
}
