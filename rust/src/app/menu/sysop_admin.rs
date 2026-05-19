//! Terminal-free sysop mail admin commands (Slice 49b).
//!
//! `K <num>` (kill / delete), `MV <num>` (move) and `EH <num>`
//! (edit-header) end up here once the wire layer has collected the
//! arguments. Each delegates to the matching domain rule through the
//! typed [`MenuSession`].

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::messaging::delete_mail::DeleteMailError;
use crate::domain::messaging::edit_mail_header::EditMailHeaderError;
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::move_mail::MoveMailError;
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
    let Some(store) = mail_stores.for_msgbase(visit_msgbase) else {
        return DeleteOutcome::NoMailBase;
    };
    let mut guard = store.lock().await;
    let result = session.delete_mail(&mut **guard, number);
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
    let Some(source_store) = mail_stores.for_msgbase(source_msgbase) else {
        return MoveOutcome::NoMailBase;
    };
    let Some(target_store) = mail_stores.for_msgbase(target_msgbase) else {
        return MoveOutcome::UnknownTarget;
    };
    let mut source_guard = source_store.lock().await;
    let mut target_guard = target_store.lock().await;
    let result = session.move_mail(
        &mut **source_guard,
        &mut **target_guard,
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
    let Some(store) = mail_stores.for_msgbase(visit_msgbase) else {
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

    let mut guard = store.lock().await;
    let result =
        session.edit_mail_header(&mut **guard, input.source_number, input.new_subject, new_to);
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
