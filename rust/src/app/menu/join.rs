//! Terminal-free explicit conference join use case.

use std::time::SystemTime;

use crate::domain::conference::{Conference, NameType};
use crate::domain::session::typed::{ExplicitJoinTransition, LoggingOffSession, MenuSession};

/// Outcome of an explicit `J <num>` command before rendering.
pub(crate) enum ExplicitJoinOutcome {
    /// The user is now attached to a conference/message-base pair.
    Joined {
        /// Session still in menu phase.
        session: MenuSession,
        /// Joined conference number.
        conference_number: u32,
        /// Joined message-base number.
        msgbase_number: u32,
        /// Whether the requested conference was actually joined.
        matched_request: bool,
        /// Optional display-name promotion screen to render.
        name_type_promoted_to: Option<NameType>,
    },
    /// The user has no accessible conferences and should log off.
    NoAccess(LoggingOffSession),
}

/// Runs the explicit join transition without terminal I/O.
pub(crate) fn explicit_join(
    session: MenuSession,
    conferences: &[Conference],
    target_conference_number: u32,
    now: SystemTime,
) -> ExplicitJoinOutcome {
    match session.explicit_join_conference(target_conference_number, conferences, now) {
        ExplicitJoinTransition::Joined {
            session,
            conference_number,
            msgbase_number,
            matched_request,
            name_type_promoted_to,
            ..
        } => ExplicitJoinOutcome::Joined {
            session,
            conference_number,
            msgbase_number,
            matched_request,
            name_type_promoted_to,
        },
        ExplicitJoinTransition::NoAccess(logging_off) => ExplicitJoinOutcome::NoAccess(logging_off),
    }
}
