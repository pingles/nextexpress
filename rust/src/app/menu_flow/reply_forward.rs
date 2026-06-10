//! `RP <num>` (reply) and `FW <num>` (forward) read sub-prompt
//! commands (Slice 49a).
//!
//! The terminal-free cores ([`reply_mail`], [`forward_mail`]) load the
//! source mail from the current msgbase, resolve any addressee through
//! the user repository, and invoke the matching domain rule via the
//! typed [`MenuSession`]. The `MenuFlow` handlers below drive the
//! line-mode editor for both flows and render the outcome. Subject
//! defaults to `"Re: <source.subject>"` for replies; the forward path
//! collects an optional `--`-separated note instead of a free body.

use std::time::SystemTime;

use crate::app::input_limits::append_line_with_newline;
use crate::app::mail_stores::MailStores;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_post_success, FORWARD_NOTE_PROMPT, FORWARD_TO_PROMPT, FORWARD_UNKNOWN_USER_LINE,
    MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, POST_ABORTED_LINE, POST_ACCESS_DENIED_LINE,
    POST_ADDRESSING_NOT_ALLOWED_LINE, POST_RECIPIENT_NO_ACCESS_LINE, SOURCE_DELETED_LINE,
    SOURCE_NOT_FOUND_LINE,
};
use crate::domain::conference::Conference;
use crate::domain::messaging::forward_mail::{
    forward_mail as forward_mail_rule, ForwardMailError, ForwardMailRequest,
};
use crate::domain::messaging::limits::MAX_MAIL_BODY_BYTES;
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::post_mail::PostMailError;
use crate::domain::messaging::reply_to_mail::{
    reply_to_mail as reply_to_mail_rule, ReplyToMailDraft, ReplyToMailError,
};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// Caller-collected fields for an `RP <num>` command.
struct ReplyInput {
    /// Source mail's number in the current msgbase.
    source_number: u32,
    /// User-typed body.
    body: String,
    /// Subject line. When `None`, the use case defaults to
    /// `"Re: <source.subject>"`.
    subject: Option<String>,
    /// Whether the user asked for private visibility.
    private: bool,
    /// Honoured only when the source is an `ALL` broadcast.
    reply_keeps_broadcast: bool,
    /// Posting timestamp.
    posted_at: SystemTime,
}

/// Caller-collected fields for an `FW <num>` command.
struct ForwardInput {
    /// Source mail's number in the current msgbase.
    source_number: u32,
    /// Typed addressee handle (case-insensitive lookup).
    typed_to: String,
    /// Optional additional note appended after a `--` separator.
    additional_note: Option<String>,
    /// Posting timestamp.
    posted_at: SystemTime,
}

/// Outcome of a reply / forward attempt.
enum ReplyForwardOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The source message does not exist.
    SourceNotFound,
    /// The addressee on a forward could not be resolved.
    UnknownAddressee,
    /// The mail was persisted.
    Posted(Mail),
    /// The reply rule rejected the draft.
    ReplyRejected(ReplyToMailError),
    /// The forward rule rejected the request.
    ForwardRejected(ForwardMailError),
}

/// Runs the reply-mail use case (Slice 45 wired) without terminal I/O.
async fn reply_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    conferences: &[Conference],
    input: ReplyInput,
) -> ReplyForwardOutcome
where
    M: MailStores + ?Sized,
{
    let Some((visit_msgbase, mut guard)) = super::lock_current_base(session, mail_stores).await
    else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(allowed_addressing) = super::allowed_addressing_for(conferences, visit_msgbase) else {
        return ReplyForwardOutcome::NoMailBase;
    };

    let from_name = session.user().handle().to_string();
    let Ok(Some(source)) = guard.load(input.source_number) else {
        return ReplyForwardOutcome::SourceNotFound;
    };
    let subject = input
        .subject
        .unwrap_or_else(|| format!("Re: {}", source.subject()));
    let result = reply_to_mail_rule(
        session.user_mut(),
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        &source,
        ReplyToMailDraft {
            from_name,
            subject,
            body: input.body,
            private: input.private,
            reply_keeps_broadcast: input.reply_keeps_broadcast,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => ReplyForwardOutcome::Posted(mail),
        Err(err) => ReplyForwardOutcome::ReplyRejected(err),
    }
}

/// Runs the forward-mail use case (Slice 46 wired) without terminal I/O.
async fn forward_mail<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    conferences: &[Conference],
    input: ForwardInput,
) -> ReplyForwardOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some((visit_msgbase, mut guard)) = super::lock_current_base(session, mail_stores).await
    else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(allowed_addressing) = super::allowed_addressing_for(conferences, visit_msgbase) else {
        return ReplyForwardOutcome::NoMailBase;
    };

    let trimmed = input.typed_to.trim();
    let resolved = match user_repo.find_by_handle(trimmed) {
        NameLookupResult::Found(user) => *user,
        NameLookupResult::NotFound => return ReplyForwardOutcome::UnknownAddressee,
    };

    let from_name = session.user().handle().to_string();
    let Ok(Some(source)) = guard.load(input.source_number) else {
        return ReplyForwardOutcome::SourceNotFound;
    };
    let result = forward_mail_rule(
        session.user_mut(),
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        &source,
        ForwardMailRequest {
            new_addressee_name: resolved.handle().to_string(),
            new_addressee_slot: resolved.slot_number(),
            additional_note: input.additional_note,
            from_name,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => ReplyForwardOutcome::Posted(mail),
        Err(err) => ReplyForwardOutcome::ForwardRejected(err),
    }
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles an `RP <num>` command (Slice 45 wired). Loads the
    /// source mail, walks the body editor (the implicit addressee
    /// follows the spec — original author by default), then calls
    /// `messaging.allium:ReplyToMail` via the typed session.
    pub(super) async fn handle_reply(
        &mut self,
        session: &mut MenuSession,
        source_number: u32,
    ) -> Result<(), T::Error> {
        let Some(body) = self.read_post_body(session, true).await? else {
            return Ok(());
        };

        let outcome = reply_mail(
            session,
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            ReplyInput {
                source_number,
                body,
                subject: None, // default `Re: <source.subject>`
                private: false,
                reply_keeps_broadcast: false,
                posted_at: SystemTime::now(),
            },
        )
        .await;

        self.render_reply_forward_outcome(outcome, "RP").await
    }

    /// Handles an `FW <num>` command (Slice 46 wired). Loads the
    /// source mail, prompts for the new addressee and an optional
    /// note, then calls `messaging.allium:ForwardMail` via the
    /// typed session.
    pub(super) async fn handle_forward(
        &mut self,
        session: &mut MenuSession,
        source_number: u32,
    ) -> Result<(), T::Error> {
        let Some(typed_to) = self
            .read_required_line(session, FORWARD_TO_PROMPT, true)
            .await?
        else {
            return Ok(());
        };

        let note = self.read_forward_note(session).await?;

        let outcome = forward_mail(
            session,
            self.services.user_repo.as_ref(),
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            ForwardInput {
                source_number,
                typed_to,
                additional_note: note,
                posted_at: SystemTime::now(),
            },
        )
        .await;

        self.render_reply_forward_outcome(outcome, "FW").await
    }

    async fn render_reply_forward_outcome(
        &mut self,
        outcome: ReplyForwardOutcome,
        command_label: &str,
    ) -> Result<(), T::Error> {
        match outcome {
            ReplyForwardOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            ReplyForwardOutcome::SourceNotFound
            | ReplyForwardOutcome::ReplyRejected(ReplyToMailError::SourceNotPermitted)
            | ReplyForwardOutcome::ForwardRejected(ForwardMailError::SourceNotPermitted) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            ReplyForwardOutcome::UnknownAddressee => {
                self.write_and_flush(FORWARD_UNKNOWN_USER_LINE).await?;
            }
            ReplyForwardOutcome::Posted(mail) => {
                let line = render_post_success(mail.number());
                self.write_and_flush(&line).await?;
            }
            ReplyForwardOutcome::ReplyRejected(ReplyToMailError::SourceDeleted)
            | ReplyForwardOutcome::ForwardRejected(ForwardMailError::SourceDeleted) => {
                self.write_and_flush(SOURCE_DELETED_LINE).await?;
            }
            ReplyForwardOutcome::ReplyRejected(ReplyToMailError::Post(err))
            | ReplyForwardOutcome::ForwardRejected(ForwardMailError::Post(err)) => {
                self.render_post_error(err, command_label).await?;
            }
        }
        Ok(())
    }

    async fn render_post_error(
        &mut self,
        err: PostMailError,
        command_label: &str,
    ) -> Result<(), T::Error> {
        match err {
            PostMailError::AccessDenied => {
                self.write_and_flush(POST_ACCESS_DENIED_LINE).await?;
            }
            PostMailError::NoMembership => {
                self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
            }
            PostMailError::EmptyAddressee
            | PostMailError::AddresseeMismatch
            | PostMailError::SubjectTooLong
            | PostMailError::BodyTooLong => {
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            PostMailError::AddressingNotAllowed => {
                self.write_and_flush(POST_ADDRESSING_NOT_ALLOWED_LINE)
                    .await?;
            }
            PostMailError::Store(err) => {
                eprintln!("{command_label} command: failed to persist mail: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    /// Drives the forward note editor. Behaves like the post-body
    /// editor (`.` terminates, `/A` aborts) but a blank first line
    /// produces no-note rather than aborting.
    async fn read_forward_note(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<Option<String>, T::Error> {
        self.write_and_flush(FORWARD_NOTE_PROMPT).await?;
        let mut note = String::new();
        let mut first_line = true;
        loop {
            // The sub-prompt forward aborts silently (B6), so `/A`,
            // an oversize note, EOF and idle all return no-note without a
            // `Message aborted.` notice.
            match self.read_prompted(b"", TerminalEcho::Visible).await? {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("/A") {
                        return Ok(None);
                    }
                    if trimmed == "." {
                        return Ok(if note.is_empty() { None } else { Some(note) });
                    }
                    if first_line && trimmed.is_empty() {
                        return Ok(None);
                    }
                    first_line = false;
                    if !append_line_with_newline(&mut note, &line, MAX_MAIL_BODY_BYTES) {
                        return Ok(None);
                    }
                }
                TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                    return Ok(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::adapters::file_screen_repository::FileScreenRepository;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
    use crate::app::wire_text::POST_ABORTED_LINE;
    use crate::domain::messaging::post_mail::PostMailError;
    use crate::domain::session::SessionPolicy;
    use crate::domain::user::RatioMode;

    #[derive(Default)]
    struct CaptureTerminal {
        output: Vec<u8>,
        inputs: VecDeque<TerminalRead>,
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
            Box::pin(async move { Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof)) })
        }
    }

    fn test_services() -> AppServices {
        AppServices {
            user_repo: Arc::new(InMemoryUserRepository::default()),
            hasher: Arc::new(Pbkdf2PasswordHasher::new()),
            caller_log: Arc::new(InMemoryCallerLog::new()),
            screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
            conferences: Arc::new(Vec::new()),
            mail_stores: Arc::new(InMemoryMailStores::new()),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            session_policy: SessionPolicy::default(),
            default_ratio: DefaultRatio {
                mode: RatioMode::Disabled,
                value: 0,
            },
            new_user_gate: Arc::new(NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            }),
            bbs_name: Arc::from("Test BBS"),
        }
    }

    #[tokio::test]
    async fn render_post_error_writes_abort_for_oversized_body() {
        let services = test_services();
        let mut terminal = CaptureTerminal::default();
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };

            flow.render_post_error(PostMailError::BodyTooLong, "FW")
                .await
                .unwrap();
        }

        assert_eq!(terminal.output, POST_ABORTED_LINE);
    }
}
