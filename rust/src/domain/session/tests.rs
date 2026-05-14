//! Behavioural tests for [`crate::domain::session`].
//!
//! Tests are grouped by behaviour into nested sub-modules so the file
//! stays navigable. Shared helpers live in [`fixtures`] and reach into
//! private session fields (`SessionPhase`, `Session.phase`) — that is
//! only sound because this module is a child of `mod session`.

mod fixtures {
    use std::collections::BTreeSet;
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use crate::domain::conference::{Conference, NameType};
    use crate::domain::password::PasswordHashKind;

    pub(super) const DAILY_RESET_OFFSET: Duration = Duration::from_secs(6 * 3_600);

    pub(super) fn alice() -> User {
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    pub(super) fn new_session(channel: LogonChannel) -> Session {
        Session::new(1, channel, 9_600, SystemTime::UNIX_EPOCH)
    }

    pub(super) fn fresh_new_user(now: SystemTime) -> User {
        User::register_new(crate::domain::user::NewUserRegistration {
            slot_number: 7,
            handle: "newbie".to_string(),
            location: Some("Townsville".to_string()),
            phone_number: Some("555".to_string()),
            email: Some("n@example.com".to_string()),
            password_hash: "hash".to_string(),
            password_salt: Some("salt".to_string()),
            password_hash_kind: PasswordHashKind::Pbkdf210000,
            line_length: 80,
            ansi_colour: true,
            flags: BTreeSet::new(),
            ratio_mode: crate::domain::user::RatioMode::ByFiles,
            ratio_value: 3,
            now,
        })
        .expect("valid registration")
    }

    pub(super) fn authenticated_session() -> Session {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s
    }

    pub(super) fn authenticating_user_mut(session: &mut Session) -> &mut User {
        match &mut session.phase {
            SessionPhase::Authenticating { user, .. } => user,
            other => panic!("expected authenticating phase, got {:?}", other.state()),
        }
    }

    pub(super) fn set_authenticating_password_retry_count(session: &mut Session, count: u32) {
        match &mut session.phase {
            SessionPhase::Authenticating {
                password_retry_count,
                ..
            } => *password_retry_count = count,
            other => panic!("expected authenticating phase, got {:?}", other.state()),
        }
    }

    /// Drives a session into [`SessionState::Onboarded`] via raw state
    /// transitions, deliberately bypassing the rules
    /// [`apply_password_match`] fires on entry. The Slice 14 rule tests
    /// use this so they can drive [`initialise_daily_budget`] under
    /// controlled inputs.
    pub(super) fn session_at_onboarded_with(user: User) -> Session {
        let mut s = new_session(LogonChannel::Remote);
        s.phase = SessionPhase::Onboarded {
            user,
            authenticated_at: SystemTime::UNIX_EPOCH,
            time_remaining: Duration::ZERO,
        };
        s
    }

    /// Drives a session from connecting to menu via the rule chain.
    pub(super) fn session_at_menu() -> Session {
        let mut s = authenticated_session();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s
    }

    pub(super) fn user_with_time_limits(per_call: Duration, per_day: Duration) -> User {
        let mut u = alice();
        u.set_time_limits(per_call, per_day);
        u
    }

    pub(super) fn user_with_access_level(level: u8) -> User {
        User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            level,
        )
        .expect("valid user")
    }

    pub(super) fn make_conf(number: u32) -> Conference {
        use crate::domain::conference::MessageBase;
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid")
    }

    pub(super) fn make_conf_with_name_type(number: u32, name_type: NameType) -> Conference {
        use crate::domain::conference::MessageBase;
        Conference::with_name_type(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
            name_type,
        )
        .expect("valid")
    }

    pub(super) fn user_with_grants(grants: &[u32]) -> User {
        let mut user = alice();
        for g in grants {
            user.upsert_membership(crate::domain::conference::ConferenceMembership::new(
                *g, true,
            ));
        }
        user
    }
}

mod state_basics {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{alice, new_session};

    #[test]
    fn new_session_is_connecting() {
        let session = new_session(LogonChannel::Remote);
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.channel(), LogonChannel::Remote);
        assert_eq!(session.node_number(), 1);
        assert_eq!(session.online_baud(), 9_600);
        assert_eq!(session.connected_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.last_input_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.name_retry_count(), 0);
        assert_eq!(session.password_retry_count(), 0);
        assert!(session.user().is_none());
        assert!(session.typed_name().is_none());
        assert!(session.authenticated_at().is_none());
        assert!(session.logoff_at().is_none());
        assert!(session.logoff_reason().is_none());
    }

    #[test]
    fn is_remote_true_for_remote_and_ftp_only() {
        assert!(new_session(LogonChannel::Remote).is_remote());
        assert!(new_session(LogonChannel::Ftp).is_remote());
        assert!(!new_session(LogonChannel::Local).is_remote());
        assert!(!new_session(LogonChannel::SysopConsole).is_remote());
    }

    #[test]
    fn full_phase1_state_path_is_allowed() {
        use super::super::transitions::is_session_transition_allowed;
        assert!(is_session_transition_allowed(
            SessionState::Connecting,
            SessionState::Identifying
        ));
        assert!(is_session_transition_allowed(
            SessionState::Identifying,
            SessionState::Authenticating
        ));
        assert!(is_session_transition_allowed(
            SessionState::Authenticating,
            SessionState::Onboarded
        ));
        assert!(is_session_transition_allowed(
            SessionState::Onboarded,
            SessionState::Menu
        ));
        assert!(is_session_transition_allowed(
            SessionState::Menu,
            SessionState::LoggingOff
        ));
        assert!(is_session_transition_allowed(
            SessionState::LoggingOff,
            SessionState::Ended
        ));
    }

    #[test]
    fn invalid_transitions_are_rejected_by_the_spec_table() {
        // `Connecting -> Onboarded` is not in the spec's permitted
        // transition table — the helper rejects it directly.
        use super::super::transitions::is_session_transition_allowed;
        assert!(!is_session_transition_allowed(
            SessionState::Connecting,
            SessionState::Onboarded
        ));
    }

    #[test]
    fn unauthenticated_session_is_not_authenticated() {
        let session = new_session(LogonChannel::Remote);
        assert!(!session.is_authenticated());
    }

    #[test]
    fn onboarded_session_with_user_is_authenticated() {
        let mut session = new_session(LogonChannel::Remote);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", alice()).unwrap();
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert!(session.is_authenticated());
    }

    #[test]
    fn authenticating_with_user_is_not_yet_authenticated() {
        let mut session = new_session(LogonChannel::Remote);
        session.prompt_for_name().unwrap();
        session.record_identified_user("alice", alice()).unwrap();
        assert!(!session.is_authenticated());
    }

    #[test]
    fn accept_connection_creates_session_with_zero_retries() {
        let session = Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            None,
        )
        .expect("should accept");
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.node_number(), 3);
        assert_eq!(session.channel(), LogonChannel::Remote);
        assert_eq!(session.online_baud(), 9_600);
        assert_eq!(session.connected_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.last_input_at(), SystemTime::UNIX_EPOCH);
        assert_eq!(session.name_retry_count(), 0);
        assert_eq!(session.password_retry_count(), 0);
    }

    #[test]
    fn accept_connection_rejects_when_active_session_exists() {
        let existing = Session::new(3, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        let err = Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            Some(&existing),
        )
        .expect_err("active session should block accept");
        assert_eq!(err, AcceptConnectionError::AlreadyActiveSession);
    }

    #[test]
    fn accept_connection_allows_when_existing_session_ended() {
        // Drive the existing session through the legitimate rule chain
        // to Ended (CarrierLost -> finalise_logoff) rather than via a
        // raw transition.
        let mut existing = Session::new(3, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        existing.apply_carrier_loss().unwrap();
        existing.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(existing.state(), SessionState::Ended);
        Session::accept_connection(
            3,
            LogonChannel::Remote,
            9_600,
            SystemTime::UNIX_EPOCH,
            Some(&existing),
        )
        .expect("ended session should not block accept");
    }

    #[test]
    fn record_input_updates_last_input_at() {
        let mut s = new_session(LogonChannel::Remote);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        s.record_input(later);
        assert_eq!(s.last_input_at(), later);
    }

    #[test]
    fn quick_logon_round_trips_via_setter() {
        let mut s = new_session(LogonChannel::Remote);
        assert!(!s.quick_logon());
        s.set_quick_logon(true);
        assert!(s.quick_logon());
        s.set_quick_logon(false);
        assert!(!s.quick_logon());
    }

    #[test]
    fn new_session_has_no_visits() {
        let s = new_session(LogonChannel::Remote);
        assert!(s.visits().is_empty());
        assert!(s.current_visit().is_none());
    }

    #[test]
    fn new_session_starts_with_handle_display_name_type() {
        let s = new_session(LogonChannel::Remote);
        assert_eq!(s.display_name_type(), NameType::Handle);
    }
}

mod identification {
    use std::time::SystemTime;

    use super::super::*;
    use super::fixtures::{alice, new_session};

    #[test]
    fn prompt_for_name_moves_to_identifying() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().expect("connecting -> identifying");
        assert_eq!(s.state(), SessionState::Identifying);
    }

    #[test]
    fn prompt_for_name_rejects_outside_connecting() {
        // A second prompt_for_name from Identifying must fail with
        // `from = Identifying` — the rule's only legitimate firing
        // is from Connecting.
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let err = s
            .prompt_for_name()
            .expect_err("identifying -> identifying not allowed");
        assert_eq!(err.from, SessionState::Identifying);
    }

    #[test]
    fn name_typed_found_advances_to_authenticating() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_identified_user("alice", alice())
            .expect("name_typed");
        assert_eq!(outcome, NameTypedOutcome::Authenticated);
        assert_eq!(s.state(), SessionState::Authenticating);
        assert_eq!(s.typed_name(), Some("alice"));
        assert_eq!(s.user().map(User::handle), Some("alice"));
    }

    #[test]
    fn name_typed_not_found_increments_retry() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, NameTypedOutcome::NotFound);
        assert_eq!(s.state(), SessionState::Identifying);
        assert_eq!(s.name_retry_count(), 1);
    }

    #[test]
    fn name_typed_five_strikes_ends_session() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        for _ in 0..4 {
            assert_eq!(
                s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap(),
                NameTypedOutcome::NotFound
            );
        }
        assert_eq!(s.name_retry_count(), 4);
        let outcome = s.record_unknown_name(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, NameTypedOutcome::SessionEnded);
        assert_eq!(s.state(), SessionState::Ended);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
        assert_eq!(s.logoff_at(), Some(SystemTime::UNIX_EPOCH));
    }

    #[test]
    fn name_typed_outside_identifying_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .record_identified_user("alice", alice())
            .expect_err("must be in identifying");
        assert!(matches!(err, NameTypedError::WrongState(_)));
    }

    #[test]
    fn name_typed_new_keyword_transitions_to_new_user_registering() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            outcome,
            NewUserRequestOutcome::Initialised {
                password_required: false
            }
        );
        assert_eq!(s.state(), SessionState::NewUserRegistering);
        assert!(
            s.new_user_password_verified(),
            "no gate required => verified"
        );
        assert_eq!(s.new_user_password_attempts(), 0);
        assert_eq!(s.name_retry_count(), 0);
    }

    #[test]
    fn record_new_user_request_with_gate_arms_unverified() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(
            outcome,
            NewUserRequestOutcome::Initialised {
                password_required: true
            }
        );
        assert!(!s.new_user_password_verified());
        assert_eq!(s.new_user_password_attempts(), 0);
    }

    #[test]
    fn record_new_user_request_with_disallowed_registration_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let outcome = s
            .record_new_user_request(false, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserRequestOutcome::Rejected);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
    }

    #[test]
    fn record_new_user_request_outside_identifying_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .expect_err("must be in identifying");
        assert!(matches!(err, NameTypedError::WrongState(_)));
    }

    #[test]
    fn carrier_loss_from_new_user_registering_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_carrier_loss().expect("permitted");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn idle_timeout_from_new_user_registering_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_idle_timeout(true).expect("permitted");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::InputTimeout));
    }

    #[test]
    fn apply_new_user_password_attempt_match_marks_verified() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let (outcome, entry) = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Verified);
        assert!(entry.is_none());
        assert!(s.new_user_password_verified());
    }

    #[test]
    fn apply_new_user_password_attempt_mismatch_increments_and_logs() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let (outcome, entry) = s
            .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::Mismatch);
        let entry = entry.expect("caller-log entry");
        assert!(entry.text.contains("New-user password failure"));
        assert!(entry.is_password_failure);
        assert_eq!(s.new_user_password_attempts(), 1);
        assert!(!s.new_user_password_verified());
        assert_eq!(s.state(), SessionState::NewUserRegistering);
    }

    #[test]
    fn apply_new_user_password_attempt_max_failures_logs_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        for _ in 0..2 {
            let (outcome, _) = s
                .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
                .unwrap();
            assert_eq!(outcome, NewUserPasswordOutcome::Mismatch);
        }
        let (outcome, entry) = s
            .apply_new_user_password_attempt(false, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(outcome, NewUserPasswordOutcome::TooManyFailures);
        assert!(entry.is_some());
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
    }

    #[test]
    fn apply_new_user_password_attempt_already_verified_errors() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        let err = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .expect_err("already verified should error");
        assert_eq!(err, VerifyNewUserPasswordError::AlreadyVerified);
    }

    #[test]
    fn apply_new_user_password_attempt_outside_new_user_registering_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .expect_err("must be in new_user_registering");
        assert!(matches!(err, VerifyNewUserPasswordError::WrongState(_)));
    }
}

mod new_user_registration {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{fresh_new_user, new_session};

    #[test]
    fn complete_new_user_registration_binds_user_and_onboards() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
            .unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let rejection = s
            .complete_new_user_registration(fresh_new_user(now), SessionPolicy::default(), now)
            .expect("valid");
        assert!(rejection.is_none(), "fresh new user should not be rejected");
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.authenticated_at(), Some(now));
        assert_eq!(s.user().map(User::handle), Some("newbie"));
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn complete_new_user_registration_blocked_by_unverified_gate() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        let err = s
            .complete_new_user_registration(
                fresh_new_user(SystemTime::UNIX_EPOCH),
                SessionPolicy::default(),
                SystemTime::UNIX_EPOCH,
            )
            .expect_err("gate not verified should error");
        assert_eq!(err, CompleteNewUserRegistrationError::GateNotVerified);
        assert_eq!(s.state(), SessionState::NewUserRegistering);
    }

    #[test]
    fn complete_new_user_registration_succeeds_after_gate_passes() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_new_user_request(true, true, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.apply_new_user_password_attempt(true, 3, SystemTime::UNIX_EPOCH)
            .unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        s.complete_new_user_registration(fresh_new_user(now), SessionPolicy::default(), now)
            .expect("valid after gate passes");
        assert_eq!(s.state(), SessionState::Onboarded);
    }

    #[test]
    fn complete_new_user_registration_outside_new_user_registering_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .complete_new_user_registration(
                fresh_new_user(SystemTime::UNIX_EPOCH),
                SessionPolicy::default(),
                SystemTime::UNIX_EPOCH,
            )
            .expect_err("must be in new_user_registering");
        assert!(matches!(
            err,
            CompleteNewUserRegistrationError::WrongState(_)
        ));
    }
}

mod authentication {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{
        alice, authenticated_session, authenticating_user_mut, new_session,
        set_authenticating_password_retry_count,
    };

    #[test]
    fn verify_password_match_advances_to_onboarded() {
        let mut s = authenticated_session();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        let (outcome, rejection) =
            apply_password_match(&mut s, SessionPolicy::default(), now).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::Authenticated);
        assert!(rejection.is_none());
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.authenticated_at(), Some(now));
        assert!(s.is_authenticated());
    }

    #[test]
    fn verify_password_match_clears_user_attempts() {
        let mut s = authenticated_session();
        authenticating_user_mut(&mut s).bump_invalid_attempts();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_match_fires_initialise_daily_budget() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn verify_password_mismatch_bumps_counters() {
        let mut s = authenticated_session();
        let (outcome, entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(3), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::NotMatching);
        assert_eq!(s.state(), SessionState::Authenticating);
        assert_eq!(s.password_retry_count(), 1);
        assert_eq!(s.user().unwrap().invalid_attempts(), 1);
        assert_eq!(entry.text, "Password failure");
        assert!(entry.is_password_failure);
    }

    #[test]
    fn session_policy_continues_below_password_failure_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 1);
        authenticating_user_mut(&mut s).bump_invalid_attempts();

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::Continue
        );
    }

    #[test]
    fn session_policy_locks_account_when_user_failures_reach_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 3);
        for _ in 0..3 {
            authenticating_user_mut(&mut s).bump_invalid_attempts();
        }

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::LockAccount
        );
    }

    #[test]
    fn session_policy_ends_session_when_session_failures_reach_limit() {
        let mut s = authenticated_session();
        set_authenticating_password_retry_count(&mut s, 3);

        assert_eq!(
            SessionPolicy::new(3).password_failure_decision(&s),
            PasswordFailureDecision::EndSession
        );
    }

    #[test]
    fn verify_password_locks_account_when_user_attempts_reach_max() {
        let mut s = authenticated_session();
        let (outcome, _entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(1), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::AccountLocked);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
        assert!(s.user().unwrap().is_account_locked());
        assert_eq!(s.user().unwrap().invalid_attempts(), 0);
    }

    #[test]
    fn verify_password_session_level_trip_fires_when_user_counter_reset() {
        // The session-level check (`password_retry_count >= max`)
        // only fires when the user-level counter happens to be below
        // max. In normal operation both counters track 1:1, so the
        // user-level check wins. This test manually clears the user
        // counter mid-session to exercise the session-level branch.
        let mut s = authenticated_session();
        apply_password_mismatch(&mut s, SessionPolicy::new(5), SystemTime::UNIX_EPOCH).unwrap();
        apply_password_mismatch(&mut s, SessionPolicy::new(5), SystemTime::UNIX_EPOCH).unwrap();
        authenticating_user_mut(&mut s).clear_invalid_attempts();
        let (outcome, _entry) =
            apply_password_mismatch(&mut s, SessionPolicy::new(3), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::TooManyFailures);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(
            s.logoff_reason(),
            Some(LogoffReason::ExcessivePasswordFails)
        );
        assert!(!s.user().unwrap().is_account_locked());
    }

    #[test]
    fn verify_password_outside_authenticating_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH)
            .expect_err("must be authenticating");
        assert!(matches!(err, VerifyPasswordError::WrongState(_)));
    }
}

mod lifecycle {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{authenticated_session, new_session, session_at_menu};

    #[test]
    fn enter_menu_advances_state_and_logs() {
        let mut s = authenticated_session();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(120);
        let entry = s.enter_menu(now).unwrap();
        assert_eq!(s.state(), SessionState::Menu);
        assert_eq!(s.user().unwrap().times_called(), 1);
        assert!(
            entry.text.contains("Logon:")
                && entry.text.contains("alice")
                && !entry.is_password_failure,
            "expected logon caller-log entry, got {entry:?}"
        );
    }

    #[test]
    fn enter_menu_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("must be onboarded");
        assert!(matches!(err, EnterMenuError::WrongState(_)));
    }

    #[test]
    fn user_requests_logoff_from_menu_records_normal_logoff() {
        let mut s = session_at_menu();
        s.user_requests_logoff().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NormalLogoff));
    }

    #[test]
    fn user_requests_logoff_from_onboarded_is_allowed() {
        let mut s = authenticated_session();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        s.user_requests_logoff().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn user_requests_logoff_outside_menu_or_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .user_requests_logoff()
            .expect_err("connecting cannot log off");
        assert_eq!(err.from, SessionState::Connecting);
    }

    #[test]
    fn finalise_logoff_updates_user_and_logs_goodbye() {
        let mut s = session_at_menu();
        s.user_requests_logoff().unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let entry = s.finalise_logoff(now).unwrap();
        assert_eq!(s.state(), SessionState::Ended);
        assert_eq!(s.logoff_at(), Some(now));
        assert_eq!(s.user().unwrap().last_call(), Some(now));
        assert!(
            entry.text.contains("Logoff:") && entry.text.contains("alice"),
            "expected logoff caller-log entry, got {entry:?}"
        );
    }

    #[test]
    fn finalise_logoff_outside_logging_off_errors() {
        let mut s = session_at_menu();
        let err = s
            .finalise_logoff(SystemTime::UNIX_EPOCH)
            .expect_err("must be logging_off");
        assert_eq!(err.from, SessionState::Menu);
    }
}

mod time_budget {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::super::log_format::floor_to_day;
    use super::super::*;
    use super::fixtures::{
        new_session, session_at_onboarded_with, user_with_time_limits, DAILY_RESET_OFFSET,
    };

    #[test]
    fn floor_to_day_buckets_into_24h_groups_offset_by_six_hours() {
        // Six hours past UNIX_EPOCH is the start of "day 0".
        let day_zero = UNIX_EPOCH + Duration::from_secs(6 * 3_600);
        assert_eq!(floor_to_day(day_zero, DAILY_RESET_OFFSET), 0);
        let just_before = day_zero - Duration::from_secs(1);
        assert_eq!(floor_to_day(just_before, DAILY_RESET_OFFSET), -1);
        let later_same_day = day_zero + Duration::from_secs(20 * 3_600);
        assert_eq!(floor_to_day(later_same_day, DAILY_RESET_OFFSET), 0);
        let next_day = day_zero + Duration::from_secs(24 * 3_600);
        assert_eq!(floor_to_day(next_day, DAILY_RESET_OFFSET), 1);
    }

    #[test]
    fn initialise_daily_budget_first_call_treats_as_new_day() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(30 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        assert_eq!(s.user().unwrap().times_called_today(), 0);
        assert_eq!(s.user().unwrap().time_used_today(), Duration::ZERO);
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_same_day_bumps_times_called_today() {
        let mut user =
            user_with_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        let earlier_today = UNIX_EPOCH + Duration::from_secs(7 * 3_600);
        user.record_last_call(earlier_today);
        user.add_time_used_today(Duration::from_secs(120));
        user.bump_times_called_today();
        let mut s = session_at_onboarded_with(user);

        let later_today = UNIX_EPOCH + Duration::from_secs(20 * 3_600);
        initialise_daily_budget(&mut s, later_today, DAILY_RESET_OFFSET).unwrap();
        assert_eq!(s.user().unwrap().times_called_today(), 2);
        assert_eq!(
            s.user().unwrap().time_used_today(),
            Duration::from_secs(120)
        );
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_new_day_after_previous_day_resets() {
        let mut user =
            user_with_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        let yesterday = UNIX_EPOCH + Duration::from_secs(10 * 3_600);
        user.record_last_call(yesterday);
        user.add_time_used_today(Duration::from_secs(900));
        user.bump_times_called_today();
        user.bump_times_called_today();
        let mut s = session_at_onboarded_with(user);

        let today = UNIX_EPOCH + Duration::from_secs(36 * 3_600);
        initialise_daily_budget(&mut s, today, DAILY_RESET_OFFSET).unwrap();
        assert_eq!(s.user().unwrap().times_called_today(), 0);
        assert_eq!(s.user().unwrap().time_used_today(), Duration::ZERO);
        assert_eq!(s.time_remaining(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn initialise_daily_budget_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET)
            .expect_err("must be onboarded");
        assert!(matches!(err, InitialiseDailyBudgetError::WrongState(_)));
    }

    #[test]
    fn tick_minute_decrements_remaining_and_accumulates_used() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(5 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::Continued);
        assert_eq!(s.time_remaining(), Duration::from_secs(4 * 60));
        assert_eq!(s.user().unwrap().time_used_today(), Duration::from_secs(60));
    }

    #[test]
    fn tick_minute_in_menu_state_works_too() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(5 * 60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::Continued);
        assert_eq!(s.state(), SessionState::Menu);
    }

    #[test]
    fn tick_minute_at_zero_logs_off_with_out_of_time() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::OutOfTime));
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn tick_minute_outside_onboarded_or_menu_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = tick_minute(&mut s).expect_err("must be onboarded/menu");
        assert!(matches!(err, TickMinuteError::WrongState(_)));
    }

    #[test]
    fn tick_minute_saturates_does_not_underflow() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::ZERO,
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        let outcome = tick_minute(&mut s).unwrap();
        assert_eq!(outcome, TickMinuteOutcome::TimeExpired);
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn finalise_logoff_after_out_of_time_logs_reason() {
        let mut s = session_at_onboarded_with(user_with_time_limits(
            Duration::from_secs(60),
            Duration::from_secs(60 * 60),
        ));
        initialise_daily_budget(&mut s, SystemTime::UNIX_EPOCH, DAILY_RESET_OFFSET).unwrap();
        tick_minute(&mut s).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("out_of_time"),
            "expected out_of_time in logoff line, got {entry:?}"
        );
    }
}

mod password_reset {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::super::*;
    use super::fixtures::{alice, new_session, session_at_onboarded_with};
    use crate::domain::password::PasswordHashKind;

    #[test]
    fn force_password_reset_sets_flag_when_expiry_elapsed() {
        let user = alice();
        let mut s = session_at_onboarded_with(user.clone());
        let now = UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        assert!(s.user().unwrap().force_password_reset());
        assert!(!user.force_password_reset());
    }

    #[test]
    fn force_password_reset_keeps_flag_when_expiry_not_elapsed() {
        let mut s = session_at_onboarded_with(alice());
        let now = UNIX_EPOCH + Duration::from_secs(3 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_disabled_at_zero_days() {
        let mut s = session_at_onboarded_with(alice());
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        force_password_reset_if_due(&mut s, 0, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_preserves_flag_already_set_by_sysop() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        force_password_reset_if_due(&mut s, 0, UNIX_EPOCH).unwrap();
        assert!(s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_no_op_for_locked_account() {
        let mut user = alice();
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * 86_400);
        force_password_reset_if_due(&mut s, 7, now).unwrap();
        assert!(!s.user().unwrap().force_password_reset());
    }

    #[test]
    fn force_password_reset_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = force_password_reset_if_due(&mut s, 7, SystemTime::UNIX_EPOCH)
            .expect_err("must be onboarded");
        assert!(matches!(err, ForcePasswordResetError::WrongState(_)));
    }

    #[test]
    fn apply_password_match_fires_force_password_reset_when_expired() {
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(60), Duration::from_secs(60));
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        let policy = SessionPolicy::default().with_password_expiry_days(1);
        let now = UNIX_EPOCH + Duration::from_secs(7 * 86_400);
        apply_password_match(&mut s, policy, now).unwrap();
        assert!(s.user().unwrap().force_password_reset());
    }

    #[test]
    fn enter_menu_blocked_when_force_password_reset_set() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("flag should block enter_menu");
        assert!(matches!(err, EnterMenuError::PasswordResetPending));
        assert_eq!(s.state(), SessionState::Onboarded);
        assert_eq!(s.user().unwrap().times_called(), 0);
    }

    #[test]
    fn apply_password_change_replaces_credentials_and_clears_flag() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        let later = UNIX_EPOCH + Duration::from_secs(5_000);
        apply_password_change(
            &mut s,
            "fresh".to_string(),
            Some("freshsalt".to_string()),
            PasswordHashKind::Pbkdf210000,
            later,
        )
        .unwrap();
        let saved = s.user().unwrap();
        assert_eq!(saved.password_hash(), "fresh");
        assert_eq!(saved.password_salt(), Some("freshsalt"));
        assert_eq!(saved.password_last_updated(), later);
        assert!(!saved.force_password_reset());
    }

    #[test]
    fn apply_password_change_unblocks_enter_menu() {
        let mut user = alice();
        user.set_force_password_reset(true);
        let mut s = session_at_onboarded_with(user);
        apply_password_change(
            &mut s,
            "fresh".to_string(),
            Some("freshsalt".to_string()),
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.state(), SessionState::Menu);
    }

    #[test]
    fn apply_password_change_outside_onboarded_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = apply_password_change(
            &mut s,
            "fresh".to_string(),
            None,
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("must be onboarded");
        assert!(matches!(err, CompletePasswordResetError::WrongState(_)));
    }

    #[test]
    fn apply_password_change_without_pending_reset_errors() {
        let mut s = session_at_onboarded_with(alice());
        let err = apply_password_change(
            &mut s,
            "fresh".to_string(),
            None,
            PasswordHashKind::Pbkdf210000,
            SystemTime::UNIX_EPOCH,
        )
        .expect_err("flag not set");
        assert!(matches!(err, CompletePasswordResetError::ResetNotPending));
    }
}

mod access_rejection {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{alice, new_session, session_at_onboarded_with, user_with_access_level};

    #[test]
    fn reject_locked_account_transitions_to_logging_off() {
        let mut user = alice();
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        let outcome = s
            .reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("locked user should be rejected");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
        assert_eq!(
            outcome.text,
            "Logon rejected: account locked or below access threshold"
        );
        assert!(!outcome.is_password_failure);
    }

    #[test]
    fn reject_low_access_uses_new_user_rejected_reason() {
        let mut s = session_at_onboarded_with(user_with_access_level(1));
        let outcome = s
            .reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("low-access user should be rejected");
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NewUserRejected));
        assert!(outcome.text.contains("Logon rejected"));
    }

    #[test]
    fn reject_account_locked_with_low_access_still_uses_locked_account() {
        let mut user = user_with_access_level(0);
        user.lock_account();
        let mut s = session_at_onboarded_with(user);
        s.reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH)
            .expect("locked user should be rejected");
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
    }

    #[test]
    fn reject_no_op_for_normal_user() {
        let mut s = session_at_onboarded_with(alice());
        let outcome = s.reject_locked_or_insufficient_access(SystemTime::UNIX_EPOCH);
        assert!(outcome.is_none());
        assert_eq!(s.state(), SessionState::Onboarded);
        assert!(s.logoff_reason().is_none());
    }

    #[test]
    fn apply_password_match_returns_logon_rejected_for_locked_user() {
        let mut user = alice();
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        let (outcome, rejection) =
            apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(outcome, VerifyPasswordOutcome::LogonRejected);
        assert!(rejection.is_some());
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::LockedAccount));
    }

    #[test]
    fn apply_password_match_short_circuits_other_rules_when_rejected() {
        let mut user = alice();
        user.set_time_limits(Duration::from_secs(30 * 60), Duration::from_secs(60 * 60));
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn locked_user_cannot_reach_menu() {
        let mut user = alice();
        user.lock_account();
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", user).unwrap();
        apply_password_match(&mut s, SessionPolicy::default(), SystemTime::UNIX_EPOCH).unwrap();
        assert_ne!(s.state(), SessionState::Menu);
        let err = s
            .enter_menu(SystemTime::UNIX_EPOCH)
            .expect_err("LoggingOff cannot enter Menu");
        assert!(matches!(err, EnterMenuError::WrongState(_)));
    }
}

mod activity {
    use std::time::SystemTime;

    use super::super::*;
    use super::fixtures::{alice, new_session, session_at_onboarded_with};

    #[test]
    fn idle_timeout_from_identifying_uses_carrier_loss_by_default() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn idle_timeout_treat_as_logoff_uses_input_timeout_reason() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_idle_timeout(true).unwrap();
        assert_eq!(s.logoff_reason(), Some(LogoffReason::InputTimeout));
    }

    #[test]
    fn idle_timeout_from_authenticating_allowed() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s.apply_idle_timeout(true).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_onboarded_allowed() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_menu_allowed() {
        let mut s = session_at_onboarded_with(alice());
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s.apply_idle_timeout(false).unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
    }

    #[test]
    fn idle_timeout_from_connecting_errors() {
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .apply_idle_timeout(false)
            .expect_err("connecting not allowed");
        assert!(matches!(
            err,
            IdleTimeoutError::WrongState(SessionState::Connecting)
        ));
    }

    #[test]
    fn idle_timeout_from_logging_off_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        let err = s
            .apply_idle_timeout(false)
            .expect_err("logging_off not allowed");
        assert!(matches!(
            err,
            IdleTimeoutError::WrongState(SessionState::LoggingOff)
        ));
    }

    #[test]
    fn finalise_logoff_after_idle_timeout_writes_reason_to_log_line() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(true).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("input_timeout"),
            "expected input_timeout in goodbye line, got {entry:?}"
        );
    }

    #[test]
    fn carrier_loss_from_connecting_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_identifying_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_authenticating_transitions_to_logging_off() {
        let mut s = new_session(LogonChannel::Remote);
        s.prompt_for_name().unwrap();
        s.record_identified_user("alice", alice()).unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_onboarded_transitions_to_logging_off() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_menu_transitions_to_logging_off() {
        let mut s = session_at_onboarded_with(alice());
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        s.apply_carrier_loss().unwrap();
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::CarrierLoss));
    }

    #[test]
    fn carrier_loss_from_logging_off_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        let err = s
            .apply_carrier_loss()
            .expect_err("logging_off cannot fire CarrierLost again");
        assert!(matches!(
            err,
            CarrierLostError::WrongState(SessionState::LoggingOff)
        ));
    }

    #[test]
    fn carrier_loss_from_ended_errors() {
        let mut s = session_at_onboarded_with(alice());
        s.user_requests_logoff().unwrap();
        s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        let err = s
            .apply_carrier_loss()
            .expect_err("ended cannot fire CarrierLost");
        assert!(matches!(
            err,
            CarrierLostError::WrongState(SessionState::Ended)
        ));
    }

    #[test]
    fn finalise_logoff_after_carrier_loss_writes_reason() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_carrier_loss().unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("carrier_loss"),
            "expected carrier_loss in goodbye line, got {entry:?}"
        );
    }

    #[test]
    fn finalise_logoff_after_carrier_loss_treatment_writes_reason_to_log_line() {
        let mut s = session_at_onboarded_with(alice());
        s.apply_idle_timeout(false).unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("carrier_loss"),
            "expected carrier_loss in goodbye line, got {entry:?}"
        );
    }
}

mod conferencing {
    use std::time::{Duration, SystemTime};

    use super::super::*;
    use super::fixtures::{
        alice, make_conf, make_conf_with_name_type, new_session, session_at_onboarded_with,
        user_with_grants,
    };

    #[test]
    fn auto_rejoin_attaches_session_to_first_accessible_conference() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[2]));
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
        let outcome = s.auto_rejoin_conference(&confs, now).expect("ok");
        assert_eq!(
            outcome,
            AutoRejoinOutcome::Joined {
                conference_number: 2,
                msgbase_number: 1,
                show_bulletin: true,
                name_type_promoted_to: None,
            }
        );
        let visit = s.current_visit().expect("open visit");
        assert_eq!(visit.conference_number(), 2);
        assert_eq!(visit.msgbase_number(), 1);
        assert_eq!(visit.joined_at(), now);
        assert_eq!(
            s.user().unwrap().last_joined().unwrap().conference_number(),
            2
        );
    }

    #[test]
    fn auto_rejoin_prefers_users_last_joined_when_still_accessible() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut user = user_with_grants(&[1, 2, 3]);
        user.record_join(&confs[2], &confs[2].msgbases()[0]);
        let mut s = session_at_onboarded_with(user);
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert!(matches!(
            outcome,
            AutoRejoinOutcome::Joined {
                conference_number: 3,
                ..
            }
        ));
    }

    #[test]
    fn auto_rejoin_with_no_grants_moves_to_logging_off_with_no_conference_access() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, AutoRejoinOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
        assert!(s.current_visit().is_none());
    }

    #[test]
    fn auto_rejoin_closes_prior_open_visit_before_attaching_new_one() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t2 = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        s.auto_rejoin_conference(&confs[..1], t1).expect("ok");
        s.phase
            .user_mut()
            .unwrap()
            .record_join(&confs[1], &confs[1].msgbases()[0]);
        s.auto_rejoin_conference(&confs, t2).expect("ok");

        let open: Vec<_> = s.visits().iter().filter(|v| v.is_open()).collect();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].conference_number(), 2);

        let closed: Vec<_> = s.visits().iter().filter(|v| !v.is_open()).collect();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].conference_number(), 1);
        assert_eq!(closed[0].left_at(), Some(t2));
    }

    #[test]
    fn auto_rejoin_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn auto_rejoin_clears_show_bulletin_when_session_is_in_quick_logon_mode() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        s.set_quick_logon(true);
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined { show_bulletin, .. } => {
                assert!(
                    !show_bulletin,
                    "quick_logon should suppress the conference bulletin"
                );
            }
            AutoRejoinOutcome::NoAccess => panic!("expected Joined, got NoAccess"),
        }
    }

    #[test]
    fn explicit_join_attaches_directly_when_user_has_access_to_target() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2, 3]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                msgbase_number,
                matched_request,
                show_bulletin,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(msgbase_number, 1);
                assert!(matched_request);
                assert!(show_bulletin);
            }
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn explicit_join_falls_through_with_matched_request_false_when_no_access_to_target() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                conference_number,
                matched_request,
                ..
            } => {
                assert_eq!(conference_number, 1);
                assert!(!matched_request);
            }
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined fallback"),
        }
    }

    #[test]
    fn explicit_join_with_no_grants_anywhere_terminates_session_with_no_conference_access() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, ExplicitJoinOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
    }

    #[test]
    fn explicit_join_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn explicit_join_from_menu_state_is_allowed() {
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.enter_menu(SystemTime::UNIX_EPOCH).expect("enter menu");
        assert_eq!(s.state(), SessionState::Menu);
        s.explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn explicit_join_clears_show_bulletin_under_quick_logon() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        s.set_quick_logon(true);
        let outcome = s
            .explicit_join_conference(1, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined { show_bulletin, .. } => assert!(!show_bulletin),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn start_conference_scan_attaches_to_first_accessible_conference_and_marks_in_progress() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3)];
        let mut s = session_at_onboarded_with(user_with_grants(&[2, 3]));
        let outcome = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ConferenceScanOutcome::Stepped {
                conference_number,
                msgbase_number,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(msgbase_number, 1);
            }
            other => panic!("expected Stepped, got {other:?}"),
        }
        let scan = s.conference_scan().expect("scan in progress");
        assert_eq!(scan.next_conference_number(), Some(2));
        assert_eq!(s.current_visit().unwrap().conference_number(), 2);
    }

    #[test]
    fn step_conference_scan_advances_through_each_accessible_conference() {
        let confs = vec![make_conf(1), make_conf(2), make_conf(3), make_conf(5)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 3, 5]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Stepped {
                conference_number, ..
            } => assert_eq!(conference_number, 3),
            other => panic!("expected Stepped, got {other:?}"),
        }
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Stepped {
                conference_number, ..
            } => assert_eq!(conference_number, 5),
            other => panic!("expected Stepped, got {other:?}"),
        }
        match s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap()
        {
            ConferenceScanOutcome::Finished {
                rejoined_conference,
            } => assert_eq!(rejoined_conference, Some(5)),
            other => panic!("expected Finished, got {other:?}"),
        }
        assert!(s.conference_scan().is_none());
    }

    #[test]
    fn start_conference_scan_with_no_grants_terminates_session() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        let outcome = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        assert_eq!(outcome, ConferenceScanOutcome::NoAccess);
        assert_eq!(s.state(), SessionState::LoggingOff);
        assert_eq!(s.logoff_reason(), Some(LogoffReason::NoConferenceAccess));
    }

    #[test]
    fn step_conference_scan_without_a_started_scan_errors() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let err = s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("no scan in progress");
        assert!(matches!(err, AutoRejoinError::WrongState(_)));
    }

    #[test]
    fn auto_rejoin_during_active_scan_suppresses_bulletin() {
        // While a scan is in progress, ShowConferenceBulletin is
        // suppressed (`conferences.allium:ShowConferenceBulletin`).
        let confs = vec![make_conf(1), make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined { show_bulletin, .. } => {
                assert!(
                    !show_bulletin,
                    "scan-in-progress should suppress conference bulletin"
                );
            }
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn start_conference_scan_outside_onboarded_or_menu_errors() {
        let confs = vec![make_conf(1)];
        let mut s = new_session(LogonChannel::Remote);
        let err = s
            .start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .expect_err("wrong state");
        assert_eq!(err, AutoRejoinError::WrongState(SessionState::Connecting));
    }

    #[test]
    fn auto_rejoin_into_real_name_conference_promotes_display_name_type() {
        let confs = vec![make_conf_with_name_type(1, NameType::RealName)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, Some(NameType::RealName)),
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::RealName);
    }

    #[test]
    fn auto_rejoin_into_handle_conference_does_not_signal_promotion() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(user_with_grants(&[1]));
        let outcome = s
            .auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            AutoRejoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, None),
            AutoRejoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::Handle);
    }

    #[test]
    fn explicit_join_promotes_display_name_type_when_target_uses_internet_names() {
        let confs = vec![
            make_conf(1),
            make_conf_with_name_type(2, NameType::InternetName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .expect("ok");
        match outcome {
            ExplicitJoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, Some(NameType::InternetName)),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
        assert_eq!(s.display_name_type(), NameType::InternetName);
    }

    #[test]
    fn conference_scan_step_promotes_display_name_type_when_visiting_real_name_conf() {
        let confs = vec![
            make_conf(1),
            make_conf_with_name_type(2, NameType::RealName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.start_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let stepped = s
            .step_conference_scan(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        match stepped {
            ConferenceScanOutcome::Stepped {
                conference_number,
                name_type_promoted_to,
                ..
            } => {
                assert_eq!(conference_number, 2);
                assert_eq!(name_type_promoted_to, Some(NameType::RealName));
            }
            other => panic!("expected Stepped, got {other:?}"),
        }
        assert_eq!(s.display_name_type(), NameType::RealName);
    }

    #[test]
    fn rejoining_same_name_type_conference_signals_no_promotion() {
        let confs = vec![
            make_conf_with_name_type(1, NameType::RealName),
            make_conf_with_name_type(2, NameType::RealName),
        ];
        let mut s = session_at_onboarded_with(user_with_grants(&[1, 2]));
        s.auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let outcome = s
            .explicit_join_conference(2, &confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        match outcome {
            ExplicitJoinOutcome::Joined {
                name_type_promoted_to,
                ..
            } => assert_eq!(name_type_promoted_to, None),
            ExplicitJoinOutcome::NoAccess => panic!("expected Joined"),
        }
    }

    #[test]
    fn auto_rejoin_finalise_logoff_log_line_includes_no_conference_access_reason() {
        let confs = vec![make_conf(1)];
        let mut s = session_at_onboarded_with(alice());
        s.auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        let entry = s.finalise_logoff(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            entry.text.contains("no_conference_access"),
            "expected no_conference_access in goodbye line, got {entry:?}"
        );
    }
}

mod mail {
    use std::time::{Duration, SystemTime};

    use super::super::typed::MenuSession;
    use super::super::*;
    use super::fixtures::{make_conf, session_at_onboarded_with, user_with_grants};

    #[test]
    fn read_mail_from_menu_state_advances_pointer_and_marks_received() {
        // Build a Menu-state session for alice (slot 2 in `alice()`).
        // After the read, the bound user must carry an advanced read
        // pointer, and the addressed mail must hold a `received_at`.
        // Routes through the typed `MenuSession` wrapper — the
        // compile-time guarantee that `read_mail` is only callable
        // from Menu replaces the old runtime assertion on
        // `Session::apply_read_mail`.
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        let confs = vec![make_conf(2)];
        let mut s = session_at_onboarded_with(user_with_grants(&[2]));
        s.auto_rejoin_conference(&confs, SystemTime::UNIX_EPOCH)
            .unwrap();
        s.enter_menu(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(s.state(), SessionState::Menu);
        let user_slot = s.user().unwrap().slot_number();

        let mut mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 3,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Welcome".to_string(),
            posted_at: SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: Some(user_slot),
            body: "Hi alice".to_string(),
        });
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(500);
        let mut menu = MenuSession::from_session(s);
        menu.read_mail(&mut mail, now).expect("happy path");

        assert_eq!(mail.received_at(), Some(now));
        let s = menu.into_inner();
        let pointers = s
            .user()
            .unwrap()
            .read_pointers_for(MessageBaseRef::new(2, 1))
            .expect("created lazily by ReadMail");
        assert_eq!(pointers.last_read(), 3);
    }
}
