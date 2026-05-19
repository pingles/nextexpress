//! `RP <num>` (reply) and `FW <num>` (forward) menu commands
//! (Slice 49a).
//!
//! Drives the line-mode editor for both flows, delegates the
//! terminal-free effect to [`crate::app::menu::reply_forward`] and
//! renders the outcome. Subject defaults to `"Re: <source.subject>"`
//! for replies; the forward path collects an optional `--`-separated
//! note instead of a free body.

use std::time::SystemTime;

use crate::app::menu::reply_forward::{
    forward_mail, reply_mail, ForwardInput, ReplyForwardOutcome, ReplyInput,
};
use crate::app::menu_command::NumberArg;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_post_success, FORWARD_NOTE_PROMPT, FORWARD_TO_PROMPT, FORWARD_UNKNOWN_USER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, POST_ABORTED_LINE,
    POST_ACCESS_DENIED_LINE, POST_ADDRESSING_NOT_ALLOWED_LINE, POST_RECIPIENT_NO_ACCESS_LINE,
    READ_REQUIRES_NUMBER_LINE, SOURCE_DELETED_LINE, SOURCE_NOT_FOUND_LINE,
};
use crate::domain::messaging::forward_mail::ForwardMailError;
use crate::domain::messaging::post_mail::PostMailError;
use crate::domain::messaging::reply_to_mail::ReplyToMailError;
use crate::domain::session::typed::MenuSession;

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
        arg: NumberArg,
    ) -> Result<(), T::Error> {
        let source_number = match arg {
            NumberArg::Number(n) => n,
            NumberArg::Missing => {
                self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?;
                return Ok(());
            }
            NumberArg::Invalid => {
                self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
                return Ok(());
            }
        };

        let Some(body) = self.read_post_body(session).await? else {
            return Ok(());
        };

        let outcome = reply_mail(
            session,
            self.services.mail_stores(),
            self.services.conferences(),
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
        arg: NumberArg,
    ) -> Result<(), T::Error> {
        let source_number = match arg {
            NumberArg::Number(n) => n,
            NumberArg::Missing => {
                self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?;
                return Ok(());
            }
            NumberArg::Invalid => {
                self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
                return Ok(());
            }
        };

        let Some(typed_to) = self.read_required_line(session, FORWARD_TO_PROMPT).await? else {
            return Ok(());
        };

        let note = self.read_forward_note(session).await?;

        let outcome = forward_mail(
            session,
            self.services.user_repo(),
            self.services.mail_stores(),
            self.services.conferences(),
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
            ReplyForwardOutcome::SourceNotFound => {
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
            PostMailError::EmptyAddressee | PostMailError::AddresseeMismatch => {
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
            match self.read_prompted(b"", TerminalEcho::Visible).await? {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("/A") {
                        self.write_and_flush(POST_ABORTED_LINE).await?;
                        return Ok(None);
                    }
                    if trimmed == "." {
                        return Ok(if note.is_empty() { None } else { Some(note) });
                    }
                    if first_line && trimmed.is_empty() {
                        return Ok(None);
                    }
                    first_line = false;
                    note.push_str(&line);
                    note.push('\n');
                }
                TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                    self.write_and_flush(POST_ABORTED_LINE).await?;
                    return Ok(None);
                }
            }
        }
    }
}
