//! [`CallerLog`] entity and the [`CallerLogAppender`] port (spec:
//! `session.allium:CallerLog`).
//!
//! The BBS writes a handful of caller-log lines per session
//! (welcome, password failures, key actions, logoff). Phase 1 starts
//! emitting password-failure entries in Slice 11 and logon / goodbye
//! lines in Slices 12 and 13.

use std::time::SystemTime;

/// A single line in the BBS-wide caller log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerLog {
    /// Node number the session that produced the entry was bound to.
    pub session_node: u32,
    /// When the entry was produced.
    pub at: SystemTime,
    /// The free-form line text.
    pub text: String,
    /// Marks password-failure entries so the sysop console can render
    /// them in red (spec: `session.allium:CallerLog.is_password_failure`).
    pub is_password_failure: bool,
}

/// Port that appends [`CallerLog`] entries to the BBS-wide caller log.
///
/// Implementations live in [`crate::adapters`].
pub trait CallerLogAppender {
    /// Records `entry` in the caller log.
    fn append(&self, entry: CallerLog);
}
