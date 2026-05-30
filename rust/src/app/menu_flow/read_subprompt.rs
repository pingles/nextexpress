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
            match self.read_prompted(&prompt, TerminalEcho::Visible).await? {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("Q") {
                        return Ok(());
                    }
                    if trimmed.is_empty() {
                        let next = number + 1;
                        if next > highest {
                            // No further messages — legacy clamps the
                            // range to `QUIT` and leaves the loop.
                            return Ok(());
                        }
                        number = next;
                        self.read_and_render(session, number).await?;
                    }
                    // Any other key is an unimplemented B5 option; fall
                    // through and re-render the prompt.
                }
                // A disconnected or idle caller leaves the sub-prompt;
                // the menu loop's next read applies carrier-loss /
                // idle-timeout, matching the other interactive handlers.
                TerminalRead::Eof | TerminalRead::IdleTimedOut => return Ok(()),
            }
        }
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
