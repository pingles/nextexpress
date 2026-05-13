//! `M` / `N` (Scan Mail) menu command (Slice 40).
//!
//! Resolves the per-session msgbase + the [`crate::domain::conference::AllScanScope`]
//! from the conference catalogue, locks the matching mail store, runs
//! `messaging.allium:ScanMail`, then renders the summary line.

use std::time::SystemTime;

use crate::app::menu_command::ScanArg;
use crate::app::terminal::Terminal;
use crate::app::wire_text::{render_scan_summary, MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE};
use crate::domain::conference::{find_msgbase_in, MessageBaseRef};
use crate::domain::session::typed::{MenuSession, ScanOnJoin};

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_scan_mail(
        &mut self,
        session: &mut MenuSession,
        scan: ScanArg,
    ) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(store) = self.services.mail_stores().for_msgbase(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let from_message = match scan {
            // N => start from `last_scanned + 1` (the "new mail since"
            // semantics the spec encodes with `from_message = 0`).
            ScanArg::New => 0,
            // M => start from message 1 (caller-controlled walk).
            ScanArg::All => 1,
        };
        let scope = find_msgbase_in(self.services.conferences(), visit_msgbase)
            .map(crate::domain::conference::MessageBase::all_scan_scope)
            .unwrap_or_default();

        let guard = store.lock().await;
        let result = match session.scan_mail(
            &**guard,
            visit_msgbase,
            scope,
            from_message,
            SystemTime::now(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("scan_mail failed: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                return Ok(());
            }
        };
        drop(guard);

        let summary = render_scan_summary(result.unread_count, result.first_unread_number);
        self.write_and_flush(&summary).await?;
        Ok(())
    }
}
