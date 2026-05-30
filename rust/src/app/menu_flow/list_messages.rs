//! `L` (list messages) handler for the read sub-prompt — the legacy
//! `listMSGs` (`amiexpress/express.e:8820`).
//!
//! Prompts for a starting message number, then prints the
//! addressed-to-reader table (message number first), paginating with the
//! shared [`Pager`] so a long base pauses with the legacy
//! `(Pause)...More(y/n/ns)? ` prompt.

use std::time::SystemTime;

use crate::app::menu::list_mail::list_mail;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{render_list_header, render_list_row};
use crate::domain::session::typed::MenuSession;

use super::pager::{PageBreak, Pager};

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
        let Some(listing) = list_mail(session, self.services.mail_stores()).await else {
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
