//! Terminal-free sysop mail admin commands (Slice 49b).
//!
//! `K <num>` (kill / delete), `MV <num>` (move) and `EH <num>`
//! (edit-header) end up here once the wire layer has collected the
//! arguments. Each delegates to the matching domain rule through the
//! typed [`MenuSession`].

use crate::app::mail_stores::{MailStorePairLockOutcome, MailStores};
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
pub(crate) enum DeleteOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The command was completed.
    Done,
    /// The domain rule rejected the request.
    Rejected(DeleteMailError),
}

/// Already-collected fields for an `MV <num>` command.
pub(crate) struct MoveInput {
    /// Source mail number in the current msgbase.
    pub(crate) source_number: u32,
    /// Target conference number.
    pub(crate) target_conference: u32,
    /// Target msgbase number inside `target_conference`.
    pub(crate) target_msgbase: u32,
}

/// Outcome of an `MV <num>` command.
pub(crate) enum MoveOutcome {
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
pub(crate) struct EditHeaderInput {
    /// Source mail number in the current msgbase.
    pub(crate) source_number: u32,
    /// New subject; `None` leaves the subject unchanged.
    pub(crate) new_subject: Option<String>,
    /// New addressee handle; `None` leaves the addressee unchanged.
    /// The repository lookup is performed by this use case.
    pub(crate) new_to_name: Option<String>,
}

/// Outcome of an `EH <num>` command.
pub(crate) enum EditHeaderOutcome {
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
pub(crate) async fn delete_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    number: u32,
) -> DeleteOutcome
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
pub(crate) async fn move_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    input: MoveInput,
) -> MoveOutcome
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
pub(crate) async fn edit_mail_header<R, M>(
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
