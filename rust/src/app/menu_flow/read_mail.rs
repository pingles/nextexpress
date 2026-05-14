//! `R <num>` (Read Mail) menu command (Slice 39).
//!
//! Delegates store resolution and the `messaging.allium:ReadMail`
//! rule to [`crate::app::menu::read_mail`], then renders the outcome.

use std::time::SystemTime;

use crate::app::menu::read_mail::{
    read_mail, ReadMailOutcome, ReadMailStoreFailure, ReadMailStoreOperation,
};
use crate::app::terminal::Terminal;
use crate::app::wire_text::{
    render_mail_body, render_mail_header, DELETED_MESSAGE_LINE, MAIL_STORE_ERROR_LINE,
    MESSAGE_NOT_FOUND_LINE, NO_MAIL_BASE_LINE, READ_DENIED_LINE,
};
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
        match read_mail(
            session,
            self.services.mail_stores(),
            self.services.conferences(),
            number,
            SystemTime::now(),
        )
        .await
        {
            ReadMailOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            ReadMailOutcome::MessageNotFound => {
                self.write_and_flush(MESSAGE_NOT_FOUND_LINE).await?;
            }
            ReadMailOutcome::StoreError(failure) => {
                log_read_store_failure(&failure);
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
            ReadMailOutcome::Deleted => {
                self.write_and_flush(DELETED_MESSAGE_LINE).await?;
            }
            ReadMailOutcome::Denied => {
                self.write_and_flush(READ_DENIED_LINE).await?;
            }
            ReadMailOutcome::Read {
                mail,
                conference_name,
            } => {
                let header = render_mail_header(&mail, &conference_name);
                let body = render_mail_body(mail.body());
                self.terminal.write(&header).await?;
                self.terminal.write(&body).await?;
                self.terminal.flush().await?;
            }
        }
        Ok(())
    }
}

fn log_read_store_failure(failure: &ReadMailStoreFailure) {
    match failure.operation {
        ReadMailStoreOperation::Load => {
            eprintln!(
                "R command: failed to load mail #{}: {}",
                failure.number, failure.source
            );
        }
        ReadMailStoreOperation::Save => {
            eprintln!(
                "R command: failed to save mail #{}: {}",
                failure.number, failure.source
            );
        }
    }
}
