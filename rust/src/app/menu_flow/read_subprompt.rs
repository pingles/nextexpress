//! `R` read sub-prompt loop (Slice B4) — the legacy `readMSG` primary
//! mail-reading UI (`amiexpress/express.e:11972`).
//!
//! Once `handle_read_mail` has displayed a message, this loop renders
//! the `Msg. Options:` sub-prompt and reads the caller's choice.
//!
//! B4 ships the smallest surface: `<CR>` (an empty line) advances to
//! the next message in the base, and `Q` returns to the menu.
//! Advancing past the highest existing message returns to the menu
//! (the legacy out-of-range → `QUIT` clamp at `express.e:12012`). Every
//! other key simply re-renders the prompt — the `A` / `F` / `R` / `L` /
//! `D` / `M` / `EH` / `?` / `??` options accrete behind this loop in
//! Slice B5.

use std::time::SystemTime;

use crate::app::menu_command::NumberArg;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::render_read_subprompt;
use crate::domain::conference::MessageBaseRef;
use crate::domain::messaging::delete_mail::can_delete;
use crate::domain::messaging::edit_mail_header::can_edit_header;
use crate::domain::messaging::move_mail::can_move;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the read sub-prompt loop, starting from the message
    /// `start` that `handle_read_mail` has already displayed.
    pub(super) async fn run_read_subprompt(
        &mut self,
        session: &mut MenuSession,
        start: u32,
    ) -> Result<(), T::Error> {
        // The upper bound of the `<number>+<highest>` range string.
        // Stable for the duration of the read (no posting happens while
        // a reader holds the loop), so it is read once on entry.
        let Some(highest) = self.current_base_highest(session).await else {
            return Ok(());
        };

        let mut number = start;
        loop {
            // The `D` / `M` options appear only for callers permitted to
            // use them on the current message (legacy `checkSecurity`
            // gates at `express.e:12017-12018`), matching the dispatch
            // guards below so the prompt never advertises an option it
            // would then refuse.
            let show_delete = self.current_user_can_delete(session, number).await;
            let show_move = can_move(session.user());
            let prompt = render_read_subprompt(number, highest, show_delete, show_move);
            // A disconnected or idle caller leaves the sub-prompt; the
            // menu loop's next read applies carrier-loss / idle-timeout,
            // matching the other interactive handlers.
            let TerminalRead::Line(line) =
                self.read_prompted(&prompt, TerminalEcho::Visible).await?
            else {
                return Ok(());
            };
            session.record_input(SystemTime::now());
            let trimmed = line.trim();

            // Empty input (`<CR>`) walks forward to the next message.
            if trimmed.is_empty() {
                if !self.advance_or_quit(session, &mut number, highest).await? {
                    return Ok(());
                }
                continue;
            }

            // Dispatch on the first character, mirroring the legacy
            // `LowerChar(str[0])` SELECT (`express.e:12092-12224`).
            match trimmed.chars().next().map(|c| c.to_ascii_lowercase()) {
                // `Q`uit returns to the menu (`express.e:12226`).
                Some('q') => return Ok(()),
                // `A`gain re-displays the current message and stays on
                // it (`express.e:12102-12105`).
                Some('a') => {
                    self.read_and_render(session, number).await?;
                }
                // `R`eply posts a reply to the current message, then
                // advances to the next one (`express.e:12161-12168`).
                Some('r') => {
                    self.handle_reply(session, NumberArg::Number(number))
                        .await?;
                    if !self.advance_or_quit(session, &mut number, highest).await? {
                        return Ok(());
                    }
                }
                // `F`orward forwards the current message, then stays on
                // it (`express.e:12153-12160`).
                Some('f') => {
                    self.handle_forward(session, NumberArg::Number(number))
                        .await?;
                }
                // `D`elete: gated on the per-message delete permission
                // (legacy `ACS_DELETE_MESSAGE`, `express.e:12148`). When
                // permitted the confirm-and-delete runs and the loop
                // advances unconditionally — the legacy `goNextMsg`
                // fires whatever the confirm answer (`:12151`). A
                // caller without delete permission falls through (the
                // option is not theirs).
                Some('d') if self.current_user_can_delete(session, number).await => {
                    self.handle_kill(session, NumberArg::Number(number)).await?;
                    if !self.advance_or_quit(session, &mut number, highest).await? {
                        return Ok(());
                    }
                }
                // `M`ove: gated on the move permission (legacy
                // `ACS_SYSOP_READ`, `express.e:12170`). Advances only on
                // a successful move (`:12172`); an aborted or rejected
                // move stays on the current message.
                Some('m') if can_move(session.user()) => {
                    let moved = self
                        .handle_move_mail(session, NumberArg::Number(number))
                        .await?;
                    if moved && !self.advance_or_quit(session, &mut number, highest).await? {
                        return Ok(());
                    }
                }
                // `EH` edits the current header (the `E`-family option,
                // gated on edit-header access — legacy `ACS_MESSAGE_EDIT`,
                // `express.e:12179`), then re-displays the edited message
                // and stays (`displayMessage` -> `nextMenu`,
                // `:12191-12193`). Bare `E` (emacs) / `EM` (body edit)
                // are deliberately not carried (see slice B5 Out of
                // Scope).
                Some('e')
                    if trimmed.eq_ignore_ascii_case("eh") && can_edit_header(session.user()) =>
                {
                    self.handle_edit_header(session, NumberArg::Number(number))
                        .await?;
                    self.read_and_render(session, number).await?;
                }
                // Any other key is an unimplemented B5 option; fall
                // through and re-render the prompt.
                _ => {}
            }
        }
    }

    /// Advances `number` to the next message and displays it. Returns
    /// `Ok(false)` when there is no next message — the legacy
    /// out-of-range -> `QUIT` clamp (`express.e:12012`) — so the caller
    /// leaves the loop.
    async fn advance_or_quit(
        &mut self,
        session: &mut MenuSession,
        number: &mut u32,
        highest: u32,
    ) -> Result<bool, T::Error> {
        let next = *number + 1;
        if next > highest {
            return Ok(false);
        }
        *number = next;
        self.read_and_render(session, *number).await?;
        Ok(true)
    }

    /// Returns the highest existing message number in the session's
    /// current message base — the range string's upper bound, the
    /// legacy `mailStat.highMsgNum - 1` (`express.e:12010`). `None` when
    /// the session has no current base or no store is registered for it.
    async fn current_base_highest(&self, session: &MenuSession) -> Option<u32> {
        let (conference, msgbase) = session.current_msgbase()?;
        let guard = self
            .services
            .mail_stores()
            .lock(MessageBaseRef::new(conference, msgbase))
            .await?;
        Some(guard.highest_message())
    }

    /// True when the session's user may delete the current message
    /// (`number`). Loads the message and applies the `can_delete`
    /// predicate, releasing the store lock before returning so the
    /// delete handler can re-lock without self-deadlock. A missing
    /// base, an absent message or a load error all read as "not
    /// permitted".
    async fn current_user_can_delete(&self, session: &MenuSession, number: u32) -> bool {
        let Some((conference, msgbase)) = session.current_msgbase() else {
            return false;
        };
        let Some(guard) = self
            .services
            .mail_stores()
            .lock(MessageBaseRef::new(conference, msgbase))
            .await
        else {
            return false;
        };
        let permitted = match guard.load(number) {
            Ok(Some(mail)) => can_delete(session.user(), &mail),
            _ => false,
        };
        drop(guard);
        permitted
    }
}
