//! `J <num>` (Explicit Join) menu command (Slice 32).
//!
//! Routes through [`MenuSession::explicit_join_conference`]: writes the
//! legacy "no access" notice when the resolver fell through, the
//! `Joining Conference: <name>` announcement on success, any name-type
//! promotion screen (Slice 34), then fires Slice 41's `ScanMailOnJoin`
//! against the new visit.

use std::time::SystemTime;

use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, NO_CONFERENCE_ACCESS_LINE};
use crate::domain::session::typed::{ExplicitJoinTransition, LoggingOffSession, MenuSession};

/// Outcome of [`super::MenuFlow::handle_explicit_join`]. The success
/// branch returns the still-Menu-state session so the menu loop
/// continues; failure terminates with `LogoffReason::NoConferenceAccess`.
pub(super) enum ExplicitJoinResult {
    /// The user is now attached to a (possibly fallback) conference.
    Joined(MenuSession),
    /// The user lost their last membership; the session is closing.
    NoAccess(LoggingOffSession),
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_explicit_join(
        &mut self,
        session: MenuSession,
        target_conference_number: u32,
    ) -> Result<ExplicitJoinResult, T::Error> {
        let conferences = self.services.conferences();
        let outcome = session.explicit_join_conference(
            target_conference_number,
            conferences,
            SystemTime::now(),
        );
        match outcome {
            ExplicitJoinTransition::Joined {
                mut session,
                conference_number,
                msgbase_number,
                matched_request,
                name_type_promoted_to,
                ..
            } => {
                // Compute the announcement bytes up-front so the
                // immutable borrow on `self.services.conferences()`
                // doesn't overlap the mutable borrows below.
                let line =
                    format_explicit_join_line(conferences, conference_number, msgbase_number);
                if !matched_request {
                    self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                        .await?;
                }
                self.write_and_flush(&line).await?;
                render_name_type_promotion(
                    self.terminal,
                    self.services.screens(),
                    name_type_promoted_to,
                )
                .await?;
                crate::app::mail_scan_on_join::scan_mail_on_join(
                    self.terminal,
                    self.services,
                    &mut session,
                    crate::app::mail_scan_on_join::JoinScanMode::FollowPointer,
                )
                .await?;
                Ok(ExplicitJoinResult::Joined(session))
            }
            ExplicitJoinTransition::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(ExplicitJoinResult::NoAccess(logging_off))
            }
        }
    }
}
