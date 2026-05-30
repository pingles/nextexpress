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
        // A message is displayed first; only then does the legacy
        // `readMSG` sub-prompt loop take over (`express.e:11972`). The
        // not-found / deleted / denied / error notices return straight
        // to the menu — there is no current message to operate on.
        if self.read_and_render(session, number).await? {
            self.run_read_subprompt(session, number).await?;
        }
        Ok(())
    }

    /// Reads message `number` through the terminal-free use case and
    /// renders the outcome. Returns `true` when a message was actually
    /// displayed (the sub-prompt's precondition), `false` for every
    /// notice path. Shared by the initial `R <num>` read and the
    /// sub-prompt's `<CR>`-advance.
    pub(super) async fn read_and_render(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<bool, T::Error> {
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
                Ok(false)
            }
            ReadMailOutcome::MessageNotFound => {
                self.write_and_flush(MESSAGE_NOT_FOUND_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::StoreError(failure) => {
                log_read_store_failure(&failure);
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::Deleted => {
                self.write_and_flush(DELETED_MESSAGE_LINE).await?;
                Ok(false)
            }
            ReadMailOutcome::Denied => {
                self.write_and_flush(READ_DENIED_LINE).await?;
                Ok(false)
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
                Ok(true)
            }
        }
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
