//! `MS` (multi-conference mail scan) menu command (Tier B, Slice B1).
//!
//! Delegates the walk over accessible conferences to
//! [`crate::app::menu::scan_all_mail`], then renders the legacy
//! `searchNewMail` multi-conference output: the `Scanning conferences
//! for mail...` header, a per-conference banner, and per-base either
//! the `Type/From/Subject/Msg` listing table, a `No mail today!`
//! line, or (for a base with messages but none addressed to the
//! caller) just the banner — mirroring `amiexpress/express.e:25250`
//! and `:11668-11739`.

use std::time::SystemTime;

use crate::app::menu::scan_all_mail::{scan_all_mail, BaseScanOutcome};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    render_scan_conference_banner, render_scan_listing_table, render_scan_msgbase_banner,
    MAIL_SCAN_ALL_HEADER, MAIL_SCAN_NO_MAIL_TODAY, MAIL_STORE_ERROR_LINE,
};
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_scan_all_mail(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let scans = scan_all_mail(
            session,
            self.services.mail_stores(),
            self.services.conferences(),
            SystemTime::now(),
        )
        .await;

        let mut out = Vec::new();
        out.extend_from_slice(MAIL_SCAN_ALL_HEADER);
        for conference in &scans {
            out.extend_from_slice(&render_scan_conference_banner(&conference.conference_name));
            for base in &conference.bases {
                out.extend_from_slice(&render_scan_msgbase_banner(
                    &base.msgbase_name,
                    base.first_base,
                ));
                match &base.outcome {
                    // Matched unread mail: the column table.
                    BaseScanOutcome::Listing(rows) => {
                        out.extend_from_slice(&render_scan_listing_table(rows));
                    }
                    // Nothing new since the user's last scan.
                    BaseScanOutcome::NothingNew => {
                        out.extend_from_slice(MAIL_SCAN_NO_MAIL_TODAY);
                    }
                    // Messages exist but none addressed to the caller, or
                    // no store at all: the legacy prints only the banner.
                    BaseScanOutcome::NoMatch | BaseScanOutcome::NoStore => {}
                    BaseScanOutcome::Error(err) => {
                        eprintln!("scan_all_mail base scan failed: {err}");
                        out.extend_from_slice(MAIL_STORE_ERROR_LINE);
                    }
                }
            }
        }
        self.write_and_flush(&out).await
    }
}
