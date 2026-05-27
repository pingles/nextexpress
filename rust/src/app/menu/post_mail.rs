//! Terminal-free post-mail and comment-to-sysop use cases.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::domain::conference::{find_msgbase_in, Conference, MessageBaseRef};
use crate::domain::messaging::mail::{BroadcastTo, Mail};
use crate::domain::messaging::post_comment_to_sysop::CommentToSysopDraft;
use crate::domain::messaging::post_mail::{PostMailDraft, PostMailError};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository};

/// Already-collected fields for an `E` command.
pub(crate) struct PostMailInput {
    /// Raw recipient line, either supplied inline or via the `To:`
    /// prompt.
    pub(crate) typed_to: String,
    /// Subject line.
    pub(crate) subject: String,
    /// Whether the user requested private visibility.
    pub(crate) private: bool,
    /// Message body.
    pub(crate) body: String,
    /// Posting timestamp.
    pub(crate) posted_at: SystemTime,
}

/// Already-collected fields for a `C` command.
pub(crate) struct CommentToSysopInput {
    /// Subject line.
    pub(crate) subject: String,
    /// Message body.
    pub(crate) body: String,
    /// Posting timestamp.
    pub(crate) posted_at: SystemTime,
}

/// Outcome of a terminal-free post command.
pub(crate) enum PostMailOutcome {
    /// The session has no usable message base.
    NoMailBase,
    /// The named addressee does not exist.
    UnknownUser,
    /// The named addressee is not a member of the current conference.
    RecipientNoAccess,
    /// No sysop user exists for `C`.
    NoSysop,
    /// The message was persisted.
    Posted(Mail),
    /// The domain rule rejected the draft, or the store failed.
    Rejected(PostMailError),
}

/// Runs the post-mail use case without terminal I/O.
pub(crate) async fn post_mail<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    conferences: &[Conference],
    input: PostMailInput,
) -> PostMailOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = current_msgbase(session) else {
        return PostMailOutcome::NoMailBase;
    };

    let Some(mut guard) = mail_stores.lock(visit_msgbase).await else {
        return PostMailOutcome::NoMailBase;
    };

    let (broadcast_to, to_name, addressee_slot) = match classify_recipient(&input.typed_to) {
        Recipient::Broadcast(kind, label) => (kind, label, None),
        Recipient::Individual(typed) => {
            let resolved = match user_repo.find_by_handle(&typed) {
                NameLookupResult::Found(user) => *user,
                NameLookupResult::NotFound => return PostMailOutcome::UnknownUser,
            };
            let Some(conference) = conferences
                .iter()
                .find(|c| c.number() == visit_msgbase.conference_number())
            else {
                return PostMailOutcome::NoMailBase;
            };
            if !resolved.has_membership(conference) {
                return PostMailOutcome::RecipientNoAccess;
            }
            (
                BroadcastTo::None,
                resolved.handle().to_string(),
                Some(resolved.slot_number()),
            )
        }
    };

    let Some(allowed_addressing) = find_msgbase_in(conferences, visit_msgbase)
        .map(crate::domain::conference::MessageBase::allowed_addressing)
    else {
        return PostMailOutcome::NoMailBase;
    };

    let author_handle = session.user().handle().to_string();
    let result = session.post_mail(
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        PostMailDraft {
            to_name,
            broadcast_to,
            addressee_slot,
            from_name: author_handle,
            subject: input.subject,
            body: input.body,
            private: input.private,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => PostMailOutcome::Posted(mail),
        Err(err) => PostMailOutcome::Rejected(err),
    }
}

/// Runs the comment-to-sysop use case without terminal I/O.
pub(crate) async fn post_comment_to_sysop<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    conferences: &[Conference],
    input: CommentToSysopInput,
) -> PostMailOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some(visit_msgbase) = current_msgbase(session) else {
        return PostMailOutcome::NoMailBase;
    };

    let Some(mut guard) = mail_stores.lock(visit_msgbase).await else {
        return PostMailOutcome::NoMailBase;
    };

    let sysop = match user_repo.find_sysop() {
        NameLookupResult::Found(user) => *user,
        NameLookupResult::NotFound => return PostMailOutcome::NoSysop,
    };

    let Some(allowed_addressing) = find_msgbase_in(conferences, visit_msgbase)
        .map(crate::domain::conference::MessageBase::allowed_addressing)
    else {
        return PostMailOutcome::NoMailBase;
    };

    let from_name = session.user().handle().to_string();
    let result = session.post_comment_to_sysop(
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        CommentToSysopDraft {
            sysop_slot: sysop.slot_number(),
            from_name,
            subject: input.subject,
            body: input.body,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => PostMailOutcome::Posted(mail),
        Err(err) => PostMailOutcome::Rejected(err),
    }
}

fn current_msgbase(session: &MenuSession) -> Option<MessageBaseRef> {
    session
        .current_msgbase()
        .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
}

/// Outcome of classifying the recipient typed at the `To:` prompt.
enum Recipient {
    /// `ALL` or `EALL` (case-insensitive). The `label` is the
    /// upper-case form used as `Mail.to_name`.
    Broadcast(BroadcastTo, String),
    /// A literal user handle the caller must resolve through the user
    /// repository.
    Individual(String),
}

/// Maps a typed `To:` line to a [`Recipient`]. An empty line reroutes
/// to ALL, matching legacy `enterMSG`
/// (`amiexpress/express.e:10827`).
fn classify_recipient(typed: &str) -> Recipient {
    let trimmed = typed.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("ALL") {
        return Recipient::Broadcast(BroadcastTo::All, "ALL".to_string());
    }
    if trimmed.eq_ignore_ascii_case("EALL") {
        return Recipient::Broadcast(BroadcastTo::Eall, "EALL".to_string());
    }
    Recipient::Individual(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_all(result: Recipient, context: &str) {
        if let Recipient::Broadcast(BroadcastTo::All, label) = result {
            assert_eq!(label, "ALL", "{context}: wrong ALL label");
        } else {
            panic!(
                "{context}: expected Recipient::Broadcast(All, _), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    fn assert_eall(result: Recipient, context: &str) {
        if let Recipient::Broadcast(BroadcastTo::Eall, label) = result {
            assert_eq!(label, "EALL", "{context}: wrong EALL label");
        } else {
            panic!(
                "{context}: expected Recipient::Broadcast(Eall, _), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    fn assert_individual(result: Recipient, expected: &str, context: &str) {
        if let Recipient::Individual(handle) = result {
            assert_eq!(handle, expected, "{context}: wrong handle");
        } else {
            panic!(
                "{context}: expected Recipient::Individual(_), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    #[test]
    fn empty_recipient_reroutes_to_all() {
        // Legacy `enterMSG` reroute (`amiexpress/express.e:10827`).
        assert_all(classify_recipient(""), "empty");
        assert_all(classify_recipient("   "), "whitespace");
    }

    #[test]
    fn all_and_eall_are_case_insensitive() {
        for typed in ["ALL", "all", "All"] {
            assert_all(classify_recipient(typed), typed);
        }
        for typed in ["EALL", "eall", "EAll"] {
            assert_eall(classify_recipient(typed), typed);
        }
    }

    #[test]
    fn ordinary_handle_is_individual() {
        assert_individual(classify_recipient("alice"), "alice", "alice");
    }

    #[test]
    fn handle_is_trimmed() {
        assert_individual(classify_recipient("  alice  "), "alice", "trimmed alice");
    }

    fn recipient_kind(r: &Recipient) -> &'static str {
        match r {
            Recipient::Broadcast(BroadcastTo::None, _) => "broadcast(None)",
            Recipient::Broadcast(BroadcastTo::All, _) => "broadcast(All)",
            Recipient::Broadcast(BroadcastTo::Eall, _) => "broadcast(Eall)",
            Recipient::Individual(_) => "individual",
        }
    }
}
