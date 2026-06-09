//! Terminal-free multi-conference mail-scan use case (Tier B, Slice
//! B1 — the `MS` command).
//!
//! Walks every conference the caller has a granted membership for, in
//! ascending number order, and within each conference every message
//! base in ascending order, running `messaging.allium:ScanMail` per
//! base (spec: `messaging.allium:ScanAllMail`, legacy
//! `internalCommandMS` at `amiexpress/express.e:25250`).
//!
//! Unlike [`crate::app::menu::scan_mail`], this use case never reads or
//! mutates the session's current visit: it scans bases by coordinate,
//! so the user's current conference is left untouched — the legacy's
//! "rejoin the original conference afterwards" restore
//! (`amiexpress/express.e:25270`) holds here by construction.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{Conference, MessageBase, MessageBaseRef, ScanFlag};
use crate::domain::conference_visit::next_accessible_conference_after;
use crate::domain::messaging::scan_mail::{
    scan_mail as scan_mail_rule, MailScanRow, ScanMailError,
};
use crate::domain::session::typed::MenuSession;

/// Render-ready classification of a single base's scan during the `MS`
/// walk. The arithmetic that distinguishes the legacy's three
/// `searchNewMail` outputs (table / `No mail today!` / banner-only)
/// lives here in the unit-tested use case so the presentation layer is
/// a guard-free match.
#[derive(Debug)]
pub(crate) enum BaseScanOutcome {
    /// Unread mail matched — the rows to render as the listing table.
    Listing(Vec<MailScanRow>),
    /// Nothing new since the user's last scan: the start point was
    /// already past `highest_message` (`amiexpress/express.e:11687`'s
    /// `msgNum >= highMsgNum`). Renders `No mail today!`.
    NothingNew,
    /// Messages exist in range but none are addressed to the caller —
    /// the legacy prints only the banner (`mailFlag` stays 0).
    NoMatch,
    /// No store is registered for the base — degrade silently, as the
    /// legacy `joinConf` does when a base has no on-disk message file.
    NoStore,
    /// The underlying store failed; surfaced so the caller can log it
    /// and render the generic mail-store error notice.
    Error(ScanMailError),
}

/// One scanned message base, with the banner data the presentation
/// layer needs (its name and whether it is the conference's first base,
/// which governs the leading CRLF in the legacy banner).
pub(crate) struct BaseScan {
    /// The base's display name (empty when unnamed — no sub-line).
    pub msgbase_name: String,
    /// `true` for the lowest-numbered base in its conference
    /// (legacy `msgBaseNum=1`).
    pub first_base: bool,
    /// What the per-base scan produced.
    pub outcome: BaseScanOutcome,
}

/// Which accessible conferences a [`scan_all_mail`] walk visits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanFilter {
    /// Every conference the caller has a granted membership for — the
    /// `MS` command, which forces a scan of all bases regardless of the
    /// per-conference flags (legacy `internalCommandMS`).
    AllConferences,
    /// Only conferences whose membership has `mail_scan` set — the
    /// logon conference scan (legacy `confScan`'s `checkMailConfScan`
    /// gate, `amiexpress/express.e:28095`).
    MailScanFlagged,
}

/// One scanned conference and its bases, in ascending base order.
pub(crate) struct ConferenceScan {
    /// The conference's display name, for the `Scanning Conference`
    /// banner.
    pub conference_name: String,
    /// The conference's message bases, scanned in ascending order.
    pub bases: Vec<BaseScan>,
}

/// Runs `messaging.allium:ScanAllMail`: scans every conference the
/// session's user has a granted membership for (ascending), and every
/// base within each conference (ascending), returning the per-base
/// results for the caller to render. Each base is scanned with
/// `from_message = 0` (the spec's "new mail since `last_scanned`"
/// sentinel — the legacy `FORCE_MAILSCAN_ALL` forces *whether* a base
/// is scanned, not where the scan starts).
///
/// `filter` selects which accessible conferences are walked: the `MS`
/// command passes [`ScanFilter::AllConferences`]; the logon conference
/// scan passes [`ScanFilter::MailScanFlagged`] to honour the
/// per-conference `mail_scan` flag.
///
/// Does not render anything and does not touch the session's current
/// visit.
pub(crate) async fn scan_all_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    conferences: &[Conference],
    filter: ScanFilter,
    now: SystemTime,
) -> Vec<ConferenceScan>
where
    M: MailStores + ?Sized,
{
    // Enumerate the accessible conference numbers first (owned `u32`s),
    // so the immutable user borrow is released before the scan loop
    // takes `user_mut`. `next_accessible_conference_after(.., 0)` yields
    // the lowest-numbered accessible conference, and each step's
    // strictly-greater result guarantees termination. The logon walk
    // additionally drops conferences whose membership has `mail_scan`
    // cleared (legacy `checkMailConfScan`).
    let accessible: Vec<u32> = {
        let user = session.user();
        let mut numbers = Vec::new();
        let mut after = 0;
        while let Some(conference) = next_accessible_conference_after(user, conferences, after) {
            after = conference.number();
            let include = match filter {
                ScanFilter::AllConferences => true,
                ScanFilter::MailScanFlagged => user
                    .memberships()
                    .iter()
                    .find(|membership| membership.conference_number() == after)
                    .is_some_and(|membership| membership.scan_flag(ScanFlag::MailScan)),
            };
            if include {
                numbers.push(after);
            }
        }
        numbers
    };

    let mut scans = Vec::with_capacity(accessible.len());
    for conference_number in accessible {
        let Some(conference) = conferences.iter().find(|c| c.number() == conference_number) else {
            continue;
        };
        let mut ordered: Vec<&MessageBase> = conference.msgbases().iter().collect();
        ordered.sort_by_key(|base| base.number());

        let mut bases = Vec::with_capacity(ordered.len());
        for (index, base) in ordered.iter().enumerate() {
            let coord = MessageBaseRef::new(conference_number, base.number());
            let outcome = match mail_stores.lock(coord).await {
                None => BaseScanOutcome::NoStore,
                Some(guard) => {
                    let scope = base.all_scan_scope();
                    let result = scan_mail_rule(session.user_mut(), &*guard, coord, scope, 0, now);
                    drop(guard);
                    match result {
                        Err(err) => BaseScanOutcome::Error(err),
                        Ok(result) if !result.listing.is_empty() => {
                            BaseScanOutcome::Listing(result.listing)
                        }
                        // Nothing new: the scan started past the highest
                        // message (`from > highest_message`).
                        Ok(result) if result.from > result.highest_message => {
                            BaseScanOutcome::NothingNew
                        }
                        // Messages in range, but none addressed to the caller.
                        Ok(_) => BaseScanOutcome::NoMatch,
                    }
                }
            };
            bases.push(BaseScan {
                msgbase_name: base.name().to_string(),
                first_base: index == 0,
                outcome,
            });
        }
        scans.push(ConferenceScan {
            conference_name: conference.name().to_string(),
            bases,
        });
    }
    scans
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::domain::conference::{
        Conference, ConferenceMembership, MessageBase, MessageBaseRef,
    };
    use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
    use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
    use crate::domain::messaging::mail_store::MailStore;
    use crate::domain::password::PasswordHashKind;
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::User;

    use super::{scan_all_mail, BaseScanOutcome, ScanFilter};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn menu_session_with_memberships(conferences: &[u32]) -> MenuSession {
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
        for &number in conferences {
            user.upsert_membership(ConferenceMembership::new(number, true));
        }
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("alice", user)
            .expect("identify");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("password match");
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        MenuSession::from_session(session)
    }

    fn two_conferences() -> Vec<Conference> {
        vec![
            Conference::new(
                1,
                "One".to_string(),
                vec![MessageBase::new(1, 1, "general".to_string())],
            )
            .expect("valid conference"),
            Conference::new(
                2,
                "Two".to_string(),
                vec![MessageBase::new(2, 1, "general".to_string())],
            )
            .expect("valid conference"),
        ]
    }

    fn store_with_mail(
        coord: MessageBaseRef,
        addressee: u32,
        count: u32,
    ) -> Box<dyn MailStore + Send> {
        let mut store = InMemoryMailStore::new(coord);
        for _ in 0..count {
            store
                .insert(MailDraft {
                    visibility: MailVisibility::Public,
                    from_name: "carol".to_string(),
                    to_name: format!("user{addressee}"),
                    broadcast_to: BroadcastTo::None,
                    subject: "subject".to_string(),
                    posted_at: t(0),
                    author_slot: 1,
                    addressee_slot: Some(addressee),
                    body: String::new(),
                })
                .expect("insert");
        }
        Box::new(store)
    }

    #[tokio::test]
    async fn scans_every_accessible_conference_in_ascending_order() {
        let mut session = menu_session_with_memberships(&[1, 2]);
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 2, 1),
        );
        stores.register(
            MessageBaseRef::new(2, 1),
            store_with_mail(MessageBaseRef::new(2, 1), 2, 2),
        );

        let scans = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert_eq!(scans.len(), 2);
        assert_eq!(scans[0].conference_name, "One");
        assert_eq!(scans[1].conference_name, "Two");

        let base0 = &scans[0].bases[0];
        assert_eq!(base0.msgbase_name, "general");
        assert!(base0.first_base);
        match &base0.outcome {
            BaseScanOutcome::Listing(rows) => assert_eq!(rows.len(), 1),
            other => panic!("expected Listing, got {other:?}"),
        }
        match &scans[1].bases[0].outcome {
            BaseScanOutcome::Listing(rows) => assert_eq!(rows.len(), 2),
            other => panic!("expected Listing, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn skips_conferences_without_a_granted_membership() {
        // Member of conference 1 only; conference 2 must not appear even
        // though a store is registered for it.
        let mut session = menu_session_with_memberships(&[1]);
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 2, 1),
        );
        stores.register(
            MessageBaseRef::new(2, 1),
            store_with_mail(MessageBaseRef::new(2, 1), 2, 2),
        );

        let scans = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].conference_name, "One");
    }

    #[tokio::test]
    async fn reports_no_store_when_a_base_is_unregistered() {
        // A missing store degrades gracefully (the legacy `joinConf`
        // simply finds no base) rather than erroring the command.
        let mut session = menu_session_with_memberships(&[1]);
        let confs = two_conferences();
        let stores = InMemoryMailStores::new();

        let scans = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert_eq!(scans.len(), 1);
        assert!(matches!(
            scans[0].bases[0].outcome,
            BaseScanOutcome::NoStore
        ));
    }

    #[tokio::test]
    async fn no_match_when_messages_exist_but_none_addressed_to_the_user() {
        // A single message addressed to someone else: the base has been
        // fully scanned (`from == highest_message`, so it is *not*
        // "nothing new") but produced no rows — the legacy prints the
        // banner only. This boundary distinguishes `from > highest`
        // (NoMatch) from `from >= highest` (which would wrongly report
        // NothingNew).
        let mut session = menu_session_with_memberships(&[1]);
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 9, 1),
        );

        let scans = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert!(
            matches!(scans[0].bases[0].outcome, BaseScanOutcome::NoMatch),
            "expected NoMatch, got {:?}",
            scans[0].bases[0].outcome
        );
    }

    #[tokio::test]
    async fn nothing_new_when_the_base_is_empty() {
        // An empty base: `from = 1 > highest = 0`, so the scan starts
        // past the end — the legacy's `No mail today!` case.
        let mut session = menu_session_with_memberships(&[1]);
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 2, 0),
        );

        let scans = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert!(
            matches!(scans[0].bases[0].outcome, BaseScanOutcome::NothingNew),
            "expected NothingNew, got {:?}",
            scans[0].bases[0].outcome
        );
    }

    #[tokio::test]
    async fn does_not_open_or_change_a_conference_visit() {
        // MS scans by coordinate and must never open or move the
        // session's visit — the legacy restores the original conference
        // afterwards; here there is nothing to restore.
        let mut session = menu_session_with_memberships(&[1, 2]);
        assert_eq!(session.current_conference_number(), None);
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 2, 1),
        );
        stores.register(
            MessageBaseRef::new(2, 1),
            store_with_mail(MessageBaseRef::new(2, 1), 2, 1),
        );

        scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;

        assert_eq!(
            session.current_conference_number(),
            None,
            "MS must not open a conference visit"
        );
    }

    #[tokio::test]
    async fn mail_scan_flagged_filter_skips_conferences_with_mail_scan_off() {
        use crate::domain::conference::ScanFlag;
        // Member of both conferences, each with mail seeded, but
        // conference 2's `mail_scan` flag is cleared (the `CF` editor's
        // effect / the legacy `checkMailConfScan` gate). The logon walk
        // (`MailScanFlagged`) must scan only conference 1; the
        // unconditional `MS` walk (`AllConferences`) still scans both.
        let mut session = menu_session_with_memberships(&[1, 2]);
        for membership in session.user_mut().memberships_mut() {
            if membership.conference_number() == 2 {
                membership.set_scan_flag(ScanFlag::MailScan, false);
            }
        }
        let confs = two_conferences();
        let mut stores = InMemoryMailStores::new();
        stores.register(
            MessageBaseRef::new(1, 1),
            store_with_mail(MessageBaseRef::new(1, 1), 2, 1),
        );
        stores.register(
            MessageBaseRef::new(2, 1),
            store_with_mail(MessageBaseRef::new(2, 1), 2, 1),
        );

        let flagged = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::MailScanFlagged,
            t(100),
        )
        .await;
        assert_eq!(
            flagged.len(),
            1,
            "a conference with mail_scan cleared must be skipped by the logon walk"
        );
        assert_eq!(flagged[0].conference_name, "One");

        // The `MS` walk ignores the flag and scans every accessible
        // conference, including the one the logon walk skipped.
        let all = scan_all_mail(
            &mut session,
            &stores,
            &confs,
            ScanFilter::AllConferences,
            t(100),
        )
        .await;
        assert_eq!(
            all.len(),
            2,
            "AllConferences must ignore the mail_scan flag"
        );
    }
}
