//! Runtime BBS configuration (spec: `core.allium:config`).
//!
//! Config keys land here as the slice that first reads them is
//! implemented. Slice 7 introduces `max_nodes`; Slice 8 introduces
//! `bbs_path`; Slice 13a introduces `port` and the on-disk TOML
//! schema parsed by [`Config::from_toml_str`].

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Deserializer};

use crate::domain::session::SessionPolicy;
use crate::domain::user::RatioMode;

/// Default TCP port the telnet listener binds on (`core.allium:config.port`).
const DEFAULT_PORT: u16 = 2323;

/// Default number of simultaneous nodes (`core.allium:config.max_nodes`).
const DEFAULT_MAX_NODES: u32 = 32;

/// Default consecutive bad-password attempts before lockout
/// (`core.allium:config.max_password_failures`).
const DEFAULT_MAX_PASSWORD_FAILURES: u32 = 3;

/// Default offset past midnight UTC at which the daily counters roll
/// over (`core.allium:config.daily_reset_offset`). Mirrors the legacy
/// `AmiExpress` constant `21600` seconds (six hours) at
/// `amiexpress/express.e:529`.
const DEFAULT_DAILY_RESET_OFFSET: Duration = Duration::from_secs(6 * 3_600);

/// Default per-input idle timeout
/// (`core.allium:config.input_timeout`, Slice 17). Five minutes.
const DEFAULT_INPUT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Runtime configuration of the BBS.
///
/// Every field corresponds to one of the documented `core.allium:config`
/// keys. The struct deserialises from TOML via [`Config::from_toml_str`];
/// missing fields fall back to [`Config::default`] so a half-written
/// config doesn't surprise an operator with a different runtime default
/// than the one a fresh install would pick.
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
    /// Offset past midnight UTC at which the per-day counters roll
    /// over (spec: `core.allium:config.daily_reset_offset`, default
    /// `6h`).
    ///
    /// Parsed from a human-readable duration string in TOML, e.g.
    /// `daily_reset_offset = "6h"` or `"30m"`. Suffixes accepted:
    /// `s`, `m`, `h`, `d`.
    #[serde(deserialize_with = "deserialize_duration")]
    pub daily_reset_offset: Duration,
    /// Number of days after which a password is considered expired
    /// and `force_password_reset` is set on the user
    /// (spec: `core.allium:config.password_expiry_days`, default `0`
    /// = disabled).
    pub password_expiry_days: u32,
    /// Minimum length of a new password when the strength check
    /// runs (spec: `core.allium:config.min_password_length`,
    /// default `0` = disabled). Mirrors the legacy
    /// `MIN_PASSWORD_LENGTH` tooltype at `amiexpress/express.e:910`.
    pub min_password_length: u32,
    /// Minimum number of distinct character categories — lowercase
    /// letters, uppercase letters, digits, symbols — a new
    /// password must include
    /// (spec: `core.allium:config.min_password_categories`,
    /// default `0` = disabled). Values above `4` are clamped to
    /// `4`. Mirrors the legacy `MIN_PASSWORD_STRENGTH` tooltype at
    /// `amiexpress/express.e:915`.
    pub min_password_categories: u32,
    /// How long a session may go without typing before the
    /// `IdleTimeout` rule fires
    /// (spec: `core.allium:config.input_timeout`, default `5m`).
    /// Parsed in the same human-readable string format as
    /// [`Self::daily_reset_offset`].
    #[serde(deserialize_with = "deserialize_duration")]
    pub input_timeout: Duration,
    /// When `true`, an idle timeout is reported as
    /// [`crate::domain::session::LogoffReason::InputTimeout`];
    /// otherwise it is reported as
    /// [`crate::domain::session::LogoffReason::CarrierLoss`]
    /// (spec: `core.allium:config.treat_timeout_as_logoff`,
    /// default `false`).
    pub treat_timeout_as_logoff: bool,
    /// Default ratio enforcement mode applied to accounts created via
    /// the new-user registration flow
    /// (spec: `core.allium:config.default_ratio_mode`,
    /// default `by_files`).
    #[serde(deserialize_with = "deserialize_ratio_mode")]
    pub default_ratio_mode: RatioMode,
    /// Default ratio threshold applied to accounts created via the
    /// new-user registration flow
    /// (spec: `core.allium:config.default_ratio_value`, default `3`).
    pub default_ratio_value: u32,
    /// Whether the BBS accepts new-user registrations at all
    /// (spec: `core.allium:config.allow_new_users`, default `true`).
    /// `false` causes `session.allium:RejectDisallowedRegistration`
    /// to bounce any session that types `NEW` at the handle prompt.
    pub allow_new_users: bool,
    /// Optional sysop-set password gating new-user registration
    /// (spec: `core.allium:config.new_user_password`, default null).
    /// When `Some`, every new user must pass the
    /// `VerifyNewUserPassword` gate before the registration form is
    /// offered. Comparison is case-insensitive (parity with the
    /// legacy `StriCmp` at `amiexpress/express.e:30027`).
    pub new_user_password: Option<String>,
    /// Maximum incorrect attempts at `new_user_password` before the
    /// session is dropped
    /// (spec: `core.allium:config.max_new_user_password_attempts`,
    /// default `3` — matches the legacy hard-coded budget at
    /// `amiexpress/express.e:30037-30042`).
    pub max_new_user_password_attempts: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            max_nodes: DEFAULT_MAX_NODES,
            bbs_path: PathBuf::from("."),
            max_password_failures: DEFAULT_MAX_PASSWORD_FAILURES,
            daily_reset_offset: DEFAULT_DAILY_RESET_OFFSET,
            password_expiry_days: 0,
            min_password_length: 0,
            min_password_categories: 0,
            input_timeout: DEFAULT_INPUT_TIMEOUT,
            treat_timeout_as_logoff: false,
            default_ratio_mode: RatioMode::ByFiles,
            default_ratio_value: 3,
            allow_new_users: true,
            new_user_password: None,
            max_new_user_password_attempts: 3,
        }
    }
}

/// Parses a TOML string like `"6h"` / `"30m"` / `"45s"` / `"2d"` into
/// a [`Duration`].
fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_duration_string(&raw).map_err(serde::de::Error::custom)
}

/// Parses a [`RatioMode`] from one of the spec's enum names: `disabled`,
/// `by_files`, or `by_bytes`.
fn deserialize_ratio_mode<'de, D>(deserializer: D) -> Result<RatioMode, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.as_str() {
        "disabled" => Ok(RatioMode::Disabled),
        "by_files" => Ok(RatioMode::ByFiles),
        "by_bytes" => Ok(RatioMode::ByBytes),
        other => Err(serde::de::Error::custom(format!(
            "unknown ratio_mode '{other}': expected disabled, by_files, or by_bytes"
        ))),
    }
}

/// Parses an integer-prefixed duration suffixed with `s`, `m`, `h`, or
/// `d`. Whitespace is trimmed; an empty input is rejected.
fn parse_duration_string(input: &str) -> Result<Duration, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty duration".to_string());
    }
    let (digits, suffix) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len()),
    );
    if digits.is_empty() {
        return Err(format!("missing magnitude in '{trimmed}'"));
    }
    let value: u64 = digits
        .parse()
        .map_err(|e| format!("couldn't parse magnitude in '{trimmed}': {e}"))?;
    let multiplier = match suffix {
        "s" | "" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        other => return Err(format!("unknown duration suffix '{other}'")),
    };
    let secs = value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("duration overflow in '{trimmed}'"))?;
    Ok(Duration::from_secs(secs))
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

    /// Converts runtime config into session-domain policy.
    ///
    /// # Returns
    /// A [`SessionPolicy`] threading every session-time policy value
    /// the config exposes: `max_password_failures`,
    /// `daily_reset_offset`, `password_expiry_days`,
    /// `min_password_length` and `min_password_categories`.
    #[must_use]
    pub fn session_policy(&self) -> SessionPolicy {
        SessionPolicy::new(self.max_password_failures)
            .with_daily_reset_offset(self.daily_reset_offset)
            .with_password_expiry_days(self.password_expiry_days)
            .with_min_password_length(self.min_password_length)
            .with_min_password_categories(self.min_password_categories)
            .with_input_timeout(self.input_timeout)
            .with_treat_timeout_as_logoff(self.treat_timeout_as_logoff)
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
    fn default_daily_reset_offset_is_six_hours() {
        assert_eq!(
            Config::default().daily_reset_offset,
            Duration::from_secs(6 * 3_600)
        );
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(
            parse_duration_string("90s").unwrap(),
            Duration::from_secs(90)
        );
        assert_eq!(parse_duration_string("0s").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(
            parse_duration_string("5m").unwrap(),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn parse_duration_hours() {
        assert_eq!(
            parse_duration_string("6h").unwrap(),
            Duration::from_secs(6 * 3_600)
        );
    }

    #[test]
    fn parse_duration_days() {
        assert_eq!(
            parse_duration_string("2d").unwrap(),
            Duration::from_secs(2 * 86_400)
        );
    }

    #[test]
    fn parse_duration_no_suffix_treated_as_seconds() {
        assert_eq!(
            parse_duration_string("42").unwrap(),
            Duration::from_secs(42)
        );
    }

    #[test]
    fn parse_duration_rejects_unknown_suffix() {
        assert!(parse_duration_string("5y").is_err());
    }

    #[test]
    fn parse_duration_rejects_empty() {
        assert!(parse_duration_string("").is_err());
        assert!(parse_duration_string("   ").is_err());
    }

    #[test]
    fn parse_duration_rejects_missing_magnitude() {
        assert!(parse_duration_string("h").is_err());
    }

    #[test]
    fn session_policy_uses_configured_password_failure_limit() {
        let config = Config {
            max_password_failures: 5,
            ..Config::default()
        };
        assert_eq!(config.session_policy(), SessionPolicy::new(5));
    }

    #[test]
    fn session_policy_threads_configured_daily_reset_offset() {
        let config = Config {
            daily_reset_offset: Duration::from_secs(7_200),
            ..Config::default()
        };
        assert_eq!(
            config.session_policy().daily_reset_offset(),
            Duration::from_secs(7_200)
        );
    }

    #[test]
    fn from_toml_str_parses_all_fields() {
        let toml = r#"
            port = 9999
            max_nodes = 8
            bbs_path = "/srv/bbs"
            max_password_failures = 5
            daily_reset_offset = "3h"
            password_expiry_days = 90
            min_password_length = 8
            min_password_categories = 3
            input_timeout = "2m"
            treat_timeout_as_logoff = true
            default_ratio_mode = "by_bytes"
            default_ratio_value = 5
            allow_new_users = false
            new_user_password = "letmein"
            max_new_user_password_attempts = 5
        "#;
        let config = Config::from_toml_str(toml).expect("parse");
        assert_eq!(config.port, 9999);
        assert_eq!(config.max_nodes, 8);
        assert_eq!(config.bbs_path, PathBuf::from("/srv/bbs"));
        assert_eq!(config.max_password_failures, 5);
        assert_eq!(config.daily_reset_offset, Duration::from_secs(3 * 3_600));
        assert_eq!(config.password_expiry_days, 90);
        assert_eq!(config.min_password_length, 8);
        assert_eq!(config.min_password_categories, 3);
        assert_eq!(config.input_timeout, Duration::from_secs(120));
        assert!(config.treat_timeout_as_logoff);
        assert_eq!(config.default_ratio_mode, RatioMode::ByBytes);
        assert_eq!(config.default_ratio_value, 5);
        assert!(!config.allow_new_users);
        assert_eq!(config.new_user_password.as_deref(), Some("letmein"));
        assert_eq!(config.max_new_user_password_attempts, 5);
    }

    #[test]
    fn default_new_user_gate_is_open_with_no_password() {
        let defaults = Config::default();
        assert!(defaults.allow_new_users);
        assert!(defaults.new_user_password.is_none());
        assert_eq!(defaults.max_new_user_password_attempts, 3);
    }

    #[test]
    fn default_ratio_mode_is_by_files_with_threshold_three() {
        let defaults = Config::default();
        assert_eq!(defaults.default_ratio_mode, RatioMode::ByFiles);
        assert_eq!(defaults.default_ratio_value, 3);
    }

    #[test]
    fn from_toml_str_rejects_unknown_ratio_mode() {
        let toml = r#"default_ratio_mode = "bogus""#;
        assert!(Config::from_toml_str(toml).is_err());
    }

    #[test]
    fn from_toml_str_rejects_invalid_duration() {
        let toml = "daily_reset_offset = \"xyz\"";
        assert!(Config::from_toml_str(toml).is_err());
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
        assert_eq!(config.daily_reset_offset, defaults.daily_reset_offset);
        assert_eq!(config.password_expiry_days, defaults.password_expiry_days);
        assert_eq!(config.min_password_length, defaults.min_password_length);
        assert_eq!(
            config.min_password_categories,
            defaults.min_password_categories
        );
        assert_eq!(config.input_timeout, defaults.input_timeout);
        assert_eq!(
            config.treat_timeout_as_logoff,
            defaults.treat_timeout_as_logoff
        );
        assert_eq!(config.default_ratio_mode, defaults.default_ratio_mode);
        assert_eq!(config.default_ratio_value, defaults.default_ratio_value);
        assert_eq!(config.allow_new_users, defaults.allow_new_users);
        assert_eq!(config.new_user_password, defaults.new_user_password);
        assert_eq!(
            config.max_new_user_password_attempts,
            defaults.max_new_user_password_attempts
        );
    }

    #[test]
    fn default_input_timeout_is_five_minutes() {
        assert_eq!(Config::default().input_timeout, Duration::from_secs(5 * 60));
    }

    #[test]
    fn default_treat_timeout_as_logoff_is_false() {
        assert!(!Config::default().treat_timeout_as_logoff);
    }

    #[test]
    fn session_policy_threads_idle_timeout_knobs() {
        let config = Config {
            input_timeout: Duration::from_secs(30),
            treat_timeout_as_logoff: true,
            ..Config::default()
        };
        let policy = config.session_policy();
        assert_eq!(policy.input_timeout(), Duration::from_secs(30));
        assert!(policy.treat_timeout_as_logoff());
    }

    #[test]
    fn default_password_policy_is_disabled() {
        let defaults = Config::default();
        assert_eq!(defaults.password_expiry_days, 0);
        assert_eq!(defaults.min_password_length, 0);
        assert_eq!(defaults.min_password_categories, 0);
    }

    #[test]
    fn session_policy_threads_password_expiry_and_strength() {
        let config = Config {
            password_expiry_days: 30,
            min_password_length: 6,
            min_password_categories: 3,
            ..Config::default()
        };
        let policy = config.session_policy();
        assert_eq!(policy.password_expiry_days(), 30);
        assert_eq!(policy.min_password_length(), 6);
        assert_eq!(policy.min_password_categories(), 3);
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
