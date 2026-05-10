//! [`ConferenceVisit`] entity and the `JoinConference` resolution
//! logic (spec: `conferences.allium`).
//!
//! Phase 4, Slice 30 introduces the auto-rejoin path: a session
//! arriving at the menu picks back up wherever it last was, falling
//! through to the lowest-numbered accessible conference when the
//! previous one has gone away or the user is brand-new.
//!
//! `JoinReason::ExplicitJoin` and `JoinReason::ConfScanWalk` arrive
//! with their owning slices (32 / 33). Bulletins (Slice 31) and mail
//! scan triggers (Slice 41) hang off `ConferenceVisit::created` events
//! but are not implemented here.

use std::time::SystemTime;

use crate::domain::conference::{first_accessible_conference, Conference, MessageBase};
use crate::domain::user::User;

/// How the user reached a join (spec: `conferences.allium:JoinReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JoinReason {
    /// On-logon return to the user's last conference (spec).
    AutoRejoin,
    /// User typed `J` (or `J <num>`) from the menu (spec: Slice 32).
    ExplicitJoin,
    /// Visit produced by the conference-scan walk
    /// (`conferences.allium:StepConferenceScan`, Slice 33).
    ConfScanWalk,
}

/// One row per (session, conference) attachment (spec:
/// `conferences.allium:ConferenceVisit`).
///
/// A visit is *open* (`left_at == None`) for as long as it is the
/// session's current conference. The
/// `SessionsHaveAtMostOneOpenVisit` invariant is enforced by the
/// session-level join logic, which closes any prior open visit before
/// pushing a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConferenceVisit {
    conference_number: u32,
    msgbase_number: u32,
    joined_at: SystemTime,
    left_at: Option<SystemTime>,
}

impl ConferenceVisit {
    /// Constructs a freshly-opened visit (spec:
    /// `ConferenceVisit.created`).
    #[must_use]
    pub fn new(conference_number: u32, msgbase_number: u32, joined_at: SystemTime) -> Self {
        Self {
            conference_number,
            msgbase_number,
            joined_at,
            left_at: None,
        }
    }

    /// Returns the 1-indexed conference number this visit is attached
    /// to.
    #[must_use]
    pub fn conference_number(&self) -> u32 {
        self.conference_number
    }

    /// Returns the 1-indexed message-base number within the
    /// conference. The
    /// `VisitedMsgBaseBelongsToVisitedConference` invariant is
    /// guaranteed at construction time by
    /// [`primary_msgbase_of`] returning a [`MessageBase`] whose
    /// `conference_number` equals the visited conference's
    /// `number` (enforced in turn by [`Conference::new`]).
    #[must_use]
    pub fn msgbase_number(&self) -> u32 {
        self.msgbase_number
    }

    /// Returns the timestamp the visit opened.
    #[must_use]
    pub fn joined_at(&self) -> SystemTime {
        self.joined_at
    }

    /// Returns the timestamp the visit was closed, if any.
    #[must_use]
    pub fn left_at(&self) -> Option<SystemTime> {
        self.left_at
    }

    /// Returns whether the visit is still the session's current
    /// attachment.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.left_at.is_none()
    }

    /// Closes an open visit at `now` (spec:
    /// `LeaveConferenceOnSwitch`). A no-op on already-closed visits
    /// so callers can sweep the visit list unconditionally.
    pub fn close(&mut self, now: SystemTime) {
        if self.left_at.is_none() {
            self.left_at = Some(now);
        }
    }
}

/// Outcome of a `JoinConference` resolution (spec:
/// `conferences.allium:JoinConference`).
///
/// References borrow from the `conferences` slice supplied to the
/// resolver. The caller turns these into a concrete
/// [`ConferenceVisit`] and an updated `User.last_joined`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinResolution<'a> {
    /// The user has access to a conference. The caller is
    /// responsible for creating the visit and updating
    /// `User.last_joined`.
    Resolved {
        /// Conference the session attaches to.
        conference: &'a Conference,
        /// Primary (number 1) message base within `conference`.
        msgbase: &'a MessageBase,
        /// `true` when the resolved conference matches the request
        /// (spec: `target_conference` was the one chosen). `false`
        /// signals the resolver fell through to
        /// `first_accessible_conference` because the user lacked
        /// access to their requested conference, so the listener
        /// can render the legacy "You do not have access to the
        /// requested conference" notice (`amiexpress/express.e:25157`)
        /// before continuing into the bulletin / menu cycle. For
        /// `auto_rejoin` this is always `true` — auto-rejoin asks
        /// for whatever the resolver settles on.
        matched_request: bool,
    },
    /// The user has no granted membership for any conference. The
    /// session should be terminated with `no_conference_access`.
    NoAccess,
}

/// Per-session conference-scan tracking (spec:
/// `conferences.allium:ConferenceScan`, Slice 33).
///
/// At most one scan is in progress on a session at a time, so the
/// session holds an `Option<ConferenceScan>`: `Some(_)` represents
/// the spec's `in_progress = true` and `None` the cleared state.
/// While present, [`crate::domain::session::Session`] suppresses
/// conference bulletins per `ShowConferenceBulletin` (Slice 31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConferenceScan {
    next_conference_number: Option<u32>,
    started_at: SystemTime,
}

impl ConferenceScan {
    /// Constructs an in-progress scan with `next_conference_number`
    /// already pointing at the conference the user should walk
    /// through next (or `None` when the walk has completed and the
    /// session is about to clear the scan).
    #[must_use]
    pub fn new(next_conference_number: Option<u32>, started_at: SystemTime) -> Self {
        Self {
            next_conference_number,
            started_at,
        }
    }

    /// Returns the next conference the scan should join, or `None`
    /// when the walk is complete.
    #[must_use]
    pub fn next_conference_number(&self) -> Option<u32> {
        self.next_conference_number
    }

    /// Returns the timestamp at which the scan started.
    #[must_use]
    pub fn started_at(&self) -> SystemTime {
        self.started_at
    }
}

/// Returns the lowest-numbered [`Conference`] in `conferences` whose
/// number is strictly greater than `after_number` and which the user
/// has a granted membership for. `None` when no such conference
/// remains. Mirrors the spec's `next_accessible_conference_after`
/// helper (`conferences.allium`).
#[must_use]
pub fn next_accessible_conference_after<'a>(
    user: &User,
    conferences: &'a [Conference],
    after_number: u32,
) -> Option<&'a Conference> {
    conferences
        .iter()
        .filter(|c| c.number() > after_number)
        .find(|c| user.has_membership(c))
}

/// Returns a [`MessageBase`] in `conference` with `number == 1`, or
/// the first declared base when no number-1 base exists. Mirrors the
/// spec's `primary_msgbase_of(conference)` helper.
///
/// `Conference::new` enforces `AtLeastOneMessageBase`, so the
/// fallback is reachable only on legacy conferences that number their
/// bases starting at something other than 1.
///
/// # Panics
/// Panics if `conference` somehow exposes an empty `msgbases()`
/// slice. The [`Conference`] constructor guarantees this never
/// happens, so the panic is a domain invariant guard rather than a
/// reachable error path.
#[must_use]
pub fn primary_msgbase_of(conference: &Conference) -> &MessageBase {
    conference
        .msgbases()
        .iter()
        .find(|m| m.number() == 1)
        .unwrap_or_else(|| {
            conference
                .msgbases()
                .first()
                .expect("AtLeastOneMessageBase guarantees a non-empty msgbases collection")
        })
}

/// Resolves the `JoinConference` rule for the auto-rejoin path
/// (spec: `conferences.allium:JoinConference`, `reason = auto_rejoin`).
///
/// The user's `last_joined_conference` is preferred when they still
/// have a granted membership for it, falling through to
/// `first_accessible_conference` otherwise. When neither yields a
/// match the session must terminate with `no_conference_access`.
#[must_use]
pub fn resolve_auto_rejoin<'a>(user: &User, conferences: &'a [Conference]) -> JoinResolution<'a> {
    let preferred = user.last_joined().and_then(|last| {
        conferences
            .iter()
            .find(|c| c.number() == last.conference_number())
            .filter(|c| user.has_membership(c))
    });
    let resolved =
        preferred.or_else(|| first_accessible_conference(user.memberships(), conferences));

    match resolved {
        None => JoinResolution::NoAccess,
        Some(conference) => JoinResolution::Resolved {
            conference,
            msgbase: primary_msgbase_of(conference),
            matched_request: true,
        },
    }
}

/// Resolves the `JoinConference` rule for the explicit-join path
/// (spec: `conferences.allium:JoinConference`,
/// `reason = explicit_join`, Slice 32).
///
/// When the user has a granted membership for `target_conference_number`
/// the resolution is direct. Otherwise the resolver falls through to
/// `first_accessible_conference` per the spec's `else` clause, with
/// `matched_request = false` so the listener can surface the legacy
/// "no access" notice (`amiexpress/express.e:25157`) before the
/// fallback conference's screens are rendered.
///
/// When the user has no granted membership anywhere at all the
/// resolver returns [`JoinResolution::NoAccess`] and the session
/// must terminate with `no_conference_access`.
#[must_use]
pub fn resolve_explicit_join<'a>(
    target_conference_number: u32,
    user: &User,
    conferences: &'a [Conference],
) -> JoinResolution<'a> {
    let target = conferences
        .iter()
        .find(|c| c.number() == target_conference_number)
        .filter(|c| user.has_membership(c));

    if let Some(conference) = target {
        return JoinResolution::Resolved {
            conference,
            msgbase: primary_msgbase_of(conference),
            matched_request: true,
        };
    }

    match first_accessible_conference(user.memberships(), conferences) {
        None => JoinResolution::NoAccess,
        Some(conference) => JoinResolution::Resolved {
            conference,
            msgbase: primary_msgbase_of(conference),
            matched_request: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::conference::{ConferenceMembership, MessageBase};
    use crate::domain::password::PasswordHashKind;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn make_user(memberships: &[(u32, bool)], last_joined: Option<u32>) -> User {
        let mut user = User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            t(0),
            100,
        )
        .expect("valid");
        for (num, granted) in memberships {
            user.upsert_membership(ConferenceMembership::new(*num, *granted));
        }
        if let Some(num) = last_joined {
            let conf = make_conf(num);
            let mb = conf.msgbases()[0].clone();
            user.record_join(&conf, &mb);
        }
        user
    }

    fn make_conf(number: u32) -> Conference {
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    fn make_conf_with_bases(number: u32, bases: Vec<(u32, &str)>) -> Conference {
        let mbs = bases
            .into_iter()
            .map(|(n, name)| MessageBase::new(number, n, name.to_string()))
            .collect();
        Conference::new(number, format!("Conf {number}"), mbs).expect("valid")
    }

    #[test]
    fn visit_starts_open() {
        // Pick distinct, non-1 numbers so accessor mutants can't slip
        // past by collapsing to a constant.
        let v = ConferenceVisit::new(7, 4, t(100));
        assert!(v.is_open());
        assert!(v.left_at().is_none());
        assert_eq!(v.joined_at(), t(100));
        assert_eq!(v.conference_number(), 7);
        assert_eq!(v.msgbase_number(), 4);
    }

    #[test]
    fn close_records_left_at_and_marks_closed() {
        let mut v = ConferenceVisit::new(1, 1, t(100));
        v.close(t(200));
        assert!(!v.is_open());
        assert_eq!(v.left_at(), Some(t(200)));
    }

    #[test]
    fn close_is_idempotent() {
        let mut v = ConferenceVisit::new(1, 1, t(100));
        v.close(t(200));
        v.close(t(300));
        // First close wins; second is a no-op.
        assert_eq!(v.left_at(), Some(t(200)));
    }

    #[test]
    fn primary_msgbase_returns_number_one_when_present() {
        let conf = make_conf_with_bases(3, vec![(1, "main"), (2, "tech")]);
        let mb = primary_msgbase_of(&conf);
        assert_eq!(mb.number(), 1);
        assert_eq!(mb.name(), "main");
    }

    #[test]
    fn primary_msgbase_falls_back_to_first_when_no_number_one_base() {
        // Defensive fallback for legacy conferences that number from
        // 2 upward; AtLeastOneMessageBase guarantees `.first()` is
        // safe.
        let conf = make_conf_with_bases(3, vec![(7, "weird"), (8, "even-weirder")]);
        let mb = primary_msgbase_of(&conf);
        assert_eq!(mb.number(), 7);
    }

    fn assert_resolved(outcome: &JoinResolution<'_>, expected_conf: u32, expected_mb: u32) {
        match outcome {
            JoinResolution::Resolved {
                conference,
                msgbase,
                ..
            } => {
                assert_eq!(conference.number(), expected_conf);
                assert_eq!(msgbase.number(), expected_mb);
            }
            JoinResolution::NoAccess => panic!("expected Resolved, got NoAccess"),
        }
    }

    #[test]
    fn auto_rejoin_picks_last_joined_when_user_still_has_access() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (2, true), (3, true)], Some(2));
        let outcome = resolve_auto_rejoin(&user, &confs);
        assert_resolved(&outcome, 2, 1);
    }

    #[test]
    fn auto_rejoin_falls_through_to_first_accessible_when_last_joined_lost_access() {
        // User's last_joined was 3 but they only have membership of 1 now
        // (e.g. sysop revoked their access to 3).
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (3, false)], Some(3));
        let outcome = resolve_auto_rejoin(&user, &confs);
        assert_resolved(&outcome, 1, 1);
    }

    #[test]
    fn auto_rejoin_falls_through_to_first_accessible_when_last_joined_was_deleted() {
        // last_joined points at a conference no longer in the
        // catalogue (sysop removed it).
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, true)], Some(99));
        let outcome = resolve_auto_rejoin(&user, &confs);
        assert_resolved(&outcome, 1, 1);
    }

    #[test]
    fn auto_rejoin_uses_first_accessible_for_users_with_no_last_joined() {
        // Brand-new account: no last_joined yet.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(2, true)], None);
        let outcome = resolve_auto_rejoin(&user, &confs);
        assert_resolved(&outcome, 2, 1);
    }

    #[test]
    fn auto_rejoin_returns_no_access_when_user_has_no_grants() {
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[], None);
        assert_eq!(resolve_auto_rejoin(&user, &confs), JoinResolution::NoAccess);
    }

    #[test]
    fn auto_rejoin_returns_no_access_when_only_revoked_rows_exist() {
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, false), (2, false)], Some(1));
        assert_eq!(resolve_auto_rejoin(&user, &confs), JoinResolution::NoAccess);
    }

    #[test]
    fn auto_rejoin_resolves_to_primary_msgbase_when_conference_has_multiple_bases() {
        let confs = vec![make_conf_with_bases(1, vec![(1, "main"), (2, "tech")])];
        let user = make_user(&[(1, true)], Some(1));
        let outcome = resolve_auto_rejoin(&user, &confs);
        assert_resolved(&outcome, 1, 1);
    }

    #[test]
    fn auto_rejoin_signals_matched_request_when_resolved() {
        let confs = vec![make_conf(1)];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_auto_rejoin(&user, &confs);
        match outcome {
            JoinResolution::Resolved {
                matched_request, ..
            } => assert!(matched_request),
            JoinResolution::NoAccess => panic!("expected Resolved"),
        }
    }

    #[test]
    fn explicit_join_resolves_directly_when_user_has_access_to_target() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (2, true)], None);
        let outcome = resolve_explicit_join(2, &user, &confs);
        match outcome {
            JoinResolution::Resolved {
                conference,
                matched_request,
                ..
            } => {
                assert_eq!(conference.number(), 2);
                assert!(matched_request);
            }
            JoinResolution::NoAccess => panic!("expected Resolved"),
        }
    }

    #[test]
    fn explicit_join_falls_through_with_matched_request_false_when_target_is_not_accessible() {
        // User picked 3 but they only have 1. Resolver falls through
        // to first_accessible (1), and matched_request is false so
        // the listener can render "You do not have access".
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_explicit_join(3, &user, &confs);
        match outcome {
            JoinResolution::Resolved {
                conference,
                matched_request,
                ..
            } => {
                assert_eq!(conference.number(), 1);
                assert!(
                    !matched_request,
                    "resolver fell through, listener must surface no-access notice"
                );
            }
            JoinResolution::NoAccess => panic!("expected Resolved"),
        }
    }

    #[test]
    fn explicit_join_falls_through_when_target_is_not_in_catalogue() {
        // User typed J 99 but no such conference exists.
        let confs = vec![make_conf(1)];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_explicit_join(99, &user, &confs);
        match outcome {
            JoinResolution::Resolved {
                conference,
                matched_request,
                ..
            } => {
                assert_eq!(conference.number(), 1);
                assert!(!matched_request);
            }
            JoinResolution::NoAccess => panic!("expected Resolved"),
        }
    }

    #[test]
    fn explicit_join_returns_no_access_when_user_has_no_grants_at_all() {
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[], None);
        assert_eq!(
            resolve_explicit_join(1, &user, &confs),
            JoinResolution::NoAccess
        );
    }

    #[test]
    fn explicit_join_with_target_having_only_revoked_membership_falls_through() {
        // The revoked row for conference 2 doesn't grant access; the
        // resolver falls through to conference 1 (the only granted
        // one). This pins the `.filter(has_membership)` step.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, true), (2, false)], None);
        let outcome = resolve_explicit_join(2, &user, &confs);
        match outcome {
            JoinResolution::Resolved {
                conference,
                matched_request,
                ..
            } => {
                assert_eq!(conference.number(), 1);
                assert!(!matched_request);
            }
            JoinResolution::NoAccess => panic!("expected Resolved"),
        }
    }
}
