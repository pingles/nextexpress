//! `R <num>` (Read Mail) menu command (Slice 39).
//!
//! Loads the requested mail from the per-msgbase store, applies the
//! `messaging.allium:ReadMail` rule (mutating both the bound user's
//! read pointers and the mail's `received_at`), persists the mail back
//! to the store, and renders header + body to the terminal.

use std::time::SystemTime;

use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    render_mail_body, render_mail_header, DELETED_MESSAGE_LINE, MAIL_STORE_ERROR_LINE,
    MESSAGE_NOT_FOUND_LINE, NO_MAIL_BASE_LINE, READ_DENIED_LINE,
};
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::read_mail::ReadMailError;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_read_mail(
        &mut self,
        session: &mut MenuSession,
        number: u32,
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

        // Resolve the conference name up-front so the immutable borrow
        // on `services.conferences()` doesn't overlap the mutable
        // borrows below.
        let conf_name = self
            .services
            .conferences()
            .iter()
            .find(|c| c.number() == visit_msgbase.conference_number())
            .map(|c| c.name().to_string())
            .unwrap_or_default();

        let mut guard = store.lock().await;
        let mut mail = match guard.load(number) {
            Ok(Some(mail)) => mail,
            Ok(None) => {
                self.write_and_flush(MESSAGE_NOT_FOUND_LINE).await?;
                return Ok(());
            }
            Err(err) => {
                eprintln!("R command: failed to load mail #{number}: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                return Ok(());
            }
        };

        match session.read_mail(&mut mail, SystemTime::now()) {
            Ok(()) => {}
            Err(ReadMailError::Deleted) => {
                self.write_and_flush(DELETED_MESSAGE_LINE).await?;
                return Ok(());
            }
            Err(
                ReadMailError::AccessDenied
                | ReadMailError::NotPermitted
                | ReadMailError::NoMembership,
            ) => {
                self.write_and_flush(READ_DENIED_LINE).await?;
                return Ok(());
            }
        }

        if let Err(err) = guard.save(&mail) {
            eprintln!("R command: failed to save mail #{number}: {err}");
            self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            return Ok(());
        }
        // Lock can be released before rendering — the mail is owned.
        drop(guard);

        let header = render_mail_header(&mail, &conf_name);
        let body = render_mail_body(mail.body());
        self.terminal.write(&header).await?;
        self.terminal.write(&body).await?;
        self.terminal.flush().await?;
        Ok(())
    }
}
