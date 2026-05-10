//! Caller-log line formatters and the day-bucketing helper they
//! depend on.
//!
//! Pure functions over a [`Session`] view — they read but never
//! mutate. Slice 53+ will extend `format_logoff_line` with transfer
//! accounting (`bytes_uploaded`, `bytes_downloaded`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{LogoffReason, LogonChannel, Session};

/// `session.allium:floor_to_day` black-box helper.
///
/// Buckets `at` into a day index where the boundary sits `offset`
/// past midnight UTC. The legacy `AmiExpress` equivalent is
/// `Div(currTime - 21600, 86400)` (six-hour offset) — see
/// `amiexpress/express.e:529`.
pub(super) fn floor_to_day(at: SystemTime, offset: Duration) -> i64 {
    fn saturating_i64(secs: u64) -> i64 {
        i64::try_from(secs).unwrap_or(i64::MAX)
    }

    let secs = match at.duration_since(UNIX_EPOCH) {
        Ok(d) => saturating_i64(d.as_secs()),
        Err(e) => -saturating_i64(e.duration().as_secs()),
    };
    let offset_secs = saturating_i64(offset.as_secs());
    (secs - offset_secs).div_euclid(86_400)
}

/// `session.allium:format_logon_line` black-box helper.
///
/// Produces the line written to the caller log when a session reaches
/// the menu. The legacy `AmiExpress` format is something like
/// `Logon: alice (node 1, 9600 baud, remote)`; we match that shape.
pub(super) fn format_logon_line(session: &Session) -> String {
    let handle = session
        .user()
        .map_or("?", crate::domain::user::User::handle);
    let channel = match session.channel() {
        LogonChannel::SysopConsole => "sysop_console",
        LogonChannel::Local => "local",
        LogonChannel::Remote => "remote",
        LogonChannel::Ftp => "ftp",
    };
    format!(
        "Logon: {handle} (node {}, {} baud, {channel})",
        session.node_number(),
        session.online_baud()
    )
}

/// `session.allium:format_logoff_line` black-box helper.
///
/// Phase 1 emits a minimal line. Slice 53 onward extends it with
/// transfer accounting (`bytes_uploaded`, `bytes_downloaded`).
pub(super) fn format_logoff_line(session: &Session) -> String {
    let handle = session
        .user()
        .map_or("?", crate::domain::user::User::handle);
    let reason = match session.logoff_reason() {
        Some(LogoffReason::NormalLogoff) => "normal_logoff",
        Some(LogoffReason::NewUserRejected) => "new_user_rejected",
        Some(LogoffReason::ExcessivePasswordFails) => "excessive_password_fails",
        Some(LogoffReason::LockedAccount) => "locked_account",
        Some(LogoffReason::OutOfTime) => "out_of_time",
        Some(LogoffReason::InputTimeout) => "input_timeout",
        Some(LogoffReason::CarrierLoss) => "carrier_loss",
        Some(LogoffReason::NoConferenceAccess) => "no_conference_access",
        None => "unknown",
    };
    format!(
        "Logoff: {handle} (node {}, reason {reason})",
        session.node_number()
    )
}
