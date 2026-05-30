//! Terminal-free `L` (list messages) use case for the read sub-prompt.
//!
//! Collects the messages the legacy `listMSGs` (`amiexpress/express.e:8820`)
//! would show the reader — those addressed to them or broadcast to
//! `ALL` / `EALL`, excluding deleted mail — so the interactive handler
//! can paginate them without holding the message-base lock across the
//! reader's keypresses.

use crate::app::mail_stores::MailStores;
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility};
use crate::domain::messaging::scan_mail::MailScanRow;
use crate::domain::session::typed::MenuSession;

/// The listable mail in the session's current message base.
pub(crate) struct ListMail {
    /// Lowest non-deleted message number — the start prompt's default
    /// (legacy `mailStat.lowestNotDel`, `express.e:8831`).
    pub(crate) lowest: u32,
    /// Every non-deleted message addressed to the reader (or broadcast),
    /// in ascending number order.
    pub(crate) rows: Vec<MailScanRow>,
}

/// Collects the reader's listable mail from the current base. Returns
/// `None` when the session has no current base or no store is registered
/// for it.
pub(crate) async fn list_mail<M>(session: &MenuSession, mail_stores: &M) -> Option<ListMail>
where
    M: MailStores + ?Sized,
{
    let (conference, msgbase) = session.current_msgbase()?;
    let msgbase = MessageBaseRef::new(conference, msgbase);
    let guard = mail_stores.lock(msgbase).await?;
    let highest = guard.highest_message();
    let lowest = guard.lowest_undeleted_message();
    let reader_slot = session.user().slot_number();
    let mut rows = Vec::new();
    for number in 1..=highest {
        if let Ok(Some(mail)) = guard.load(number) {
            if addressed_to_reader(reader_slot, &mail) {
                rows.push(MailScanRow {
                    msgbase,
                    number: mail.number(),
                    visibility: mail.visibility(),
                    from_name: mail.from_name().to_string(),
                    to_name: mail.to_name().to_string(),
                    broadcast_to: mail.broadcast_to(),
                    subject: mail.subject().to_string(),
                });
            }
        }
    }
    drop(guard);
    Some(ListMail { lowest, rows })
}

/// True when a `mail` is one the reader should see in `L`: not deleted,
/// and either addressed to them or a broadcast to `ALL` / `EALL`
/// (mirrors the `toName` test at `express.e:8854`, in `NextExpress`'s
/// slot / broadcast model).
fn addressed_to_reader(reader_slot: u32, mail: &Mail) -> bool {
    if matches!(mail.visibility(), MailVisibility::Deleted) {
        return false;
    }
    mail.addressee_slot() == Some(reader_slot)
        || matches!(mail.broadcast_to(), BroadcastTo::All | BroadcastTo::Eall)
}
