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

use crate::app::menu_flow::table::left_field;
use crate::app::terminal::{Terminal, TerminalEcho};
use crate::domain::conference_flags::{
    apply_scan_flag_edit, conf_flag_rows, parse_scan_flag_mask, parse_scan_flag_selection,
};
use crate::domain::session::typed::MenuSession;

/// Clears the screen for the `CF` listing: the legacy `sendCLS()` (a
/// single form-feed byte, `amiexpress/express.e:5224`) then the leading
/// CRLF at `:24690`.
const CONF_FLAGS_CLEAR: &[u8] = b"\x0c\r\n";

/// The `CF` listing column header (`amiexpress/express.e:24691`).
const CONF_FLAGS_HEADER: &[u8] =
    b"\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n";

/// The `CF` listing ruler row, with its trailing blank line
/// (`amiexpress/express.e:24692`, whose literal ends `[0m\b\n\b\n`).
const CONF_FLAGS_RULER: &[u8] =
    b"\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n";

/// The `CF` mask-selection prompt, with its two leading blank lines
/// (`amiexpress/express.e:24749`).
const CONF_FLAGS_MASK_PROMPT: &[u8] =
    b"\r\n\r\nEdit which flags [M]ailScan, [A]ll Messages, [F]ileScan, [Z]oom >: ";

/// The `CF` conference-selection prompt (`amiexpress/express.e:24769`).
const CONF_FLAGS_EXPR_PROMPT: &[u8] =
    b"Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on >: ";

/// Renders the full `CF` (conference flags) listing screen — the legacy
/// `internalCommandCF` redraw (`amiexpress/express.e:24689-24747`): a
/// clear-screen, the M/A/F/Z column header and ruler, then one
/// `[ n] M A F Z <name>` cell per accessible conference, two per line.
///
/// The caller emits [`CONF_FLAGS_MASK_PROMPT`] after this. A set flag
/// renders as `*`, a clear flag as a space; the cell order is
/// `M A F Z` = mail / all-messages / file / zoom (`:24732`/`:24735`).
fn render_conf_flags_listing(rows: &[crate::domain::conference_flags::ConfFlagRow]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        CONF_FLAGS_CLEAR.len() + CONF_FLAGS_HEADER.len() + CONF_FLAGS_RULER.len() + rows.len() * 80,
    );
    out.extend_from_slice(CONF_FLAGS_CLEAR);
    out.extend_from_slice(CONF_FLAGS_HEADER);
    out.extend_from_slice(CONF_FLAGS_RULER);
    for (index, row) in rows.iter().enumerate() {
        out.extend_from_slice(b"\x1b[34m[\x1b[0m");
        out.extend_from_slice(format!("{:5}", row.conference_number).as_bytes());
        out.extend_from_slice(b"\x1b[34m] \x1b[36m");
        out.push(conf_flag_glyph(row.mail_scan));
        out.push(b' ');
        out.push(conf_flag_glyph(row.mailscan_all));
        out.push(b' ');
        out.push(conf_flag_glyph(row.file_scan));
        out.push(b' ');
        out.push(conf_flag_glyph(row.zoom_scan));
        out.push(b' ');
        out.extend_from_slice(b"\x1b[0m");
        out.extend_from_slice(left_field(&row.conference_name, 23).as_bytes());
        // Two conferences per line: even cells get a separating space,
        // odd cells close the line (legacy `IF n AND 1`, `:24739`).
        if index % 2 == 1 {
            out.extend_from_slice(b"\r\n");
        } else {
            out.push(b' ');
        }
    }
    out
}

/// `*` for a set scan flag, a space for a clear one (the legacy `c1..c4`
/// glyphs at `amiexpress/express.e:24704-24726`, restricted to the
/// `*`/space cases — the `F`/`D` tooltype overrides are out of scope).
fn conf_flag_glyph(on: bool) -> u8 {
    if on {
        b'*'
    } else {
        b' '
    }
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_conference_flags(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        loop {
            // Redraw the listing. The bytes are computed up-front so the
            // immutable user / conferences borrows are released before the
            // mutable terminal and session borrows below.
            let listing = {
                let rows = conf_flag_rows(
                    session.user().memberships(),
                    self.services.conferences.as_ref(),
                );
                render_conf_flags_listing(&rows)
            };
            self.terminal.write(&listing).await?;

            // Mask key. A non-M/A/F/Z key (including a bare `<CR>`) leaves
            // the editor for the menu; EOF/idle exits the session.
            let mask_line = self
                .read_prompted(CONF_FLAGS_MASK_PROMPT, TerminalEcho::Visible)
                .await?;
            session.record_input(self.services.clock.now());
            let Some(flag) = parse_scan_flag_mask(&mask_line) else {
                return Ok(());
            };

            // Conference-selection expression. An empty line returns to the
            // menu (legacy `StrLen(confNums)=0 -> RETURN`, `:24773`).
            let expr_line = self
                .read_prompted(CONF_FLAGS_EXPR_PROMPT, TerminalEcho::Visible)
                .await?;
            session.record_input(self.services.clock.now());
            let Some(selection) = parse_scan_flag_selection(&expr_line) else {
                return Ok(());
            };

            apply_scan_flag_edit(session.user_mut().memberships_mut(), flag, &selection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::fn_params_excessive_bools)] // one bool per M/A/F/Z cell
    fn cf_row(
        number: u32,
        name: &str,
        mail_scan: bool,
        mailscan_all: bool,
        file_scan: bool,
        zoom_scan: bool,
    ) -> crate::domain::conference_flags::ConfFlagRow {
        crate::domain::conference_flags::ConfFlagRow {
            conference_number: number,
            conference_name: name.to_string(),
            mail_scan,
            mailscan_all,
            file_scan,
            zoom_scan,
        }
    }

    #[test]
    fn render_conf_flags_listing_matches_the_legacy_all_off_capture() {
        // Byte-for-byte against the FS-UAE reference capture (two
        // single-base conferences, every flag clear).
        let rows = vec![
            cf_row(1, "New Users", false, false, false, false),
            cf_row(2, "Amiga", false, false, false, false),
        ];
        let expected: &[u8] = b"\x0c\r\n\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n\x1b[34m[\x1b[0m    1\x1b[34m] \x1b[36m        \x1b[0mNew Users               \x1b[34m[\x1b[0m    2\x1b[34m] \x1b[36m        \x1b[0mAmiga                  \r\n";
        assert_eq!(render_conf_flags_listing(&rows), expected);
    }

    #[test]
    fn render_conf_flags_listing_marks_a_set_flag_with_a_star() {
        // Conference 2's mail-scan set: the M cell becomes `*` (the
        // legacy post-toggle capture). The two-per-line layout also
        // pins the even-cell separating space and the odd-cell CRLF.
        let rows = vec![
            cf_row(1, "New Users", false, false, false, false),
            cf_row(2, "Amiga", true, false, false, false),
        ];
        let expected: &[u8] = b"\x0c\r\n\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n\r\n\x1b[34m[\x1b[0m    1\x1b[34m] \x1b[36m        \x1b[0mNew Users               \x1b[34m[\x1b[0m    2\x1b[34m] \x1b[36m*       \x1b[0mAmiga                  \r\n";
        assert_eq!(render_conf_flags_listing(&rows), expected);
    }

    #[test]
    fn cf_prompts_match_the_legacy_captures() {
        assert_eq!(
            CONF_FLAGS_MASK_PROMPT,
            b"\r\n\r\nEdit which flags [M]ailScan, [A]ll Messages, [F]ileScan, [Z]oom >: "
        );
        assert_eq!(
            CONF_FLAGS_EXPR_PROMPT,
            b"Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on >: "
        );
    }
}
