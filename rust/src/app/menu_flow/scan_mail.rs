//! `M` / `N` (Scan Mail) menu command (Slice 40).
//!
//! Delegates store resolution and the `messaging.allium:ScanMail`
//! rule to [`crate::app::menu::scan_mail`], then renders the summary.

use std::time::SystemTime;

use crate::app::menu::scan_mail::{scan_mail, ScanMailOutcome};
use crate::app::menu_command::ScanArg;
use crate::app::terminal::Terminal;
use crate::app::wire_text::{render_scan_summary, MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE};
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_scan_mail(
        &mut self,
        session: &mut MenuSession,
        scan: ScanArg,
    ) -> Result<(), T::Error> {
        let from_message = match scan {
            // N => start from `last_scanned + 1` (the "new mail since"
            // semantics the spec encodes with `from_message = 0`).
            ScanArg::New => 0,
            // M => start from message 1 (caller-controlled walk).
            ScanArg::All => 1,
        };
        match scan_mail(
            session,
            self.services.mail_stores(),
            self.services.conferences(),
            from_message,
            SystemTime::now(),
        )
        .await
        {
            ScanMailOutcome::NoOpenMsgbase | ScanMailOutcome::NoStore => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            ScanMailOutcome::StoreError(err) => {
                eprintln!("scan_mail failed: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
            ScanMailOutcome::Scanned(result) => {
                let summary = render_scan_summary(result.unread_count, result.first_unread_number);
                self.write_and_flush(&summary).await?;
            }
        }
        Ok(())
    }
}
