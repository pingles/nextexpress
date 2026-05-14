//! Shared `ScanMailOnJoin` driver helper (Phase 6, Slice 41).
//!
//! Both the auto-rejoin path (in [`crate::app::session_driver`]) and
//! the explicit-join path (in [`crate::app::menu_flow`]) need to fire
//! `messaging.allium:ScanMail` after the new [`ConferenceVisit`] has
//! been created. The two call-sites differ only in which typed
//! wrapper holds the session, so this module exposes a generic
//! [`scan_mail_on_join`] free function parameterised by the
//! [`ScanOnJoin`] trait.
//!
//! The function:
//! 1. Resolves the session's open visit's [`MessageBaseRef`];
//! 2. Looks up the [`crate::domain::messaging::mail_store::MailStore`] handle
//!    for that coordinate from the [`AppServices`]'s mail-store
//!    registry;
//! 3. Locks the store, runs [`scan_mail`](crate::domain::messaging::scan_mail::scan_mail);
//! 4. Renders the `SCREEN_MAILSCAN` asset when the scan surfaced
//!    unread mail, then the textual summary line.
//!
//! Errors are written to stderr (so the operator can see them) and
//! degraded to a generic "mail store error" notice on the wire —
//! the session continues either way.
//!
//! [`ConferenceVisit`]: crate::domain::conference_visit::ConferenceVisit
//! [`MessageBaseRef`]: crate::domain::conference::MessageBaseRef

use std::time::SystemTime;

use crate::app::menu::scan_mail::{scan_mail, ScanMailOutcome};
use crate::app::services::AppServices;
use crate::app::terminal::Terminal;
use crate::app::wire_text::{render_scan_summary, MAIL_STORE_ERROR_LINE};
use crate::domain::session::typed::ScanOnJoin;

/// Whether the auto-scan-on-join walks from message 1 (`ForceAll`) or
/// from `pointers.last_scanned + 1` (`FollowPointer`)
/// — spec: `conferences.allium:scan_mode_for(visit)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinScanMode {
    /// Scan from the first message visible after the user's
    /// `last_scanned` pointer. The legacy `forceMailScan = NOFORCE`
    /// path; the natural default for both auto-rejoin and explicit
    /// join.
    FollowPointer,
    /// Scan every message in the base from 1. Legacy
    /// `forceMailScan = FORCE_MAILSCAN_ALL`, used by the conference-scan
    /// walk (`amiexpress/express.e:25264`).
    #[allow(dead_code)]
    ForceAll,
}

impl JoinScanMode {
    /// Returns the `from_message` value to feed `messaging.allium:ScanMail`.
    /// Mirrors the spec's
    /// `if scan_mode_for(visit) = force_all: 1 else: pointers.last_scanned + 1`
    /// branch in `conferences.allium:ScanMailOnJoin`.
    fn as_from_message(self) -> u32 {
        match self {
            JoinScanMode::ForceAll => 1,
            // 0 is the spec's "use last_scanned + 1" sentinel.
            JoinScanMode::FollowPointer => 0,
        }
    }
}

/// Fires `conferences.allium:ScanMailOnJoin` against `session`.
///
/// Returns `Ok(())` regardless of whether the scan found anything;
/// callers don't differentiate the unread-vs-empty case — both
/// fall through to the menu prompt.
pub(crate) async fn scan_mail_on_join<T, S>(
    terminal: &mut T,
    services: &AppServices,
    session: &mut S,
    mode: JoinScanMode,
) -> Result<(), T::Error>
where
    T: Terminal,
    S: ScanOnJoin,
{
    match scan_mail(
        session,
        services.mail_stores(),
        services.conferences(),
        mode.as_from_message(),
        SystemTime::now(),
    )
    .await
    {
        // No open visit / missing store is silent for auto-scan-on-join.
        ScanMailOutcome::NoOpenMsgbase | ScanMailOutcome::NoStore => Ok(()),
        ScanMailOutcome::StoreError(err) => {
            eprintln!("scan_mail_on_join failed: {err}");
            terminal.write(MAIL_STORE_ERROR_LINE).await?;
            terminal.flush().await?;
            Ok(())
        }
        ScanMailOutcome::Scanned(result) => {
            if result.unread_count > 0 {
                let screen = services.screens().mailscan_screen().await;
                terminal.write(&screen).await?;
            }
            let summary = render_scan_summary(result.unread_count, result.first_unread_number);
            terminal.write(&summary).await?;
            terminal.flush().await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_pointer_resolves_to_zero_so_scan_mail_uses_last_scanned_plus_one() {
        // Pin the spec's `from_message: pointers.last_scanned + 1`
        // branch — implemented by passing `0` as the sentinel.
        // A mutation that returned `1` here would silently flip
        // FollowPointer to "from message 1", losing the "new mail
        // only" semantics.
        assert_eq!(JoinScanMode::FollowPointer.as_from_message(), 0);
    }

    #[test]
    fn force_all_resolves_to_one_so_scan_mail_starts_from_message_one() {
        // Pin the spec's `from_message: 1` branch for force-all.
        assert_eq!(JoinScanMode::ForceAll.as_from_message(), 1);
    }
}
