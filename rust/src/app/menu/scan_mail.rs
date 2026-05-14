//! Terminal-free scan-mail use case shared by menu scans and scan-on-join.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{find_msgbase_in, Conference, MessageBaseRef};
use crate::domain::messaging::scan_mail::{ScanMailError, ScanResult};
use crate::domain::session::typed::ScanOnJoin;

/// Outcome of scanning the current message base.
pub(crate) enum ScanMailOutcome {
    /// The session has no current message base.
    NoOpenMsgbase,
    /// No store is registered for the current message base.
    NoStore,
    /// The scan rule or underlying store failed.
    StoreError(ScanMailError),
    /// Scan completed successfully.
    Scanned(ScanResult),
}

/// Runs `messaging.allium:ScanMail` for the session's current message
/// base without rendering terminal output.
pub(crate) async fn scan_mail<S, M>(
    session: &mut S,
    mail_stores: &M,
    conferences: &[Conference],
    from_message: u32,
    now: SystemTime,
) -> ScanMailOutcome
where
    S: ScanOnJoin + ?Sized,
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = session
        .current_msgbase()
        .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
    else {
        return ScanMailOutcome::NoOpenMsgbase;
    };

    let Some(store) = mail_stores.for_msgbase(visit_msgbase) else {
        return ScanMailOutcome::NoStore;
    };

    let scope = find_msgbase_in(conferences, visit_msgbase)
        .map(crate::domain::conference::MessageBase::all_scan_scope)
        .unwrap_or_default();

    let guard = store.lock().await;
    let result = session.scan_mail(&**guard, visit_msgbase, scope, from_message, now);
    drop(guard);

    match result {
        Ok(result) => ScanMailOutcome::Scanned(result),
        Err(err) => ScanMailOutcome::StoreError(err),
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::app::mail_stores::MailStores;
    use crate::domain::conference::{Conference, MessageBase};
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::User;

    use super::{scan_mail, ScanMailOutcome};

    fn alice() -> User {
        User::new(
            2,
            "alice".to_string(),
            crate::domain::password::PasswordHashKind::Pbkdf210000,
            "hash".to_string(),
            Some("salt".to_string()),
            SystemTime::UNIX_EPOCH,
            100,
        )
        .expect("valid user")
    }

    fn menu_session_without_visit() -> MenuSession {
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
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        MenuSession::from_session(session)
    }

    #[tokio::test]
    async fn scan_mail_without_an_open_msgbase_returns_no_open_msgbase() {
        let mut session = menu_session_without_visit();
        let mail_stores = InMemoryMailStores::new();
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference")];

        let outcome = scan_mail(
            &mut session,
            &mail_stores as &dyn MailStores,
            &conferences,
            1,
            SystemTime::UNIX_EPOCH,
        )
        .await;

        assert!(matches!(outcome, ScanMailOutcome::NoOpenMsgbase));
    }
}
