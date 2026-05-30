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
            let prompt = render_read_subprompt(number, highest);
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
}
