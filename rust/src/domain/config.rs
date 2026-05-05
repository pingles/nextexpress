//! BBS configuration (spec: `core.allium:config`).
//!
//! Config keys land here as the slice that first reads them is
//! implemented. Slice 7 introduces `max_nodes`; Slice 8 introduces
//! `bbs_path`.

use std::path::PathBuf;

/// Runtime configuration of the BBS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
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
            max_nodes: 32,
            bbs_path: PathBuf::from("."),
            max_password_failures: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_nodes_is_thirty_two() {
        assert_eq!(Config::default().max_nodes, 32);
    }

    #[test]
    fn default_max_password_failures_is_three() {
        assert_eq!(Config::default().max_password_failures, 3);
    }
}
