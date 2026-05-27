//! Terminal-free `R <num>` menu command use case.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::mail_store::MailStoreError;
use crate::domain::messaging::read_mail::{read_mail as read_mail_rule, ReadMailError};
use crate::domain::session::typed::{BoundMenuUser, MenuSession};

/// Outcome of the terminal-free read-mail command.
pub(crate) enum ReadMailOutcome {
    /// The session has no current message base, or no store is
    /// registered for it.
    NoMailBase,
    /// The requested message number does not exist.
    MessageNotFound,
    /// Loading or saving the message failed.
    StoreError(ReadMailStoreFailure),
    /// The message is soft-deleted.
    Deleted,
    /// The bound user cannot read this message.
    Denied,
    /// The message was read and persisted.
    Read {
        /// Mutated mail returned after the read rule stamps
        /// `received_at` where appropriate.
        mail: Mail,
        /// Conference name used by the renderer.
        conference_name: String,
    },
}

/// Store operation that failed while reading mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadMailStoreOperation {
    /// The use case failed while loading the requested message.
    Load,
    /// The use case failed while saving read-state changes.
    Save,
}

/// Details for a failed mail-store operation.
#[derive(Debug)]
pub(crate) struct ReadMailStoreFailure {
    /// Failed operation.
    pub(crate) operation: ReadMailStoreOperation,
    /// Message number involved in the operation.
    pub(crate) number: u32,
    /// Adapter-originated error.
    pub(crate) source: MailStoreError,
}

/// Runs the read-mail use case without touching terminal I/O.
pub(crate) async fn read_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    conferences: &[Conference],
    number: u32,
    now: SystemTime,
) -> ReadMailOutcome
where
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = session
        .current_msgbase()
        .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
    else {
        return ReadMailOutcome::NoMailBase;
    };

    let Some(mut guard) = mail_stores.lock(visit_msgbase).await else {
        return ReadMailOutcome::NoMailBase;
    };

    let conference_name = conferences
        .iter()
        .find(|c| c.number() == visit_msgbase.conference_number())
        .map(|c| c.name().to_string())
        .unwrap_or_default();

    let mut mail = match guard.load(number) {
        Ok(Some(mail)) => mail,
        Ok(None) => return ReadMailOutcome::MessageNotFound,
        Err(source) => {
            return ReadMailOutcome::StoreError(ReadMailStoreFailure {
                operation: ReadMailStoreOperation::Load,
                number,
                source,
            });
        }
    };

    match read_mail_rule(session.user_mut(), &mut mail, now) {
        Ok(()) => {}
        Err(ReadMailError::Deleted) => return ReadMailOutcome::Deleted,
        Err(
            ReadMailError::AccessDenied | ReadMailError::NotPermitted | ReadMailError::NoMembership,
        ) => return ReadMailOutcome::Denied,
    }

    if let Err(source) = guard.save(&mail) {
        return ReadMailOutcome::StoreError(ReadMailStoreFailure {
            operation: ReadMailStoreOperation::Save,
            number,
            source,
        });
    }
    drop(guard);

    ReadMailOutcome::Read {
        mail,
        conference_name,
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

    use super::{read_mail, ReadMailOutcome};

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
    async fn read_mail_without_an_open_msgbase_returns_no_mail_base() {
        let mut session = menu_session_without_visit();
        let mail_stores = InMemoryMailStores::new();
        let conferences = vec![Conference::new(
            1,
            "Main".to_string(),
            vec![MessageBase::new(1, 1, "main".to_string())],
        )
        .expect("valid conference")];

        let outcome = read_mail(
            &mut session,
            &mail_stores as &dyn MailStores,
            &conferences,
            7,
            SystemTime::UNIX_EPOCH,
        )
        .await;

        assert!(matches!(outcome, ReadMailOutcome::NoMailBase));
    }
}
