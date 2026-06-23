//! `MS` (multi-conference mail scan) menu command (Tier B, Slice B1).
//!
//! The terminal-free walk over accessible conferences lives in the
//! [`core`] submodule; this module renders the legacy `searchNewMail`
//! multi-conference output: the `Scanning conferences for mail...`
//! header, a per-conference banner, and per-base either the
//! `Type/From/Subject/Msg` listing table, a `No mail today!` line, or
//! (for a base with messages but none addressed to the caller) just
//! the banner — mirroring `amiexpress/express.e:25250` and
//! `:11668-11739`.

mod core;

pub(super) use self::core::ScanFilter;

use std::time::SystemTime;

use self::core::{scan_all_mail, BaseScanOutcome};
use crate::app::menu_flow::mail_text::MAIL_STORE_ERROR_LINE;
use crate::app::menu_flow::table::{left_field, scan_row_status};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::domain::messaging::scan_mail::MailScanRow;
use crate::domain::session::typed::MenuSession;

/// The `MS` command's opening banner (`amiexpress/express.e:25258`,
/// `'\b\nScanning conferences for mail...\b\n\b\n'`, Amiga `\b\n`
/// translated to telnet `\r\n`).
const MAIL_SCAN_ALL_HEADER: &[u8] = b"\r\nScanning conferences for mail...\r\n\r\n";

/// Printed in place of a listing when a conference has no new mail
/// since the user's last scan (`amiexpress/express.e:11689`, the
/// `currentConf=0` branch).
const MAIL_SCAN_NO_MAIL_TODAY: &[u8] = b"No mail today!\r\n";

/// The read-it-now prompt the multi-conference scan shows after a base
/// whose listing matched unread mail (`amiexpress/express.e:11739`'s
/// `'\b\nWould you like to read it now '` followed by `yesNo(1)`'s
/// default-Yes `(Y/n)?` render at `:2136`). On a `y`/CR the caller is
/// dropped into the read/reply sub-prompt for the found message.
const MAIL_SCAN_READ_IT_NOW_PROMPT: &[u8] =
    b"\r\nWould you like to read it now \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[32m?\x1b[0m ";

/// Renders the per-conference banner the multi-conference scan prints
/// before each conference's listing (`amiexpress/express.e:11670`).
/// No trailing newline — the message-base sub-line or the listing
/// table that follows supplies the break.
fn render_scan_conference_banner(conference_name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(48 + conference_name.len());
    out.extend_from_slice(b"\x1b[32mScanning Conference\x1b[33m: \x1b[0m");
    out.extend_from_slice(conference_name.as_bytes());
    out.extend_from_slice(b" - ");
    out
}

/// Renders the message-base sub-line of the scan banner
/// (`amiexpress/express.e:11676-11677`). Empty when the base is
/// unnamed (mirrors `IF StrLen(msgBaseName)>0`). A leading CRLF is
/// emitted only for a conference's first base (`IF msgBaseNum=1`).
fn render_scan_msgbase_banner(msgbase_name: &str, first_base: bool) -> Vec<u8> {
    if msgbase_name.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(48 + msgbase_name.len());
    if first_base {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b" \x1b[32mMessage Base\x1b[33m: \x1b[0m");
    out.extend_from_slice(msgbase_name.as_bytes());
    out.extend_from_slice(b" - ");
    out
}

/// Renders the legacy mail-scan listing table for one base
/// (`amiexpress/express.e:11712-11721`): two leading CRLFs, three
/// header writes (green column titles, yellow dash separator, a
/// standalone colour reset), then one row per unread message. Returns
/// an empty buffer for an empty `rows` slice — the legacy only paints
/// the header once it has matched a first message (the `mailFlag`
/// gate at `:11710`).
///
/// Each row is `status(7)  from(29)  subject(21)  <reset>msg(6)` with
/// the over-wide `from` / `subject` fields truncated to their columns,
/// matching `AmigaE` `StringF`'s `\s[n]` behaviour (`:11720`).
fn render_scan_listing_table(rows: &[MailScanRow]) -> Vec<u8> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(160 + rows.len() * 72);
    out.extend_from_slice(b"\r\n\r\n");
    out.extend_from_slice(
        b"\x1b[32mType     From                           Subject                Msg    \r\n",
    );
    out.extend_from_slice(
        b"\x1b[33m-------  -----------------------------  ---------------------  -------\r\n",
    );
    out.extend_from_slice(b"\x1b[0m");
    for entry in rows {
        out.extend_from_slice(scan_row_status(entry.visibility).as_bytes());
        out.extend_from_slice(b"  ");
        out.extend_from_slice(left_field(&entry.from_name, 29).as_bytes());
        out.extend_from_slice(b"  ");
        out.extend_from_slice(left_field(&entry.subject, 21).as_bytes());
        out.extend_from_slice(b"  \x1b[0m");
        out.extend_from_slice(format!("{:06}", entry.number).as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    pub(super) async fn handle_scan_all_mail(
        &mut self,
        session: &mut MenuSession,
        filter: ScanFilter,
    ) -> Result<(), T::Error> {
        let scans = scan_all_mail(
            session,
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            filter,
            SystemTime::now(),
        )
        .await;

        // The caller's home conference/base, restored after every
        // read-it-now detour so the menu prompt that follows `MS` shows
        // where the caller actually is (`amiexpress/express.e:25270`).
        let home = session.current_msgbase();

        self.write_and_flush(MAIL_SCAN_ALL_HEADER).await?;
        for conference in &scans {
            self.write_and_flush(&render_scan_conference_banner(&conference.conference_name))
                .await?;
            for base in &conference.bases {
                self.write_and_flush(&render_scan_msgbase_banner(
                    &base.msgbase_name,
                    base.first_base,
                ))
                .await?;
                match &base.outcome {
                    // Matched unread mail: the column table, then the
                    // legacy read-it-now offer.
                    BaseScanOutcome::Listing(rows) => {
                        self.write_and_flush(&render_scan_listing_table(rows))
                            .await?;
                        self.offer_read_it_now(session, rows, home).await?;
                    }
                    // Nothing new since the user's last scan.
                    BaseScanOutcome::NothingNew => {
                        self.write_and_flush(MAIL_SCAN_NO_MAIL_TODAY).await?;
                    }
                    // Messages exist but none addressed to the caller, or
                    // no store at all: the legacy prints only the banner.
                    BaseScanOutcome::NoMatch | BaseScanOutcome::NoStore => {}
                    BaseScanOutcome::Error(err) => {
                        eprintln!("scan_all_mail base scan failed: {err}");
                        self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Asks `Would you like to read it now ` for a base whose listing
    /// matched unread mail and, on Yes, drops into the read/reply
    /// sub-prompt for the found messages — the legacy `searchNewMail`
    /// getOUT branch (`amiexpress/express.e:11738-11765`). The found
    /// base is attached as a transient read visit and the caller's
    /// `home` coordinate restored afterwards, mirroring the legacy
    /// `currentConf:=cn ... :=oldcn`.
    async fn offer_read_it_now(
        &mut self,
        session: &mut MenuSession,
        rows: &[MailScanRow],
        home: Option<(u32, u32)>,
    ) -> Result<(), T::Error> {
        let Some(first) = rows.first() else {
            return Ok(());
        };
        if !self.prompt_read_it_now(session).await? {
            return Ok(());
        }
        let coord = first.msgbase;
        let start = first.number;
        session.attach_read_visit(
            coord.conference_number(),
            coord.msgbase_number(),
            SystemTime::now(),
        );
        if self.read_and_render(session, start).await? {
            self.run_read_subprompt(session, start + 1, Some(start))
                .await?;
        }
        if let Some((conference, msgbase)) = home {
            session.attach_read_visit(conference, msgbase, SystemTime::now());
        }
        Ok(())
    }

    /// Reads the read-it-now answer, mirroring the legacy `yesNo(1)`
    /// default-Yes semantics: an empty line / `y` accepts, only an
    /// explicit `n`/`N` declines. EOF or idle declines without reading.
    /// A trailing CRLF follows the answer (the legacy `aePuts('\b\n')`
    /// at `amiexpress/express.e:11741`).
    async fn prompt_read_it_now(&mut self, session: &mut MenuSession) -> Result<bool, T::Error> {
        match self
            .read_prompted(MAIL_SCAN_READ_IT_NOW_PROMPT, TerminalEcho::Visible)
            .await?
        {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                self.write_newline().await?;
                Ok(!matches!(line.trim().chars().next(), Some('n' | 'N')))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::conference::MessageBaseRef;
    use crate::domain::messaging::mail::{BroadcastTo, MailVisibility};

    fn row(number: u32, visibility: MailVisibility, from: &str, subject: &str) -> MailScanRow {
        MailScanRow {
            msgbase: MessageBaseRef::new(2, 1),
            number,
            visibility,
            from_name: from.to_string(),
            to_name: "bob".to_string(),
            broadcast_to: BroadcastTo::None,
            subject: subject.to_string(),
        }
    }

    #[test]
    fn scan_all_header_is_the_legacy_scanning_conferences_banner() {
        // amiexpress/express.e:25258 — '\b\nScanning conferences for mail...\b\n\b\n'.
        assert_eq!(
            MAIL_SCAN_ALL_HEADER,
            b"\r\nScanning conferences for mail...\r\n\r\n"
        );
    }

    #[test]
    fn renders_conference_banner_with_legacy_ansi() {
        // amiexpress/express.e:11670.
        assert_eq!(
            render_scan_conference_banner("General"),
            b"\x1b[32mScanning Conference\x1b[33m: \x1b[0mGeneral - ".to_vec()
        );
    }

    #[test]
    fn renders_msgbase_banner_for_named_first_base() {
        // amiexpress/express.e:11676-11677 — leading CRLF only on the
        // conference's first base.
        assert_eq!(
            render_scan_msgbase_banner("main", true),
            b"\r\n \x1b[32mMessage Base\x1b[33m: \x1b[0mmain - ".to_vec()
        );
    }

    #[test]
    fn renders_msgbase_banner_without_leading_newline_for_later_bases() {
        assert_eq!(
            render_scan_msgbase_banner("tech", false),
            b" \x1b[32mMessage Base\x1b[33m: \x1b[0mtech - ".to_vec()
        );
    }

    #[test]
    fn msgbase_banner_is_empty_for_unnamed_base() {
        // Mirrors `IF StrLen(msgBaseName)>0` — no sub-line for an unnamed base.
        assert!(render_scan_msgbase_banner("", true).is_empty());
        assert!(render_scan_msgbase_banner("", false).is_empty());
    }

    #[test]
    fn no_mail_today_line_is_verbatim() {
        // amiexpress/express.e:11689 (currentConf=0 branch).
        assert_eq!(MAIL_SCAN_NO_MAIL_TODAY, b"No mail today!\r\n");
    }

    #[test]
    fn renders_listing_table_header_and_rows() {
        // amiexpress/express.e:11712-11721: leading CRLFx2, three header
        // writes (green columns / yellow dashes / standalone reset),
        // then one row per unread mail.
        let rows = vec![
            row(1, MailVisibility::Public, "alice", "hello"),
            row(42, MailVisibility::Private, "carol", "re: hello"),
        ];
        let expected = b"\r\n\r\n\x1b[32mType     From                           Subject                Msg    \r\n\x1b[33m-------  -----------------------------  ---------------------  -------\r\n\x1b[0mPublic   alice                          hello                  \x1b[0m000001\r\nPrivate  carol                          re: hello              \x1b[0m000042\r\n".to_vec();
        assert_eq!(render_scan_listing_table(&rows), expected);
    }

    #[test]
    fn listing_table_truncates_overlong_fields_to_their_columns() {
        // AmigaE StringF `\s[n]` truncates to the field width; the
        // legacy table relies on this to keep columns aligned.
        let long_from = "abcdefghijklmnopqrstuvwxyz0123456789"; // 36 chars > 29
        let long_subject = "this subject is far too long to fit"; // > 21
        let rows = vec![row(7, MailVisibility::Public, long_from, long_subject)];
        let out = render_scan_listing_table(&rows);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("abcdefghijklmnopqrstuvwxyz012  ")); // 29 chars then 2 spaces
        assert!(!text.contains("3456789")); // tail dropped
        assert!(text.contains("this subject is far t  ")); // 21 chars then 2 spaces
        assert!(text.ends_with("\x1b[0m000007\r\n"));
    }

    #[test]
    fn listing_table_is_empty_when_there_are_no_rows() {
        // The legacy only emits the header once the first match is found
        // (`mailFlag` gate); zero rows print nothing.
        assert!(render_scan_listing_table(&[]).is_empty());
    }

    #[test]
    fn listing_row_status_is_private_for_private_to_sysop() {
        let rows = vec![row(1, MailVisibility::PrivateToSysop, "x", "y")];
        let text = String::from_utf8(render_scan_listing_table(&rows)).unwrap();
        assert!(text.contains("\x1b[0mPrivate  x"));
    }
}
