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
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::read_pointers::ReadPointers;
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
        // `R <num>` is read-first (legacy `passItIN` -> `goNextMsg`,
        // `express.e:12003-12004`): the message is displayed, then the
        // sub-prompt loop opens with the pointer advanced past it. The
        // not-found / deleted / denied / error notices return straight to
        // the menu — there is no current message to operate on.
        if self.read_and_render(session, number).await? {
            self.run_read_subprompt(session, number + 1, Some(number))
                .await?;
        }
        Ok(())
    }

    /// Bare `R` (no message number): opens the read sub-prompt
    /// PROMPT-FIRST at the caller's resume point — the legacy `readMSG`
    /// no-arg entry (`express.e:11984-12021`). The resume point is the
    /// per-base read pointer plus one (`lastMsgReadConf + 1`, `:11984`,
    /// where `lastMsgReadConf := cb.confYM`, `:4912`), clamped up to the
    /// base's lowest key (`:11985`). This is the sequential read pointer,
    /// not the first unread message addressed to the reader.
    ///
    /// Unlike `R <num>`, bare `R` shows no message before the prompt: the
    /// `Msg. Options:` prompt renders at the resume range and the first
    /// `<CR>` then displays the resume message. When the resume point is
    /// past the highest existing message (the pointer is exhausted, or the
    /// base is empty) the prompt renders with the `( QUIT )` range and a
    /// `<CR>` / `Q` returns to the menu (legacy `:12012`).
    pub(super) async fn handle_read_mail_at_pointer(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let Some((conference, msgbase)) = session.current_msgbase() else {
            return self.write_and_flush(NO_MAIL_BASE_LINE).await;
        };
        let base = MessageBaseRef::new(conference, msgbase);

        // A never-read base has no pointer row; treat that as 0 so the
        // resume starts at message 1 (legacy `lastMsgReadConf` default).
        let last_read = session
            .user()
            .read_pointers_for(base)
            .map_or(0, ReadPointers::last_read);

        // Clamp UP to the base's lowest key. The trait exposes the lowest
        // *undeleted* message; this matches the legacy `mailStat.lowestKey`
        // except when the true lowest key is a soft-deleted message below
        // it.
        let lowest = match self.services.mail_stores().lock(base).await {
            Some(guard) => guard.lowest_undeleted_message(),
            None => return self.write_and_flush(NO_MAIL_BASE_LINE).await,
        };
        let start = last_read.saturating_add(1).max(lowest);

        // The legacy entry blank line (`express.e:11987`) precedes the
        // prompt-first loop; no message is displayed yet, so
        // `last_displayed` is `None`.
        self.write_and_flush(b"\r\n").await?;
        self.run_read_subprompt(session, start, None).await
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
