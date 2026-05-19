//! Sysop mail-admin menu commands (Slice 49b):
//! `K <num>` (kill / delete), `MV <num>` (move) and `EH <num>`
//! (edit header).
//!
//! Drives the prompt loop for each command and delegates the
//! terminal-free effect to [`crate::app::menu::sysop_admin`].

use crate::app::menu::sysop_admin::{
    delete_mail, edit_mail_header, move_mail, DeleteOutcome, EditHeaderInput, EditHeaderOutcome,
    MoveInput, MoveOutcome,
};
use crate::app::menu_command::NumberArg;
use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    CONFIRM_DELETE_PROMPT, DELETE_DONE_LINE, EDIT_HEADER_DONE_LINE, EDIT_HEADER_SUBJECT_PROMPT,
    EDIT_HEADER_TO_PROMPT, FORWARD_UNKNOWN_USER_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, MAIL_STORE_ERROR_LINE, MOVE_DONE_PREFIX,
    MOVE_TARGET_CONFERENCE_PROMPT, MOVE_TARGET_MSGBASE_PROMPT, MOVE_UNKNOWN_TARGET_LINE,
    NO_MAIL_BASE_LINE, POST_ABORTED_LINE, READ_REQUIRES_NUMBER_LINE, SOURCE_NOT_FOUND_LINE,
    SYSOP_ONLY_LINE,
};
use crate::domain::messaging::delete_mail::DeleteMailError;
use crate::domain::messaging::edit_mail_header::EditMailHeaderError;
use crate::domain::messaging::move_mail::MoveMailError;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles `K <num>` — prompt for confirmation, then delete.
    pub(super) async fn handle_kill(
        &mut self,
        session: &mut MenuSession,
        arg: NumberArg,
    ) -> Result<(), T::Error> {
        let number = match arg {
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

        let Some(line) = self
            .read_required_line(session, CONFIRM_DELETE_PROMPT)
            .await?
        else {
            return Ok(());
        };
        if !matches!(line.chars().next(), Some('y' | 'Y')) {
            self.write_and_flush(POST_ABORTED_LINE).await?;
            return Ok(());
        }

        let outcome = delete_mail(session, self.services.mail_stores(), number).await;
        match outcome {
            DeleteOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            DeleteOutcome::Done => {
                self.write_and_flush(DELETE_DONE_LINE).await?;
            }
            DeleteOutcome::Rejected(err) => {
                self.render_delete_error(err).await?;
            }
        }
        Ok(())
    }

    /// Handles `MV <num>` — prompt for the target conference +
    /// msgbase numbers, then move.
    pub(super) async fn handle_move_mail(
        &mut self,
        session: &mut MenuSession,
        arg: NumberArg,
    ) -> Result<(), T::Error> {
        let number = match arg {
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

        let Some(conf_line) = self
            .read_required_line(session, MOVE_TARGET_CONFERENCE_PROMPT)
            .await?
        else {
            return Ok(());
        };
        let Ok(target_conf) = conf_line.parse::<u32>() else {
            self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
            return Ok(());
        };
        let Some(mb_line) = self
            .read_required_line(session, MOVE_TARGET_MSGBASE_PROMPT)
            .await?
        else {
            return Ok(());
        };
        let Ok(target_mb) = mb_line.parse::<u32>() else {
            self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
            return Ok(());
        };

        let outcome = move_mail(
            session,
            self.services.mail_stores(),
            MoveInput {
                source_number: number,
                target_conference: target_conf,
                target_msgbase: target_mb,
            },
        )
        .await;
        match outcome {
            MoveOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            MoveOutcome::UnknownTarget => {
                self.write_and_flush(MOVE_UNKNOWN_TARGET_LINE).await?;
            }
            MoveOutcome::Moved(mail) => {
                let mut line = MOVE_DONE_PREFIX.to_vec();
                line.extend_from_slice(mail.number().to_string().as_bytes());
                line.extend_from_slice(b".\r\n");
                self.write_and_flush(&line).await?;
            }
            MoveOutcome::Rejected(err) => {
                self.render_move_error(err).await?;
            }
        }
        Ok(())
    }

    /// Handles `EH <num>` — prompt for new subject and/or new
    /// addressee (blank keeps), then edit.
    pub(super) async fn handle_edit_header(
        &mut self,
        session: &mut MenuSession,
        arg: NumberArg,
    ) -> Result<(), T::Error> {
        let number = match arg {
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

        let new_subject = self
            .read_optional_unchanged_line(session, EDIT_HEADER_SUBJECT_PROMPT)
            .await?;
        let new_to_name = self
            .read_optional_unchanged_line(session, EDIT_HEADER_TO_PROMPT)
            .await?;

        let outcome = edit_mail_header(
            session,
            self.services.user_repo(),
            self.services.mail_stores(),
            self.services.conferences(),
            EditHeaderInput {
                source_number: number,
                new_subject,
                new_to_name,
            },
        )
        .await;

        match outcome {
            EditHeaderOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            EditHeaderOutcome::UnknownAddressee => {
                self.write_and_flush(FORWARD_UNKNOWN_USER_LINE).await?;
            }
            EditHeaderOutcome::Done => {
                self.write_and_flush(EDIT_HEADER_DONE_LINE).await?;
            }
            EditHeaderOutcome::Rejected(err) => {
                self.render_edit_header_error(err).await?;
            }
        }
        Ok(())
    }

    async fn render_delete_error(&mut self, err: DeleteMailError) -> Result<(), T::Error> {
        match err {
            DeleteMailError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            DeleteMailError::AlreadyDeleted => {
                // Mirror SOURCE_DELETED_LINE-ish surface: the user
                // tried to delete a deleted mail. Re-using
                // POST_ABORTED for now; bespoke wording can land
                // later.
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            DeleteMailError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            DeleteMailError::Store(err) => {
                eprintln!("K command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    async fn render_move_error(&mut self, err: MoveMailError) -> Result<(), T::Error> {
        match err {
            MoveMailError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            MoveMailError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            MoveMailError::SameMsgbase => {
                self.write_and_flush(MOVE_UNKNOWN_TARGET_LINE).await?;
            }
            MoveMailError::Store(err) => {
                eprintln!("MV command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    async fn render_edit_header_error(&mut self, err: EditMailHeaderError) -> Result<(), T::Error> {
        match err {
            EditMailHeaderError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            EditMailHeaderError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            EditMailHeaderError::Store(err) => {
                eprintln!("EH command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    /// Reads a single trimmed line in response to `prompt`; returns
    /// `Some(None)` for the blank-input case (semantics: "keep
    /// current"), `Some(Some(value))` for non-empty input,
    /// `None` for EOF/idle (the prompt aborted).
    async fn read_optional_unchanged_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
    ) -> Result<Option<String>, T::Error> {
        use crate::app::terminal::{TerminalEcho, TerminalRead};
        use std::time::SystemTime;
        match self.read_prompted(prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed.to_string()))
                }
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => Ok(None),
        }
    }
}
