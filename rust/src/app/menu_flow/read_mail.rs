//! `R <num>` (Read Mail) menu command (Slice 39).
//!
//! The terminal-free core ([`read_mail`]) owns store resolution and
//! the `messaging.allium:ReadMail` rule; the `MenuFlow` handlers below
//! own the prompts and wire rendering.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::app::menu_flow::mail_text::{MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE};
use crate::app::terminal::Terminal;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::mail_store::MailStoreError;
use crate::domain::messaging::read_mail::{read_mail as read_mail_rule, ReadMailError};
use crate::domain::messaging::read_pointers::ReadPointers;
use crate::domain::session::typed::MenuSession;

/// Sent when the requested message number is unknown in this
/// message base. Mirrors the legacy `Msg #X not found.` notice
/// (`amiexpress/express.e:25460`).
const MESSAGE_NOT_FOUND_LINE: &[u8] = b"\r\nMessage not found.\r\n";

/// Sent when the user tries to read a soft-deleted message. Mirrors
/// the legacy `That message has been deleted.` line
/// (`amiexpress/express.e:8890`).
const DELETED_MESSAGE_LINE: &[u8] = b"\r\nThat message has been deleted.\r\n\r\n";

/// Sent when the user has no membership grant for the current
/// conference, or the message's visibility blocks them from reading
/// it.
const READ_DENIED_LINE: &[u8] = b"\r\nYou are not permitted to read this message.\r\n";

/// Renders a [`Mail`]'s header block for the menu's `R` command.
///
/// Mirrors the legacy `displayMessage` output at
/// `amiexpress/express.e:8900-8938`:
///
/// ```text
///   Date   : <date>                Number: <n>
///   To     : <to>                  Recv'd: <date | No | N/A>
///   From   : <from>                Status: Public Message | Private Message
///   Subject: <subject>
/// ```
///
/// ANSI colour escapes match the legacy output: `[32m` green for
/// the labels' left half, `[33m` yellow for the separating colon,
/// `[0m` reset for the value, all on a single colour budget that
/// the existing telnet adapter passes through verbatim.
///
/// Timestamps are rendered as RFC 3339 UTC for now; the legacy's
/// human-friendly `formatLongDateTime` format lands with the
/// locale-aware formatter slice in Phase 13.
///
/// [`Mail`]: crate::domain::messaging::mail::Mail
fn render_mail_header(
    mail: &crate::domain::messaging::mail::Mail,
    conference_name: &str,
) -> Vec<u8> {
    use crate::domain::messaging::mail::{BroadcastTo, MailVisibility};
    use time::OffsetDateTime;
    let mut out = Vec::with_capacity(256);
    let posted = OffsetDateTime::from(mail.posted_at());
    let posted_str = posted
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    out.extend_from_slice(b"\r\n\x1b[32mDate   \x1b[33m:\x1b[0m ");
    out.extend_from_slice(posted_str.as_bytes());
    out.extend_from_slice(b"  \x1b[32mNumber\x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.number().to_string().as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mTo     \x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.to_name().as_bytes());
    out.extend_from_slice(b"  \x1b[32mRecv'd\x1b[33m:\x1b[0m ");
    match (mail.received_at(), mail.broadcast_to()) {
        (Some(t), _) => {
            let recvd = OffsetDateTime::from(t)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "unknown".to_string());
            out.extend_from_slice(recvd.as_bytes());
        }
        (None, BroadcastTo::All | BroadcastTo::Eall) => out.extend_from_slice(b"N/A"),
        (None, BroadcastTo::None) => out.extend_from_slice(b"No"),
    }
    out.extend_from_slice(b"\r\n\x1b[32mFrom   \x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.from_name().as_bytes());
    out.extend_from_slice(b"  \x1b[32mStatus\x1b[33m:\x1b[0m ");
    let status = match mail.visibility() {
        MailVisibility::Public => "Public Message",
        MailVisibility::Private => "Private Message",
        // PrivateToSysop and Deleted are filtered upstream — the `R`
        // dispatch rejects deleted mail and the read-permission gate
        // blocks PrivateToSysop for non-author/non-sysop readers. We
        // render them defensively for the sysop-reads-anything case
        // (PrivateToSysop) and the never-reachable Deleted branch.
        MailVisibility::PrivateToSysop => "Private to Sysop",
        MailVisibility::Deleted => "Deleted",
    };
    out.extend_from_slice(status.as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mSubject\x1b[33m:\x1b[0m ");
    out.extend_from_slice(mail.subject().as_bytes());
    out.extend_from_slice(b"\r\n\x1b[32mConf   \x1b[33m:\x1b[0m [");
    out.extend_from_slice(mail.msgbase().conference_number().to_string().as_bytes());
    out.extend_from_slice(b"] ");
    out.extend_from_slice(conference_name.as_bytes());
    out.extend_from_slice(b"\r\n\r\n");
    out
}

/// Translates a mail body's LF line endings to telnet CRLF, ensuring
/// the trailing line ends with `\r\n`. The on-disk body uses Unix
/// line endings; rendering for telnet has to normalise them or the
/// receiving terminal stair-steps. Mirrors the legacy `displayFile`'s
/// per-line `aePuts` behaviour.
fn render_mail_body(body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 16);
    let mut last_was_cr = false;
    for ch in body.chars() {
        if ch == '\n' && !last_was_cr {
            out.extend_from_slice(b"\r\n");
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
        last_was_cr = ch == '\r';
    }
    if !body.ends_with('\n') {
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Outcome of the terminal-free read-mail command.
enum ReadMailOutcome {
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
enum ReadMailStoreOperation {
    /// The use case failed while loading the requested message.
    Load,
    /// The use case failed while saving read-state changes.
    Save,
}

/// Details for a failed mail-store operation.
#[derive(Debug)]
struct ReadMailStoreFailure {
    /// Failed operation.
    operation: ReadMailStoreOperation,
    /// Message number involved in the operation.
    number: u32,
    /// Adapter-originated error.
    source: MailStoreError,
}

/// Runs the read-mail use case without touching terminal I/O.
async fn read_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    conferences: &[Conference],
    number: u32,
    now: SystemTime,
) -> ReadMailOutcome
where
    M: MailStores + ?Sized,
{
    let Some((visit_msgbase, mut guard)) = super::lock_current_base(session, mail_stores).await
    else {
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

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_read_mail(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        // `R <num>` is read-first (legacy `passItIN` -> `goNextMsg`,
        // `express.e:12003-12004`): the message is displayed, then the
        // sub-prompt loop opens with the pointer advanced past it. The
        // not-found / deleted / denied / error notices return straight to
        // the menu — there is no current message to operate on.
        if self.read_and_render(session, number).await? {
            self.run_read_subprompt(session, number + 1, Some(number))
                .await?;
        }
        Ok(())
    }

    /// Bare `R` (no message number): opens the read sub-prompt
    /// PROMPT-FIRST at the caller's resume point — the legacy `readMSG`
    /// no-arg entry (`express.e:11984-12021`). The resume point is the
    /// per-base read pointer plus one (`lastMsgReadConf + 1`, `:11984`,
    /// where `lastMsgReadConf := cb.confYM`, `:4912`), clamped up to the
    /// base's lowest key (`:11985`). This is the sequential read pointer,
    /// not the first unread message addressed to the reader.
    ///
    /// Unlike `R <num>`, bare `R` shows no message before the prompt: the
    /// `Msg. Options:` prompt renders at the resume range and the first
    /// `<CR>` then displays the resume message. When the resume point is
    /// past the highest existing message (the pointer is exhausted, or the
    /// base is empty) the prompt renders with the `( QUIT )` range and a
    /// `<CR>` / `Q` returns to the menu (legacy `:12012`).
    pub(super) async fn handle_read_mail_at_pointer(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let Some((conference, msgbase)) = session.current_msgbase() else {
            return self.write_and_flush(NO_MAIL_BASE_LINE).await;
        };
        let base = MessageBaseRef::new(conference, msgbase);

        // A never-read base has no pointer row; treat that as 0 so the
        // resume starts at message 1 (legacy `lastMsgReadConf` default).
        let last_read = session
            .user()
            .read_pointers_for(base)
            .map_or(0, ReadPointers::last_read);

        // Clamp UP to the base's lowest key. The trait exposes the lowest
        // *undeleted* message; this matches the legacy `mailStat.lowestKey`
        // except when the true lowest key is a soft-deleted message below
        // it.
        let lowest = match self.services.mail_stores.as_ref().lock(base).await {
            Some(guard) => match guard.lowest_undeleted_message() {
                Ok(lowest) => lowest,
                Err(error) => {
                    eprintln!("R command: failed to determine lowest readable mail: {error}");
                    return self.write_and_flush(MAIL_STORE_ERROR_LINE).await;
                }
            },
            None => return self.write_and_flush(NO_MAIL_BASE_LINE).await,
        };
        let start = last_read.saturating_add(1).max(lowest);

        // The legacy entry blank line (`express.e:11987`) precedes the
        // prompt-first loop; no message is displayed yet, so
        // `last_displayed` is `None`.
        self.write_newline().await?;
        self.run_read_subprompt(session, start, None).await
    }

    /// Reads message `number` through the terminal-free use case and
    /// renders the outcome. Returns `true` when a message was actually
    /// displayed (the sub-prompt's precondition), `false` for every
    /// notice path. Shared by the initial `R <num>` read and the
    /// sub-prompt's `<CR>`-advance.
    pub(super) async fn read_and_render(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<bool, T::Error> {
        match read_mail(
            session,
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            number,
            SystemTime::now(),
        )
        .await
        {
            ReadMailOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::MessageNotFound => {
                self.write_and_flush(MESSAGE_NOT_FOUND_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::StoreError(failure) => {
                log_read_store_failure(&failure);
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::Deleted => {
                self.write_and_flush(DELETED_MESSAGE_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::Denied => {
                self.write_and_flush(READ_DENIED_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::Read {
                mail,
                conference_name,
            } => {
                let header = render_mail_header(&mail, &conference_name);
                let body = render_mail_body(mail.body());
                self.terminal.write(&header).await?;
                self.terminal.write(&body).await?;
                self.terminal.flush().await?;
                Ok(true)
            }
        }
    }
}

fn log_read_store_failure(failure: &ReadMailStoreFailure) {
    match failure.operation {
        ReadMailStoreOperation::Load => {
            eprintln!(
                "R command: failed to load mail #{}: {}",
                failure.number, failure.source
            );
        }
        ReadMailStoreOperation::Save => {
            eprintln!(
                "R command: failed to save mail #{}: {}",
                failure.number, failure.source
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::app::mail_stores::MailStores;
    use crate::domain::conference::{Conference, ConferenceMembership, MessageBase};
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::User;

    use super::{read_mail, render_mail_body, render_mail_header, ReadMailOutcome};

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

    #[tokio::test]
    async fn read_mail_resolves_the_conference_name_of_the_open_visit() {
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, MailDraft, MailVisibility};
        use crate::domain::messaging::mail_store::test_support::InMemoryMailStore;
        use crate::domain::messaging::mail_store::MailStore;

        // The rendered header names the conference of the *open visit*
        // (here number 2, "Other"), not whichever conference happens to
        // sort first in the loaded list.
        let conferences = vec![
            Conference::new(
                1,
                "Main".to_string(),
                vec![MessageBase::new(1, 1, "main".to_string())],
            )
            .expect("valid conference"),
            Conference::new(
                2,
                "Other".to_string(),
                vec![MessageBase::new(2, 1, "general".to_string())],
            )
            .expect("valid conference"),
        ];
        let mut user = alice();
        user.upsert_membership(ConferenceMembership::new(2, true));
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
        session
            .auto_rejoin_conference(&conferences, SystemTime::UNIX_EPOCH)
            .expect("rejoin");
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        let mut session = MenuSession::from_session(session);

        let coord = MessageBaseRef::new(2, 1);
        let mut store = InMemoryMailStore::new(coord);
        store
            .insert(MailDraft {
                visibility: MailVisibility::Public,
                from_name: "carol".to_string(),
                to_name: "alice".to_string(),
                broadcast_to: BroadcastTo::None,
                subject: "hello".to_string(),
                posted_at: SystemTime::UNIX_EPOCH,
                author_slot: 1,
                addressee_slot: Some(2),
                body: "hi".to_string(),
            })
            .expect("insert");
        let mut mail_stores = InMemoryMailStores::new();
        mail_stores.register(coord, Box::new(store));

        let outcome = read_mail(
            &mut session,
            &mail_stores as &dyn MailStores,
            &conferences,
            1,
            SystemTime::UNIX_EPOCH,
        )
        .await;

        match outcome {
            ReadMailOutcome::Read {
                conference_name, ..
            } => assert_eq!(conference_name, "Other"),
            _ => panic!("expected ReadMailOutcome::Read"),
        }
    }

    #[test]
    fn render_mail_body_translates_lone_lf_into_crlf() {
        // Bodies arrive from disk with Unix `\n` line endings;
        // emitting them as-is would stair-step the receiving
        // terminal. Telnet requires `\r\n`.
        let body = "First line.\nSecond line.\n";
        assert_eq!(render_mail_body(body), b"First line.\r\nSecond line.\r\n");
    }

    #[test]
    fn render_mail_body_preserves_existing_crlf_pairs() {
        // A body that already carries `\r\n` (e.g. authored on
        // Windows or by a migration tool) must not turn each pair
        // into `\r\r\n`.
        let body = "Line 1.\r\nLine 2.\r\n";
        assert_eq!(render_mail_body(body), b"Line 1.\r\nLine 2.\r\n");
    }

    #[test]
    fn render_mail_body_appends_terminator_when_body_has_no_trailing_newline() {
        // A body without a trailing LF must still end with `\r\n`
        // so the menu prompt that follows starts on a fresh line.
        let body = "No trailing newline";
        assert_eq!(render_mail_body(body), b"No trailing newline\r\n");
    }

    #[test]
    fn render_mail_body_handles_empty_input() {
        let body = "";
        // Empty body still emits the terminator so the menu prompt
        // is not jammed against the header block.
        assert_eq!(render_mail_body(body), b"\r\n");
    }

    #[test]
    fn render_mail_header_emits_legacy_label_block() {
        // Pin the legacy label block. The escape sequences and the
        // ordering of labels must match `amiexpress/express.e:8900-8938`.
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        let mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 7,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Welcome".to_string(),
            posted_at: std::time::SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: Some(2),
            body: "Hello".to_string(),
        });
        let rendered = render_mail_header(&mail, "Programming");
        let text = std::str::from_utf8(&rendered).expect("utf8");
        // Each label and its value appears once.
        assert!(text.contains("Date   "), "missing Date label: {text:?}");
        assert!(text.contains("Number"), "missing Number label: {text:?}");
        assert!(text.contains("Number\x1b[33m:\x1b[0m 7"), "wrong number");
        assert!(
            text.contains("To     \x1b[33m:\x1b[0m alice"),
            "wrong To: {text:?}",
        );
        assert!(
            text.contains("From   \x1b[33m:\x1b[0m Sysop"),
            "wrong From: {text:?}",
        );
        assert!(
            text.contains("Status\x1b[33m:\x1b[0m Public Message"),
            "wrong Status: {text:?}",
        );
        assert!(
            text.contains("Subject\x1b[33m:\x1b[0m Welcome"),
            "wrong Subject: {text:?}",
        );
        assert!(
            text.contains("Conf   \x1b[33m:\x1b[0m [2] Programming"),
            "wrong Conf: {text:?}",
        );
        // An unread mail addressed to a named user renders Recv'd: No.
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m No"),
            "expected Recv'd: No for unread, got: {text:?}",
        );
    }

    #[test]
    fn render_mail_header_marks_broadcast_recipients_as_not_applicable() {
        // `amiexpress/express.e:8923` — broadcast mail has
        // `Recv'd: N/A` because no single addressee owns it.
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        let mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "ALL".to_string(),
            broadcast_to: BroadcastTo::All,
            subject: "Notice".to_string(),
            posted_at: std::time::SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: None,
            body: "Notice body".to_string(),
        });
        let rendered = render_mail_header(&mail, "Conf");
        let text = std::str::from_utf8(&rendered).expect("utf8");
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m N/A"),
            "broadcast mail must render N/A, got: {text:?}",
        );
    }

    #[test]
    fn render_mail_header_renders_received_at_when_set() {
        // A read mail (with `received_at = Some`) shows the timestamp
        // rather than the literal "No".
        use crate::domain::conference::MessageBaseRef;
        use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility, NewMail};
        use std::time::{Duration, SystemTime};
        let mut mail = Mail::new(NewMail {
            msgbase: MessageBaseRef::new(2, 1),
            number: 1,
            visibility: MailVisibility::Public,
            from_name: "Sysop".to_string(),
            to_name: "alice".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: "Hi".to_string(),
            posted_at: SystemTime::UNIX_EPOCH,
            author_slot: 1,
            addressee_slot: Some(2),
            body: String::new(),
        });
        mail.mark_received(SystemTime::UNIX_EPOCH + Duration::from_secs(100))
            .unwrap();
        let text = String::from_utf8(render_mail_header(&mail, "Conf")).unwrap();
        assert!(
            text.contains("Recv'd\x1b[33m:\x1b[0m 1970-01-01T00:01:40Z"),
            "expected RFC 3339 received_at, got: {text:?}",
        );
    }
}
