//! [`ConferenceActivity`] — the per-session collection of
//! [`ConferenceVisit`]s and any in-progress `CS` scan
//! ([`ConferenceScan`]).
//!
//! Centralises the "close any open visit, then push a fresh one"
//! sequence the four [`crate::domain::session::Session`] join methods
//! used to repeat verbatim (`auto_rejoin_conference`,
//! `explicit_join_conference`, `start_conference_scan`,
//! `step_conference_scan`). With the sub-aggregate in place those
//! methods only call `activity.attach(...)` and stay free of vector
//! plumbing.

use std::time::SystemTime;

use crate::domain::conference_visit::{ConferenceScan, ConferenceVisit};

/// Owned per-session conference state.
///
/// `visits` is the ordered history of conferences the session has
/// joined; at most one entry has `left_at == None`
/// (`SessionsHaveAtMostOneOpenVisit`). `scan` is `Some(_)` while a
/// `CS` conference scan is mid-walk so the spec's
/// `ShowConferenceBulletin` rule can suppress the post-join bulletin.
#[derive(Debug, Clone)]
pub(super) struct ConferenceActivity {
    visits: Vec<ConferenceVisit>,
    scan: Option<ConferenceScan>,
}

impl ConferenceActivity {
    /// Constructs an empty activity record (no visits, no scan).
    pub(super) fn new() -> Self {
        Self {
            visits: Vec::new(),
            scan: None,
        }
    }

    /// Returns every visit recorded against this session, in the order
    /// they were attached. Mirrors
    /// [`crate::domain::session::Session::visits`].
    pub(super) fn visits(&self) -> &[ConferenceVisit] {
        &self.visits
    }

    /// Returns the single open visit, if any. The
    /// `SessionsHaveAtMostOneOpenVisit` invariant guarantees this is
    /// at most one entry.
    pub(super) fn current_visit(&self) -> Option<&ConferenceVisit> {
        self.visits.iter().find(|v| v.is_open())
    }

    /// Returns the in-progress scan, if any.
    pub(super) fn scan(&self) -> Option<&ConferenceScan> {
        self.scan.as_ref()
    }

    /// `true` when a conference scan is currently in progress.
    /// Used by the join paths to suppress the post-join bulletin
    /// (`conferences.allium:ShowConferenceBulletin`).
    pub(super) fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Closes any currently-open visit at `now`, then pushes a fresh
    /// visit attached to `(conference_number, msgbase_number)`.
    /// Maintains the `SessionsHaveAtMostOneOpenVisit` invariant.
    pub(super) fn attach(&mut self, conference_number: u32, msgbase_number: u32, now: SystemTime) {
        for visit in &mut self.visits {
            visit.close(now);
        }
        self.visits
            .push(ConferenceVisit::new(conference_number, msgbase_number, now));
    }

    /// Sets (or clears) the in-progress scan. Used by
    /// `start_conference_scan` and `step_conference_scan`.
    pub(super) fn set_scan(&mut self, scan: Option<ConferenceScan>) {
        self.scan = scan;
    }
}
