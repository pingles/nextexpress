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

/// Outcome of the auto-rejoin `JoinConference` resolution (spec:
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
    },
    /// The user has no granted membership for any conference. The
    /// session should be terminated with `no_conference_access`.
    NoAccess,
}

/// Outcome of the explicit-join resolution (Tier C C2).
///
/// Unlike [`resolve_auto_rejoin`] there is no fall-through to
/// `first_accessible_conference` and no disconnect: the legacy
/// `internalCommandJ` access-checks the requested conference and, on
/// failure, prints the no-access notice and returns to the menu with
/// the caller still attached to their current conference
/// (`amiexpress/express.e:25156-25158`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplicitJoinResolution<'a> {
    /// The user holds a granted membership for the requested
    /// conference. The caller creates the visit and updates
    /// `User.last_joined`.
    Granted {
        /// Conference the session attaches to.
        conference: &'a Conference,
        /// Message base within `conference` the session attaches to:
        /// the requested base when it exists, else the conference's
        /// primary base (Tier C C4a; the legacy `joinConf` reset,
        /// `amiexpress/express.e:4995`).
        msgbase: &'a MessageBase,
    },
    /// The requested conference is not accessible to the user, or —
    /// defensively — no conference with that number exists in the
    /// catalogue (impossible after the prompt clamp under legacy
    /// contiguous numbering, but `NextExpress` allows gaps). The
    /// session stays where it is.
    Denied,
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

/// Returns the highest-numbered [`Conference`] in `conferences` whose
/// number is strictly less than `before_number` and which the user
/// has a granted membership for. `None` when no such conference
/// exists — the caller decides the edge behaviour (the legacy `<`
/// command falls into the interactive join prompt, no wraparound).
/// Mirrors the downward walk of `internalCommandLT`
/// (`amiexpress/express.e:24535-24538`) over the ascending catalogue,
/// the dual of [`next_accessible_conference_after`].
#[must_use]
pub fn prev_accessible_conference_before<'a>(
    user: &User,
    conferences: &'a [Conference],
    before_number: u32,
) -> Option<&'a Conference> {
    // The catalogue is in ascending number order per the
    // `ConferenceRepository::load_all` contract, so a reverse scan
    // visits candidates nearest-first.
    conferences
        .iter()
        .rev()
        .filter(|c| c.number() < before_number)
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
        },
    }
}

/// Resolves the explicit-join path of `J <num>` / `JM <num>` (Tier C
/// C2 / C4a, legacy `internalCommandJ` / `internalCommandJM`,
/// `amiexpress/express.e:25156-25158` / `:25236`).
///
/// The requested conference number is looked up exactly: a granted
/// membership yields [`ExplicitJoinResolution::Granted`]; anything
/// else — revoked membership, no membership, or no such conference in
/// the catalogue — is [`ExplicitJoinResolution::Denied`] and the
/// caller stays in its current conference. Explicit join never falls
/// through to another conference and never logs the user off (they
/// already hold a conference).
///
/// `requested_msgbase_number` targets a specific message base of the
/// conference (`None` means unspecified — the primary base). A
/// requested base that does not exist on the conference defensively
/// resets to the primary base, mirroring the legacy `joinConf` clamp
/// `IF msgBaseNum<1 OR >getConfMsgBaseCount(conf) THEN msgBaseNum:=1`
/// (`amiexpress/express.e:4995`) — the range checks that decide
/// between joining and prompting are the caller's concern, exactly as
/// in the legacy split between `internalCommandJ`/`JM` and `joinConf`.
#[must_use]
pub fn resolve_explicit_join<'a>(
    target_conference_number: u32,
    requested_msgbase_number: Option<u32>,
    user: &User,
    conferences: &'a [Conference],
) -> ExplicitJoinResolution<'a> {
    conferences
        .iter()
        .find(|c| c.number() == target_conference_number)
        .filter(|c| user.has_membership(c))
        .map_or(ExplicitJoinResolution::Denied, |conference| {
            let msgbase = requested_msgbase_number
                .and_then(|n| conference.find_msgbase(n))
                .unwrap_or_else(|| primary_msgbase_of(conference));
            ExplicitJoinResolution::Granted {
                conference,
                msgbase,
            }
        })
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
    fn next_accessible_walk_picks_the_nearest_higher_granted_conference() {
        // Legacy `internalCommandGT` walk (`amiexpress/express.e:24554-24557`):
        // upward from current+1, skipping conferences the caller has no
        // grant for.
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (2, false), (3, true)], None);
        let next = next_accessible_conference_after(&user, &confs, 1).expect("conference 3");
        assert_eq!(next.number(), 3, "the revoked conference 2 is skipped");
    }

    #[test]
    fn next_accessible_walk_returns_none_past_the_top_edge() {
        // No wraparound (`amiexpress/express.e:24559`): past the top
        // the legacy falls into the interactive J prompt.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, true), (2, true)], None);
        assert!(next_accessible_conference_after(&user, &confs, 2).is_none());
    }

    #[test]
    fn prev_accessible_walk_picks_the_nearest_lower_granted_conference() {
        // Legacy `internalCommandLT` walk (`amiexpress/express.e:24535-24538`):
        // downward from current-1. With every grant in place the
        // nearest lower neighbour wins — not the lowest one.
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (2, true), (3, true)], None);
        let prev = prev_accessible_conference_before(&user, &confs, 3).expect("conference 2");
        assert_eq!(prev.number(), 2);
    }

    #[test]
    fn prev_accessible_walk_skips_conferences_without_a_grant() {
        // Revoked membership for 2 and no membership row at all for 3:
        // both are skipped transparently, landing on 1.
        let confs = vec![make_conf(1), make_conf(2), make_conf(3), make_conf(4)];
        let user = make_user(&[(1, true), (2, false), (4, true)], None);
        let prev = prev_accessible_conference_before(&user, &confs, 4).expect("conference 1");
        assert_eq!(prev.number(), 1);
    }

    #[test]
    fn prev_accessible_walk_handles_non_contiguous_numbering() {
        // NextExpress allows catalogue gaps; the walk follows the
        // sorted catalogue, not `n - 1` arithmetic.
        let confs = vec![make_conf(2), make_conf(5), make_conf(9)];
        let user = make_user(&[(2, true), (5, true), (9, true)], None);
        let prev = prev_accessible_conference_before(&user, &confs, 9).expect("conference 5");
        assert_eq!(prev.number(), 5);
        let prev = prev_accessible_conference_before(&user, &confs, 5).expect("conference 2");
        assert_eq!(prev.number(), 2);
    }

    #[test]
    fn prev_accessible_walk_returns_none_at_the_bottom_edge() {
        // No wraparound (`amiexpress/express.e:24540`): below the
        // lowest grant the legacy falls into the interactive J prompt.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, true), (2, true)], None);
        assert!(prev_accessible_conference_before(&user, &confs, 1).is_none());
        // Same when every lower-numbered conference is revoked.
        let user = make_user(&[(1, false), (2, true)], None);
        assert!(prev_accessible_conference_before(&user, &confs, 2).is_none());
    }

    #[test]
    fn explicit_join_grants_the_exact_target_when_user_has_access() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true), (2, true)], None);
        let outcome = resolve_explicit_join(2, None, &user, &confs);
        match outcome {
            ExplicitJoinResolution::Granted {
                conference,
                msgbase,
            } => {
                assert_eq!(conference.number(), 2);
                assert_eq!(msgbase.number(), 1);
            }
            ExplicitJoinResolution::Denied => panic!("expected Granted"),
        }
    }

    fn assert_granted(outcome: &ExplicitJoinResolution<'_>, expected_conf: u32, expected_mb: u32) {
        match outcome {
            ExplicitJoinResolution::Granted {
                conference,
                msgbase,
            } => {
                assert_eq!(conference.number(), expected_conf);
                assert_eq!(msgbase.number(), expected_mb);
            }
            ExplicitJoinResolution::Denied => panic!("expected Granted, got Denied"),
        }
    }

    #[test]
    fn explicit_join_resolves_the_requested_msgbase_when_it_exists() {
        // Tier C C4a: `JM <n>` / `J <a>.<b>` target a specific base
        // of the conference (`joinConf(currentConf,newMsgBase,...)`,
        // `amiexpress/express.e:25236`).
        let confs = vec![make_conf_with_bases(1, vec![(1, "main"), (2, "tech")])];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_explicit_join(1, Some(2), &user, &confs);
        assert_granted(&outcome, 1, 2);
    }

    #[test]
    fn explicit_join_out_of_range_msgbase_resets_to_the_primary_base() {
        // Defensive rule from legacy `joinConf`
        // (`amiexpress/express.e:4995`): an out-of-range message-base
        // number reaching the join resets to the primary base — never
        // a denial, never a panic.
        let confs = vec![make_conf_with_bases(1, vec![(1, "main"), (2, "tech")])];
        let user = make_user(&[(1, true)], None);
        for out_of_range in [0, 3, 99] {
            let outcome = resolve_explicit_join(1, Some(out_of_range), &user, &confs);
            assert_granted(&outcome, 1, 1);
        }
    }

    #[test]
    fn explicit_join_msgbase_reset_lands_on_the_declared_primary() {
        // The reset goes to the *primary* base, which for legacy data
        // numbering from something other than 1 is the first declared
        // base (the `primary_msgbase_of` fallback).
        let confs = vec![make_conf_with_bases(
            1,
            vec![(7, "weird"), (8, "even-weirder")],
        )];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_explicit_join(1, Some(99), &user, &confs);
        assert_granted(&outcome, 1, 7);
    }

    #[test]
    fn explicit_join_without_msgbase_request_lands_on_the_primary_base() {
        // `None` is the "unspecified" marker: `J <n>`'s
        // `DEF newMsgBase=1` default (`amiexpress/express.e:25116`)
        // — the conference's primary base.
        let confs = vec![make_conf_with_bases(1, vec![(1, "main"), (2, "tech")])];
        let user = make_user(&[(1, true)], None);
        let outcome = resolve_explicit_join(1, None, &user, &confs);
        assert_granted(&outcome, 1, 1);
    }

    #[test]
    fn explicit_join_denies_an_inaccessible_target_without_falling_through() {
        // Legacy internalCommandJ access-checks the request and stays
        // put (`amiexpress/express.e:25156-25158`) — there is no
        // first-accessible fallback for explicit joins.
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let user = make_user(&[(1, true)], None);
        assert_eq!(
            resolve_explicit_join(3, None, &user, &confs),
            ExplicitJoinResolution::Denied
        );
    }

    #[test]
    fn explicit_join_denies_a_target_missing_from_the_catalogue() {
        // Defensive: legacy contiguous numbering makes a clamped
        // in-range number always resolvable, but NextExpress allows
        // gaps — a hole in the catalogue is treated as denied.
        let confs = vec![make_conf(1)];
        let user = make_user(&[(1, true)], None);
        assert_eq!(
            resolve_explicit_join(99, None, &user, &confs),
            ExplicitJoinResolution::Denied
        );
    }

    #[test]
    fn explicit_join_denies_when_user_has_no_grants_at_all() {
        // Even with zero grants explicit join only denies — it never
        // disconnects; the caller already holds a conference.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[], None);
        assert_eq!(
            resolve_explicit_join(1, None, &user, &confs),
            ExplicitJoinResolution::Denied
        );
    }

    #[test]
    fn explicit_join_denies_a_revoked_membership() {
        // The revoked row for conference 2 doesn't grant access; this
        // pins the `.filter(has_membership)` step.
        let confs = vec![make_conf(1), make_conf(2)];
        let user = make_user(&[(1, true), (2, false)], None);
        assert_eq!(
            resolve_explicit_join(2, None, &user, &confs),
            ExplicitJoinResolution::Denied
        );
    }
}
