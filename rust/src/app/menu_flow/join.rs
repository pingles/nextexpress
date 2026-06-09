//! `J <num>` (Explicit Join) menu command (Slice 32).
//!
//! Routes through [`crate::app::menu::join`]: writes the legacy "no
//! access" notice when the resolver fell through, the `Joining
//! Conference: <name>` announcement on success, any name-type
//! promotion screen (Slice 34), then fires Slice 41's
//! `ScanMailOnJoin` against the new visit.

use std::time::SystemTime;

use crate::app::menu::join::{explicit_join, ExplicitJoinOutcome};
use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    render_scan_summary, MAIL_STORE_ERROR_LINE, NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE,
    NO_CONFERENCE_ACCESS_LINE,
};
use crate::domain::conference::{find_msgbase_in, MessageBase, MessageBaseRef};
use crate::domain::messaging::scan_mail::scan_mail;
use crate::domain::session::typed::{LoggingOffSession, MenuSession};

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
        match explicit_join(
            session,
            conferences,
            target_conference_number,
            SystemTime::now(),
        ) {
            ExplicitJoinOutcome::Joined {
                mut session,
                conference_number,
                msgbase_number,
                matched_request,
                name_type_promoted_to,
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
                self.scan_mail_on_join(&mut session).await?;
                Ok(ExplicitJoinResult::Joined(session))
            }
            ExplicitJoinOutcome::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(ExplicitJoinResult::NoAccess(logging_off))
            }
        }
    }

    /// Fires `conferences.allium:ScanMailOnJoin` against the new visit
    /// (Slice 41): locks the visit's mail store, runs
    /// `messaging.allium:ScanMail` from the caller's read pointer
    /// (`from_message = 0` is the rule's "`last_scanned + 1`" sentinel —
    /// the legacy `forceMailScan = NOFORCE` path), renders the
    /// `SCREEN_MAILSCAN` asset when the scan surfaced unread mail, then
    /// the textual summary line. A missing visit or unregistered store
    /// is silent; a store error is logged to stderr and degraded to the
    /// generic mail-store-error notice — the session continues either
    /// way.
    async fn scan_mail_on_join(&mut self, session: &mut MenuSession) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            return Ok(());
        };
        let Some(guard) = self.services.mail_stores().lock(visit_msgbase).await else {
            return Ok(());
        };
        let scope = find_msgbase_in(self.services.conferences(), visit_msgbase)
            .map(MessageBase::all_scan_scope)
            .unwrap_or_default();
        let result = scan_mail(
            session.user_mut(),
            &*guard,
            visit_msgbase,
            scope,
            0,
            SystemTime::now(),
        );
        drop(guard);
        match result {
            Ok(result) => {
                if result.unread_count > 0 {
                    let screen = self.services.screens().mailscan_screen().await;
                    self.terminal.write(&screen).await?;
                }
                let summary = render_scan_summary(result.unread_count, result.first_unread_number);
                self.write_and_flush(&summary).await
            }
            Err(err) => {
                eprintln!("scan_mail_on_join failed: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::file_screen_repository::FileScreenRepository;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
    use crate::domain::conference::{
        Conference, ConferenceMembership, MessageBase, MessageBaseRef,
    };
    use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

    #[derive(Default)]
    struct CaptureTerminal {
        output: Vec<u8>,
    }

    impl Terminal for CaptureTerminal {
        type Error = Infallible;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            Box::pin(async move {
                self.output.extend_from_slice(bytes);
                Ok(())
            })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(
            &mut self,
            _echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async { Ok(TerminalRead::Eof) })
        }
    }

    fn one_conference() -> Vec<Conference> {
        vec![Conference::new(
            1,
            "One".to_string(),
            vec![MessageBase::new(1, 1, "general".to_string())],
        )
        .expect("valid conference")]
    }

    fn services_with_one_broadcast_message() -> AppServices {
        let coord = MessageBaseRef::new(1, 1);
        let mut store = InMemoryMailStore::new(coord);
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: "carol".to_string(),
                to_name: "ALL".to_string(),
                broadcast_to: BroadcastTo::All,
                subject: "hello everyone".to_string(),
                posted_at: SystemTime::UNIX_EPOCH,
                author_slot: 1,
                addressee_slot: None,
                body: String::new(),
            })
            .expect("insert broadcast");
        let mut stores = InMemoryMailStores::new();
        stores.register(coord, Box::new(store));
        AppServices::new(
            Arc::new(InMemoryUserRepository::default()),
            Arc::new(Pbkdf2PasswordHasher::new()),
            Arc::new(InMemoryCallerLog::new()),
            Arc::new(FileScreenRepository::new(std::env::temp_dir())),
            Arc::new(one_conference()),
            Arc::new(stores),
            SessionPolicy::default(),
            DefaultRatio {
                mode: RatioMode::Disabled,
                value: 0,
            },
            NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            },
            "Test BBS".to_string(),
        )
    }

    fn alice() -> User {
        let mut user = User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user");
        user.upsert_membership(ConferenceMembership::new(1, true));
        user
    }

    fn menu_session(with_visit: bool) -> MenuSession {
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("alice", alice())
            .expect("identify");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("password match");
        if with_visit {
            session
                .auto_rejoin_conference(&one_conference(), SystemTime::UNIX_EPOCH)
                .expect("rejoin");
        }
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        MenuSession::from_session(session)
    }

    #[tokio::test]
    async fn join_scan_without_an_open_visit_writes_nothing() {
        // The auto-scan-on-join is silent when the session has no open
        // visit (the deleted `ScanMailOutcome::NoOpenMsgbase` arm): no
        // summary, no error — the menu prompt follows immediately.
        let services = services_with_one_broadcast_message();
        let mut terminal = CaptureTerminal::default();
        let mut session = menu_session(false);
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.scan_mail_on_join(&mut session).await.expect("scan");
        }
        assert!(
            terminal.output.is_empty(),
            "scan without a visit must write nothing, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }

    #[tokio::test]
    async fn join_scan_follows_the_read_pointer_not_message_one() {
        // Spec `conferences.allium:ScanMailOnJoin` scans from
        // `pointers.last_scanned + 1` (`from_message = 0` sentinel; the
        // legacy `forceMailScan = NOFORCE`). A broadcast message stays
        // "unread" for as long as it is in scan range, so the second
        // join-scan only reports `No new mail.` if the first one
        // advanced the pointer past it — a mutant hardcoding the scan
        // to start from message 1 re-surfaces the broadcast forever.
        let services = services_with_one_broadcast_message();
        let mut terminal = CaptureTerminal::default();
        let mut session = menu_session(true);
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.scan_mail_on_join(&mut session).await.expect("scan");
        }
        let first = String::from_utf8_lossy(&terminal.output).into_owned();
        assert!(
            first.contains("You have 1 new message. First: 1."),
            "first scan must surface the broadcast, got {first:?}"
        );
        // The SCREEN_MAILSCAN render is gated on `unread_count > 0`
        // (here the adapter's built-in fallback banner).
        assert!(
            first.contains("New mail in this conference"),
            "an unread scan must render the mailscan screen, got {first:?}"
        );

        terminal.output.clear();
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.scan_mail_on_join(&mut session).await.expect("rescan");
        }
        let second = String::from_utf8_lossy(&terminal.output).into_owned();
        assert!(
            second.contains("No new mail."),
            "second scan must start past the advanced pointer, got {second:?}"
        );
        assert!(
            !second.contains("New mail in this conference"),
            "a zero-unread scan must not render the mailscan screen, got {second:?}"
        );
    }
}
