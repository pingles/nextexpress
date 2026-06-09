//! Sysop mail-admin read sub-prompt commands (Slice 49b):
//! `K <num>` (kill / delete), `MV <num>` (move) and `EH <num>`
//! (edit header).
//!
//! The terminal-free cores ([`delete_mail`], [`move_mail`],
//! [`edit_mail_header`]) own store/repository resolution and the
//! domain-rule invocation through the typed [`MenuSession`]; the
//! `MenuFlow` handlers below drive the prompt loops and render the
//! outcomes.

use crate::app::mail_stores::{MailStorePairLockOutcome, MailStores};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    CONFIRM_DELETE_PROMPT, DELETE_DONE_LINE, EDIT_HEADER_DONE_LINE, EDIT_HEADER_SUBJECT_PROMPT,
    EDIT_HEADER_TO_PROMPT, FORWARD_UNKNOWN_USER_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, MAIL_STORE_ERROR_LINE, MOVE_DONE_PREFIX,
    MOVE_TARGET_CONFERENCE_PROMPT, MOVE_TARGET_MSGBASE_PROMPT, MOVE_UNKNOWN_TARGET_LINE,
    NO_MAIL_BASE_LINE, POST_ABORTED_LINE, SOURCE_NOT_FOUND_LINE, SYSOP_ONLY_LINE,
};
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::messaging::delete_mail::{delete_mail as delete_mail_rule, DeleteMailError};
use crate::domain::messaging::edit_mail_header::{
    edit_mail_header as edit_mail_header_rule, EditMailHeaderError,
};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::move_mail::{move_mail as move_mail_rule, MoveMailError};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// Outcome of a `K <num>` command.
enum DeleteOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The command was completed.
    Done,
    /// The domain rule rejected the request.
    Rejected(DeleteMailError),
}

/// Already-collected fields for an `MV <num>` command.
struct MoveInput {
    /// Source mail number in the current msgbase.
    source_number: u32,
    /// Target conference number.
    target_conference: u32,
    /// Target msgbase number inside `target_conference`.
    target_msgbase: u32,
}

/// Outcome of an `MV <num>` command.
enum MoveOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The supplied target msgbase coordinate is not registered.
    UnknownTarget,
    /// The mail was moved; `mail` is the new row at the target.
    Moved(Mail),
    /// The domain rule rejected the request.
    Rejected(MoveMailError),
}

/// Already-collected fields for an `EH <num>` command.
struct EditHeaderInput {
    /// Source mail number in the current msgbase.
    source_number: u32,
    /// New subject; `None` leaves the subject unchanged.
    new_subject: Option<String>,
    /// New addressee handle; `None` leaves the addressee unchanged.
    /// The repository lookup is performed by this use case.
    new_to_name: Option<String>,
}

/// Outcome of an `EH <num>` command.
enum EditHeaderOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The supplied new addressee could not be resolved.
    UnknownAddressee,
    /// The edit was applied.
    Done,
    /// The domain rule rejected the request.
    Rejected(EditMailHeaderError),
}

/// Runs the delete-mail use case (Slice 49 wired) without terminal I/O.
async fn delete_mail<M>(session: &mut MenuSession, mail_stores: &M, number: u32) -> DeleteOutcome
where
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = current_msgbase(session) else {
        return DeleteOutcome::NoMailBase;
    };
    let Some(mut guard) = mail_stores.lock(visit_msgbase).await else {
        return DeleteOutcome::NoMailBase;
    };
    let result = delete_mail_rule(session.user_mut(), &mut *guard, number);
    drop(guard);
    match result {
        Ok(()) => DeleteOutcome::Done,
        Err(err) => DeleteOutcome::Rejected(err),
    }
}

/// Runs the move-mail use case (Slice 49 wired) without terminal I/O.
async fn move_mail<M>(session: &mut MenuSession, mail_stores: &M, input: MoveInput) -> MoveOutcome
where
    M: MailStores + ?Sized,
{
    let Some(source_msgbase) = current_msgbase(session) else {
        return MoveOutcome::NoMailBase;
    };
    let target_msgbase = MessageBaseRef::new(input.target_conference, input.target_msgbase);
    let (mut source_guard, mut target_guard) =
        match mail_stores.lock_pair(source_msgbase, target_msgbase).await {
            MailStorePairLockOutcome::MissingSource => return MoveOutcome::NoMailBase,
            MailStorePairLockOutcome::MissingTarget => return MoveOutcome::UnknownTarget,
            // The domain rule rejects same-msgbase moves; the registry
            // short-circuits before locking so it can't deadlock on a
            // shared mutex. Surface the rejection through the existing
            // domain error variant for callers.
            MailStorePairLockOutcome::SameStore => {
                return MoveOutcome::Rejected(MoveMailError::SameMsgbase);
            }
            MailStorePairLockOutcome::Locked { source, target } => (source, target),
        };
    let result = move_mail_rule(
        session.user_mut(),
        &mut *source_guard,
        &mut *target_guard,
        input.source_number,
    );
    drop(target_guard);
    drop(source_guard);
    match result {
        Ok(mail) => MoveOutcome::Moved(mail),
        Err(err) => MoveOutcome::Rejected(err),
    }
}

/// Runs the edit-mail-header use case (Slice 49 wired).
async fn edit_mail_header<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    _conferences: &[Conference],
    input: EditHeaderInput,
) -> EditHeaderOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = current_msgbase(session) else {
        return EditHeaderOutcome::NoMailBase;
    };
    let Some(mut guard) = mail_stores.lock(visit_msgbase).await else {
        return EditHeaderOutcome::NoMailBase;
    };

    let new_to = if let Some(typed) = input.new_to_name {
        let trimmed = typed.trim();
        if trimmed.is_empty() {
            None
        } else {
            match user_repo.find_by_handle(trimmed) {
                NameLookupResult::Found(user) => {
                    Some((user.handle().to_string(), Some(user.slot_number())))
                }
                NameLookupResult::NotFound => return EditHeaderOutcome::UnknownAddressee,
            }
        }
    } else {
        None
    };

    let result = edit_mail_header_rule(
        session.user_mut(),
        &mut *guard,
        input.source_number,
        input.new_subject,
        new_to,
    );
    drop(guard);
    match result {
        Ok(()) => EditHeaderOutcome::Done,
        Err(err) => EditHeaderOutcome::Rejected(err),
    }
}

fn current_msgbase(session: &MenuSession) -> Option<MessageBaseRef> {
    session
        .current_msgbase()
        .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles `K <num>` — prompt for confirmation, then delete.
    pub(super) async fn handle_kill(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        let Some(line) = self
            .read_required_line(session, CONFIRM_DELETE_PROMPT, false)
            .await?
        else {
            return Ok(());
        };
        if !matches!(line.chars().next(), Some('y' | 'Y')) {
            self.write_and_flush(POST_ABORTED_LINE).await?;
            return Ok(());
        }

        let outcome = delete_mail(session, self.services.mail_stores.as_ref(), number).await;
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
    /// msgbase numbers, then move. Returns `true` only when the
    /// message was actually moved, so the read sub-prompt can honour
    /// the legacy "advance only on a successful move" navigation
    /// (`express.e:12172`); every abort / rejection returns `false`.
    pub(super) async fn handle_move_mail(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<bool, T::Error> {
        let Some(conf_line) = self
            .read_required_line(session, MOVE_TARGET_CONFERENCE_PROMPT, false)
            .await?
        else {
            return Ok(false);
        };
        let Ok(target_conf) = conf_line.parse::<u32>() else {
            self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
            return Ok(false);
        };
        let Some(mb_line) = self
            .read_required_line(session, MOVE_TARGET_MSGBASE_PROMPT, false)
            .await?
        else {
            return Ok(false);
        };
        let Ok(target_mb) = mb_line.parse::<u32>() else {
            self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
            return Ok(false);
        };

        let outcome = move_mail(
            session,
            self.services.mail_stores.as_ref(),
            MoveInput {
                source_number: number,
                target_conference: target_conf,
                target_msgbase: target_mb,
            },
        )
        .await;
        let moved = match outcome {
            MoveOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
                false
            }
            MoveOutcome::UnknownTarget => {
                self.write_and_flush(MOVE_UNKNOWN_TARGET_LINE).await?;
                false
            }
            MoveOutcome::Moved(mail) => {
                let mut line = MOVE_DONE_PREFIX.to_vec();
                line.extend_from_slice(mail.number().to_string().as_bytes());
                line.extend_from_slice(b".\r\n");
                self.write_and_flush(&line).await?;
                true
            }
            MoveOutcome::Rejected(err) => {
                self.render_move_error(err).await?;
                false
            }
        };
        Ok(moved)
    }

    /// Handles `EH <num>` — prompt for new subject and/or new
    /// addressee (blank keeps), then edit.
    pub(super) async fn handle_edit_header(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        let new_subject = self
            .read_optional_unchanged_line(session, EDIT_HEADER_SUBJECT_PROMPT)
            .await?;
        let new_to_name = self
            .read_optional_unchanged_line(session, EDIT_HEADER_TO_PROMPT)
            .await?;

        let outcome = edit_mail_header(
            session,
            self.services.user_repo.as_ref(),
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
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
