//! Terminal-free `RP <num>` (reply) and `FW <num>` (forward) menu
//! command use cases (Slice 49a).
//!
//! Both load the source mail from the current msgbase, resolve any
//! addressee through the user repository, and invoke the matching
//! domain rule via the typed [`MenuSession`]. Wire prompts and
//! rendering live in `menu_flow::reply_forward`.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{find_msgbase_in, Conference, MessageBaseRef};
use crate::domain::messaging::forward_mail::{ForwardMailError, ForwardMailRequest};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::reply_to_mail::{ReplyToMailDraft, ReplyToMailError};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// Caller-collected fields for an `RP <num>` command.
pub(crate) struct ReplyInput {
    /// Source mail's number in the current msgbase.
    pub(crate) source_number: u32,
    /// User-typed body.
    pub(crate) body: String,
    /// Subject line. When `None`, the use case defaults to
    /// `"Re: <source.subject>"`.
    pub(crate) subject: Option<String>,
    /// Whether the user asked for private visibility.
    pub(crate) private: bool,
    /// Honoured only when the source is an `ALL` broadcast.
    pub(crate) reply_keeps_broadcast: bool,
    /// Posting timestamp.
    pub(crate) posted_at: SystemTime,
}

/// Caller-collected fields for an `FW <num>` command.
pub(crate) struct ForwardInput {
    /// Source mail's number in the current msgbase.
    pub(crate) source_number: u32,
    /// Typed addressee handle (case-insensitive lookup).
    pub(crate) typed_to: String,
    /// Optional additional note appended after a `--` separator.
    pub(crate) additional_note: Option<String>,
    /// Posting timestamp.
    pub(crate) posted_at: SystemTime,
}

/// Outcome of a reply / forward attempt.
pub(crate) enum ReplyForwardOutcome {
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
pub(crate) async fn reply_mail<M>(
    session: &mut MenuSession,
    mail_stores: &M,
    conferences: &[Conference],
    input: ReplyInput,
) -> ReplyForwardOutcome
where
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = current_msgbase(session) else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(store) = mail_stores.for_msgbase(visit_msgbase) else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(allowed_addressing) = find_msgbase_in(conferences, visit_msgbase)
        .map(crate::domain::conference::MessageBase::allowed_addressing)
    else {
        return ReplyForwardOutcome::NoMailBase;
    };

    let from_name = session.user().handle().to_string();
    let mut guard = store.lock().await;
    let Ok(Some(source)) = guard.load(input.source_number) else {
        return ReplyForwardOutcome::SourceNotFound;
    };
    let subject = input
        .subject
        .unwrap_or_else(|| format!("Re: {}", source.subject()));
    let result = session.reply_to_mail(
        visit_msgbase,
        allowed_addressing,
        &mut **guard,
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
pub(crate) async fn forward_mail<R, M>(
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
    let Some(visit_msgbase) = current_msgbase(session) else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(store) = mail_stores.for_msgbase(visit_msgbase) else {
        return ReplyForwardOutcome::NoMailBase;
    };
    let Some(allowed_addressing) = find_msgbase_in(conferences, visit_msgbase)
        .map(crate::domain::conference::MessageBase::allowed_addressing)
    else {
        return ReplyForwardOutcome::NoMailBase;
    };

    let trimmed = input.typed_to.trim();
    let resolved = match user_repo.find_by_handle(trimmed) {
        NameLookupResult::Found(user) => *user,
        NameLookupResult::NotFound => return ReplyForwardOutcome::UnknownAddressee,
    };

    let from_name = session.user().handle().to_string();
    let mut guard = store.lock().await;
    let Ok(Some(source)) = guard.load(input.source_number) else {
        return ReplyForwardOutcome::SourceNotFound;
    };
    let result = session.forward_mail(
        visit_msgbase,
        allowed_addressing,
        &mut **guard,
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

fn current_msgbase(session: &MenuSession) -> Option<MessageBaseRef> {
    session
        .current_msgbase()
        .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
}
