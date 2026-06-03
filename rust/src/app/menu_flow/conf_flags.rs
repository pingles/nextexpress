//! `CF` (conference flags editor) menu command — the legacy
//! `internalCommandCF` loop (`amiexpress/express.e:24672-24841`).
//!
//! Each turn redraws the M/A/F/Z listing, prompts for a mask key, then a
//! conference-selection expression, and applies the edit to the caller's
//! **own** memberships. A non-M/A/F/Z mask key (`:24759-24761`) or an
//! empty expression (`:24773`) returns to the menu.
//!
//! Divergence from the legacy: the mask is read as a full line (the
//! `NextExpress` terminal is line-based) rather than a single `readChar`
//! keystroke, so the user presses Enter after the mask letter. The wire
//! echo (`<key>\r\n`) is identical either way.

use std::time::SystemTime;

use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_conf_flags_listing, CONF_FLAGS_EXPR_PROMPT, CONF_FLAGS_MASK_PROMPT,
};
use crate::domain::conference_flags::{
    apply_scan_flag_edit, conf_flag_rows, parse_scan_flag_mask, parse_scan_flag_selection,
};
use crate::domain::session::typed::{BoundMenuUser, MenuSession};

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_conference_flags(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        loop {
            // Redraw the listing. The bytes are computed up-front so the
            // immutable user / conferences borrows are released before the
            // mutable terminal and session borrows below.
            let listing = {
                let rows =
                    conf_flag_rows(session.user().memberships(), self.services.conferences());
                render_conf_flags_listing(&rows)
            };
            self.terminal.write(&listing).await?;

            // Mask key. A disconnected / idle caller, or a non-M/A/F/Z key
            // (including a bare `<CR>`), leaves the editor for the menu.
            let TerminalRead::Line(mask_line) = self
                .read_prompted(CONF_FLAGS_MASK_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(());
            };
            session.record_input(SystemTime::now());
            let Some(flag) = parse_scan_flag_mask(&mask_line) else {
                return Ok(());
            };

            // Conference-selection expression. An empty line returns to the
            // menu (legacy `StrLen(confNums)=0 -> RETURN`, `:24773`).
            let TerminalRead::Line(expr_line) = self
                .read_prompted(CONF_FLAGS_EXPR_PROMPT, TerminalEcho::Visible)
                .await?
            else {
                return Ok(());
            };
            session.record_input(SystemTime::now());
            let Some(selection) = parse_scan_flag_selection(&expr_line) else {
                return Ok(());
            };

            apply_scan_flag_edit(session.user_mut().memberships_mut(), flag, &selection);
        }
    }
}
