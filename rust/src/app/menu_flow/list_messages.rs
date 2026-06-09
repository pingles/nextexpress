//! `L` (list messages) sub-prompt command — the legacy `listMSGs`
//! (`amiexpress/express.e:8820`).
//!
//! The terminal-free core ([`list_mail`]) collects the messages the
//! reader should see — those addressed to them or broadcast to
//! `ALL` / `EALL`, excluding deleted mail — so the handler below can
//! paginate them without holding the message-base lock across the
//! reader's keypresses. The handler prompts for a starting message
//! number, then prints the addressed-to-reader table (message number
//! first), paginating with the shared [`Pager`] so a long base pauses
//! with the legacy `(Pause)...More(y/n/ns)? ` prompt.

use std::time::SystemTime;

use crate::app::mail_stores::MailStores;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{render_list_header, render_list_row};
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::mail::{BroadcastTo, Mail, MailVisibility};
use crate::domain::messaging::scan_mail::MailScanRow;
use crate::domain::session::typed::MenuSession;

use super::pager::{PageBreak, Pager};

/// The listable mail in the session's current message base.
struct ListMail {
    /// Lowest non-deleted message number — the start prompt's default
    /// (legacy `mailStat.lowestNotDel`, `express.e:8831`).
    lowest: u32,
    /// Every non-deleted message addressed to the reader (or broadcast),
    /// in ascending number order.
    rows: Vec<MailScanRow>,
}

/// Collects the reader's listable mail from the current base. Returns
/// `None` when the session has no current base or no store is registered
/// for it.
async fn list_mail<M>(session: &MenuSession, mail_stores: &M) -> Option<ListMail>
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

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles the sub-prompt's `L`ist option (`express.e:12220`):
    /// collect the reader's mail, prompt for a starting number, then
    /// print the paginated table. Returns to the sub-prompt afterwards
    /// (legacy `nextMenu`).
    pub(super) async fn handle_list_messages(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let Some(listing) = list_mail(session, self.services.mail_stores.as_ref()).await else {
            return Ok(());
        };
        let Some(start) = self.read_list_start(session, listing.lowest).await? else {
            // A non-numeric start aborts the listing (legacy `Val` -> 0,
            // `express.e:8840`).
            return Ok(());
        };

        let mut rows = listing.rows;
        rows.retain(|row| row.number >= start);
        if rows.is_empty() {
            return Ok(());
        }

        self.write_and_flush(&render_list_header()).await?;
        // The header counts as four lines toward the page (`:8860`).
        let mut pager = Pager::new(session.user().line_length());
        pager.add_lines(4);
        for row in &rows {
            self.terminal.write(&render_list_row(row)).await?;
            self.terminal.flush().await?;
            if self.page_break(&mut pager).await? == PageBreak::Abort {
                break;
            }
        }
        Ok(())
    }

    /// Renders the `Starting message [<lowest>]: ` prompt
    /// (`express.e:8831`) and reads the reader's choice: a blank line
    /// defaults to `lowest`, a number selects it, and any non-numeric
    /// input (or EOF / idle) aborts the listing.
    async fn read_list_start(
        &mut self,
        session: &mut MenuSession,
        lowest: u32,
    ) -> Result<Option<u32>, T::Error> {
        let mut prompt = b"\x1b[32mStarting message \x1b[33m[\x1b[0m".to_vec();
        prompt.extend_from_slice(lowest.to_string().as_bytes());
        prompt.extend_from_slice(b"\x1b[33m]\x1b[0m: ");
        match self.read_prompted(&prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return Ok(Some(lowest));
                }
                Ok(trimmed.parse::<u32>().ok())
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => Ok(None),
        }
    }
}
