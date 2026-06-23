//! `MS` (multi-conference mail scan) menu command (Tier B, Slice B1).
//!
//! The terminal-free walk over accessible conferences lives in the
//! [`core`] submodule; this module renders the legacy `searchNewMail`
//! multi-conference output: the `Scanning conferences for mail...`
//! header, a per-conference banner, and per-base either the
//! `Type/From/Subject/Msg` listing table, a `No mail today!` line, or
//! (for a base with messages but none addressed to the caller) just
//! the banner — mirroring `amiexpress/express.e:25250` and
//! `:11668-11739`.

mod core;

pub(super) use self::core::ScanFilter;

use std::time::SystemTime;

use self::core::{scan_all_mail, BaseScanOutcome};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_scan_conference_banner, render_scan_listing_table, render_scan_msgbase_banner,
    MAIL_SCAN_ALL_HEADER, MAIL_SCAN_NO_MAIL_TODAY, MAIL_SCAN_READ_IT_NOW_PROMPT,
    MAIL_STORE_ERROR_LINE,
};
use crate::domain::messaging::scan_mail::MailScanRow;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_scan_all_mail(
        &mut self,
        session: &mut MenuSession,
        filter: ScanFilter,
    ) -> Result<(), T::Error> {
        let scans = scan_all_mail(
            session,
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            filter,
            SystemTime::now(),
        )
        .await;

        // The caller's home conference/base, restored after every
        // read-it-now detour so the menu prompt that follows `MS` shows
        // where the caller actually is (`amiexpress/express.e:25270`).
        let home = session.current_msgbase();

        self.write_and_flush(MAIL_SCAN_ALL_HEADER).await?;
        for conference in &scans {
            self.write_and_flush(&render_scan_conference_banner(&conference.conference_name))
                .await?;
            for base in &conference.bases {
                self.write_and_flush(&render_scan_msgbase_banner(
                    &base.msgbase_name,
                    base.first_base,
                ))
                .await?;
                match &base.outcome {
                    // Matched unread mail: the column table, then the
                    // legacy read-it-now offer.
                    BaseScanOutcome::Listing(rows) => {
                        self.write_and_flush(&render_scan_listing_table(rows))
                            .await?;
                        self.offer_read_it_now(session, rows, home).await?;
                    }
                    // Nothing new since the user's last scan.
                    BaseScanOutcome::NothingNew => {
                        self.write_and_flush(MAIL_SCAN_NO_MAIL_TODAY).await?;
                    }
                    // Messages exist but none addressed to the caller, or
                    // no store at all: the legacy prints only the banner.
                    BaseScanOutcome::NoMatch | BaseScanOutcome::NoStore => {}
                    BaseScanOutcome::Error(err) => {
                        eprintln!("scan_all_mail base scan failed: {err}");
                        self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Asks `Would you like to read it now ` for a base whose listing
    /// matched unread mail and, on Yes, drops into the read/reply
    /// sub-prompt for the found messages — the legacy `searchNewMail`
    /// getOUT branch (`amiexpress/express.e:11738-11765`). The found
    /// base is attached as a transient read visit and the caller's
    /// `home` coordinate restored afterwards, mirroring the legacy
    /// `currentConf:=cn ... :=oldcn`.
    async fn offer_read_it_now(
        &mut self,
        session: &mut MenuSession,
        rows: &[MailScanRow],
        home: Option<(u32, u32)>,
    ) -> Result<(), T::Error> {
        let Some(first) = rows.first() else {
            return Ok(());
        };
        if !self.prompt_read_it_now(session).await? {
            return Ok(());
        }
        let coord = first.msgbase;
        let start = first.number;
        session.attach_read_visit(
            coord.conference_number(),
            coord.msgbase_number(),
            SystemTime::now(),
        );
        if self.read_and_render(session, start).await? {
            self.run_read_subprompt(session, start + 1, Some(start))
                .await?;
        }
        if let Some((conference, msgbase)) = home {
            session.attach_read_visit(conference, msgbase, SystemTime::now());
        }
        Ok(())
    }

    /// Reads the read-it-now answer, mirroring the legacy `yesNo(1)`
    /// default-Yes semantics: an empty line / `y` accepts, only an
    /// explicit `n`/`N` declines. EOF or idle declines without reading.
    /// A trailing CRLF follows the answer (the legacy `aePuts('\b\n')`
    /// at `amiexpress/express.e:11741`).
    async fn prompt_read_it_now(&mut self, session: &mut MenuSession) -> Result<bool, T::Error> {
        match self
            .read_prompted(MAIL_SCAN_READ_IT_NOW_PROMPT, TerminalEcho::Visible)
            .await?
        {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                self.write_newline().await?;
                Ok(!matches!(line.trim().chars().next(), Some('n' | 'N')))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => Ok(false),
        }
    }
}
