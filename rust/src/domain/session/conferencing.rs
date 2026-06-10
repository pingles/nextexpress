//! Conference join, visit, and conference-scan transitions.

use std::time::SystemTime;

use crate::domain::conference::{first_accessible_conference, Conference, NameType};
use crate::domain::conference_visit::{
    next_accessible_conference_after, primary_msgbase_of, resolve_auto_rejoin,
    resolve_explicit_join, ConferenceScan, ConferenceVisit, ExplicitJoinResolution, JoinResolution,
};

use super::{
    AutoRejoinError, AutoRejoinOutcome, ConferenceScanOutcome, ExplicitJoinOutcome, LogoffReason,
    Session, SessionState,
};

impl Session {
    /// Returns this session's conference-visit history (spec:
    /// `conferences.allium:ConferenceVisit`). At most one entry has
    /// `left_at == None` thanks to
    /// [`Session::auto_rejoin_conference`] closing prior visits on
    /// every join — that's the
    /// `SessionsHaveAtMostOneOpenVisit` invariant.
    #[must_use]
    pub fn visits(&self) -> &[ConferenceVisit] {
        self.activity.visits()
    }

    /// Returns the visit currently open for this session, if any.
    /// Phase 4's join workflow (Slice 30) keeps this in lock-step
    /// with the bound user's `last_joined`.
    #[must_use]
    pub fn current_visit(&self) -> Option<&ConferenceVisit> {
        self.activity.current_visit()
    }

    /// Points the session's single open visit at `(conference_number,
    /// msgbase_number)` without the join bookkeeping — no
    /// `User.last_joined` update, no name-type promotion, no bulletin
    /// suppression. The `MS` read-it-now path uses it to aim the read
    /// flow at a base it found mail in, then restores the caller's home
    /// coordinate, mirroring the legacy transient `currentConf:=cn ...
    /// currentConf:=oldcn` around `displayMessage`/`replyPrompt`
    /// (`amiexpress/express.e:11750-11758`).
    pub fn attach_visit(&mut self, conference_number: u32, msgbase_number: u32, now: SystemTime) {
        self.activity.attach(conference_number, msgbase_number, now);
    }

    /// Resolves the auto-rejoin path of
    /// `conferences.allium:JoinConference` (Slice 30).
    ///
    /// On a successful resolution the session attaches a fresh
    /// [`ConferenceVisit`] and updates the bound user's
    /// `last_joined`. When the user has no granted membership for
    /// any catalogued conference the session moves to
    /// [`SessionState::LoggingOff`] with
    /// [`LogoffReason::NoConferenceAccess`].
    ///
    /// # Parameters
    /// - `conferences`: catalogue loaded by the
    ///   [`crate::domain::conference_repository::ConferenceRepository`],
    ///   in ascending `number` order.
    /// - `now`: timestamp recorded as `joined_at` on the new visit
    ///   (and `left_at` on any prior open visit).
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn auto_rejoin_conference(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<AutoRejoinOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let resolution = resolve_auto_rejoin(user, conferences);
        match resolution {
            JoinResolution::NoAccess => {
                self.move_to_logging_off(Some(LogoffReason::NoConferenceAccess));
                Ok(AutoRejoinOutcome::NoAccess)
            }
            JoinResolution::Resolved {
                conference,
                msgbase,
            } => {
                let conference_number = conference.number();
                let msgbase_number = msgbase.number();
                let conference_name_type = conference.accepted_name_type();
                user.record_join(conference, msgbase);
                let show_bulletin = !self.shared.quick_logon && !self.activity.is_scanning();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                self.activity.attach(conference_number, msgbase_number, now);
                Ok(AutoRejoinOutcome::Joined {
                    conference_number,
                    msgbase_number,
                    show_bulletin,
                    name_type_promoted_to,
                })
            }
        }
    }

    /// Updates `display_name_type` to `target` (spec:
    /// `conferences.allium:JoinedConferenceForNameType`, Slice 34).
    /// Returns `Some(target)` if the value changed and `None` if the
    /// session was already rendering that name-type, so callers can
    /// surface the change without keeping their own before/after
    /// state.
    fn promote_display_name_type(&mut self, target: NameType) -> Option<NameType> {
        if self.shared.display_name_type == target {
            None
        } else {
            self.shared.display_name_type = target;
            Some(target)
        }
    }

    /// Resolves the explicit-join path of
    /// `conferences.allium:JoinConference`
    /// (`reason = explicit_join`, Slice 32 / Tier C C2).
    ///
    /// Models the user typing `J <number>` from the menu. When the
    /// user has a granted membership for `target_conference_number`
    /// the session attaches there directly. Otherwise the request is
    /// denied and the session is left untouched — still in its
    /// current conference, still at the menu — mirroring the legacy
    /// `internalCommandJ` access check
    /// (`amiexpress/express.e:25156-25158`); the listener renders the
    /// "You do not have access to the requested conference" notice.
    /// Explicit join never logs the user off.
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn explicit_join_conference(
        &mut self,
        target_conference_number: u32,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ExplicitJoinOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let resolution = resolve_explicit_join(target_conference_number, user, conferences);
        match resolution {
            ExplicitJoinResolution::Denied => Ok(ExplicitJoinOutcome::Denied),
            ExplicitJoinResolution::Granted {
                conference,
                msgbase,
            } => {
                let conference_number = conference.number();
                let msgbase_number = msgbase.number();
                let conference_name_type = conference.accepted_name_type();
                user.record_join(conference, msgbase);
                let show_bulletin = !self.shared.quick_logon && !self.activity.is_scanning();
                let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
                self.activity.attach(conference_number, msgbase_number, now);
                Ok(ExplicitJoinOutcome::Joined {
                    conference_number,
                    msgbase_number,
                    show_bulletin,
                    name_type_promoted_to,
                })
            }
        }
    }

    /// Returns the in-progress conference-scan, if any
    /// (`conferences.allium:ConferenceScan`, Slice 33).
    #[must_use]
    pub fn conference_scan(&self) -> Option<&ConferenceScan> {
        self.activity.scan()
    }

    /// Starts a `CS` conference scan
    /// (`conferences.allium:StartConferenceScan`, Slice 33).
    ///
    /// Initialises a [`ConferenceScan`] with `next_conference`
    /// pointing at the first conference the user has access to,
    /// and runs the first scan step so the listener has a join
    /// outcome to display. When the user has no granted membership
    /// the session terminates with
    /// [`LogoffReason::NoConferenceAccess`].
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when the session is
    /// not in [`SessionState::Onboarded`] or [`SessionState::Menu`],
    /// or [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn start_conference_scan(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ConferenceScanOutcome, AutoRejoinError> {
        if !matches!(self.state(), SessionState::Onboarded | SessionState::Menu) {
            return Err(AutoRejoinError::WrongState(self.state()));
        }
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        let first = first_accessible_conference(user.memberships(), conferences);
        let Some(first_conference) = first else {
            self.move_to_logging_off(Some(LogoffReason::NoConferenceAccess));
            return Ok(ConferenceScanOutcome::NoAccess);
        };

        let first_number = first_conference.number();
        let mb = primary_msgbase_of(first_conference);
        let msgbase_number = mb.number();
        let conference_name_type = first_conference.accepted_name_type();
        user.record_join(first_conference, mb);
        let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
        // The next call to step_conference_scan will resume from the
        // conference *after* this one.
        self.activity
            .set_scan(Some(ConferenceScan::new(Some(first_number), now)));
        self.activity.attach(first_number, msgbase_number, now);
        Ok(ConferenceScanOutcome::Stepped {
            conference_number: first_number,
            msgbase_number,
            name_type_promoted_to,
        })
    }

    /// Advances the in-progress conference scan
    /// (`conferences.allium:StepConferenceScan` /
    /// `FinishConferenceScan`, Slice 33).
    ///
    /// Joins the scan's `next_conference`. When no more conferences
    /// remain, the scan finishes: `in_progress` is cleared and the
    /// session re-attaches to `User.last_joined` per the spec's
    /// "re-join the user's last conference at the end of the scan".
    ///
    /// # Errors
    /// Returns [`AutoRejoinError::WrongState`] when no scan is
    /// in progress on this session (the listener should call
    /// [`Self::start_conference_scan`] first), or
    /// [`AutoRejoinError::UserMissing`] when no user is bound.
    pub fn step_conference_scan(
        &mut self,
        conferences: &[Conference],
        now: SystemTime,
    ) -> Result<ConferenceScanOutcome, AutoRejoinError> {
        let Some(current_number) = self
            .activity
            .scan()
            .and_then(ConferenceScan::next_conference_number)
        else {
            return Err(AutoRejoinError::WrongState(self.state()));
        };
        let user = self.phase.user_mut().ok_or(AutoRejoinError::UserMissing)?;

        if let Some(next) = next_accessible_conference_after(user, conferences, current_number) {
            let next_number = next.number();
            let mb = primary_msgbase_of(next);
            let msgbase_number = mb.number();
            let conference_name_type = next.accepted_name_type();
            user.record_join(next, mb);
            let name_type_promoted_to = self.promote_display_name_type(conference_name_type);
            self.activity
                .set_scan(Some(ConferenceScan::new(Some(next_number), now)));
            self.activity.attach(next_number, msgbase_number, now);
            Ok(ConferenceScanOutcome::Stepped {
                conference_number: next_number,
                msgbase_number,
                name_type_promoted_to,
            })
        } else {
            // FinishConferenceScan: clear the scan and re-attach to
            // the user's last_joined (which during the scan was
            // updated to the last visited conference).
            self.activity.set_scan(None);
            let last = user.last_joined();
            Ok(ConferenceScanOutcome::Finished {
                rejoined_conference: last.map(|r| r.conference_number()),
            })
        }
    }
}
