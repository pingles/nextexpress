//! The `N` command — the `NextScan` new-files scan (slice D9).
//!
//! Parity target: the `AquaScan` v1.0 door experience with `NextScan`
//! branding, pinned to the live capture
//! `comparison/transcripts/ae_tierd_newfiles.txt` (two full logon
//! rounds; section labels N1–N9 cited throughout). The shadowed
//! internal `internalCommandN` (`amiexpress/express.e:25275`, the
//! looping `Date as (mm-dd-yy)…` prompt) is the stock diff record
//! only. Menu `N` scans the **current conference only** and never
//! consults the `CF` file-scan flag — both capture-proven (N9 shows
//! `(1-1)` in a one-area conference; `express.e:591-608`/`:28089`
//! gate only the logon `confScan`).

use std::time::SystemTime;

use super::dir_row;
use super::scan::{DirectoryOrder, ScanFlow, ScanKind, ScanLine, ScanMode, ScanState};
use super::wire;
use crate::app::menu_command::{FileSpan, NewFilesArg, NewFilesSpec, ScanRequest};
use crate::app::menu_flow::{AbortNotice, EmptyMeaning, PromptLine};
use crate::app::terminal::Terminal;
use crate::app::terminal::CRLF;
use crate::domain::files::area::FileArea;
use crate::domain::session::typed::MenuSession;

impl<T> crate::app::menu_flow::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the `N` menu command — the `NextScan` new-files scan.
    ///
    /// # Parameters
    /// - `session`: the live menu session — supplies the current
    ///   conference, the user's `last_call` (the default scan date)
    ///   and the flagged-file set the pager's flag verbs mutate.
    /// - `arg`: the parsed [`NewFilesArg`].
    ///
    /// # Errors
    /// Propagates the terminal's write/read error.
    pub(in crate::app::menu_flow) async fn handle_new_files(
        &mut self,
        session: &mut MenuSession,
        arg: NewFilesArg,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        match arg {
            NewFilesArg::Invalid => self.new_files_argument_error().await,
            NewFilesArg::Help => {
                // `N ?` (capture N6, rebranded).
                self.terminal
                    .write(wire::NEW_FILES_HELP_SCREEN.as_bytes())
                    .await?;
                Ok(self.terminal.flush().await?)
            }
            NewFilesArg::Prompt => self.new_files_prompt(session).await,
            NewFilesArg::Scan(spec) => self.new_files_scan(session, spec).await,
        }
    }

    /// `Argument error! Type 'n ?' for help.` under the help banner —
    /// F's argument-error envelope with the `'n ?'` text (capture N7e,
    /// `N R -1`; single-reset tail).
    async fn new_files_argument_error(
        &mut self,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        self.terminal.write(b"\x1b[0m\r\n").await?;
        self.terminal.write(wire::HELP_BANNER.as_bytes()).await?;
        self.terminal.write(b"\r\n\r\n").await?;
        self.terminal.write(wire::NEW_FILES_ARGUMENT_ERROR).await?;
        self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
        Ok(self.terminal.flush().await?)
    }

    /// Bare `N`: banner, the door's `Date: …` prompt, then its
    /// `Directories: …` prompt (byte-identical to bare `F`'s), then
    /// the scan (captures N1a/N1b/N2).
    ///
    /// The preamble is written **raw, not page-counted**: the door
    /// resets its pager counter at each interactive prompt — the
    /// captured page-1 More? of the prompt path fires exactly 29
    /// counted lines after the post-answer blank (N2), preamble
    /// excluded. The inline path counts its preamble instead (N7c);
    /// the two boundaries are deliberately different models, each
    /// carrying its own pin.
    async fn new_files_prompt(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);

        // Raw (uncounted) preamble: reset line, banner, blank (N1a).
        self.terminal.write(b"\x1b[0m\r\n").await?;
        self.terminal.write(wire::NEW_FILES_BANNER).await?;
        self.terminal.write(b"\r\n\r\n").await?;

        // The prompt advertises the day of the previous call as its
        // Enter default (capture-proven: pass 1 showed `06-25-26`
        // while "today" was 07-03); a caller with no prior call gets
        // today (a NextExpress choice — TO-CONFIRM #12).
        let now = self.services.clock.now();
        let last_call = session.user().last_call();
        let default_label = dir_row::format_dir_date(last_call.unwrap_or(now));
        let request = match self
            .prompt_line(
                session,
                &wire::date_prompt(&default_label),
                EmptyMeaning::Keep,
                AbortNotice::Silent,
            )
            .await?
        {
            PromptLine::Kept => ScanRequest::SinceLastCall,
            PromptLine::Aborted => return Ok(self.terminal.flush().await?),
            PromptLine::Entered(answer) => match parse_date_answer(&answer) {
                Some(request) => request,
                None => return self.error_in_date().await,
            },
        };
        // Resolve before the Directories prompt: the captured
        // junk-date error (N5) fires straight after the date answer. A
        // calendar-invalid but date-shaped answer takes the same
        // envelope (provisional — TO-CONFIRM #7).
        let Some(kind) = resolve_request(request, now, last_call) else {
            return self.error_in_date().await;
        };

        let answer = match self
            .prompt_line(
                session,
                &wire::directories_prompt(max),
                EmptyMeaning::Verbatim,
                AbortNotice::Silent,
            )
            .await?
        {
            PromptLine::Entered(answer) => answer,
            PromptLine::Aborted => return Ok(self.terminal.flush().await?),
            PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
        };
        if answer.is_empty() {
            // Enter = None: blank + a single reset — F's abort tail,
            // byte-identical in the N capture (N1a).
            self.terminal.write(b"\r\n\x1b[0m\r\n").await?;
            return Ok(self.terminal.flush().await?);
        }
        let Some(span) = crate::app::menu_command::parse_span_token(&answer) else {
            // Junk at the Directories prompt: F's `Error in input!`
            // envelope (same door machinery, byte-identical prompt —
            // provisional, TO-CONFIRM #3).
            self.terminal.write(CRLF).await?;
            self.terminal.write(wire::ERROR_IN_INPUT).await?;
            self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
            return Ok(self.terminal.flush().await?);
        };

        // Both prompts have stamped `record_input`, so the flag set
        // can be borrowed for the whole scan. The post-answer blank is
        // COUNTED — it opens the 29-line page-1 window the capture
        // pins (N2: More? after exactly 29 lines from this blank).
        let flagged = session.flagged_files_mut();
        let mut state = ScanState::new(false, conference);
        let directory_order = match &kind {
            ScanKind::Full { reverse: true } => DirectoryOrder::Reverse,
            _ => DirectoryOrder::Forward,
        };
        let mode = ScanMode {
            kind,
            directory_order,
            quick: false,
        };
        if self
            .emit_scan_line(&mut state, ScanLine::raw(Vec::new()), flagged)
            .await?
            == ScanFlow::Quit
        {
            return self.finish_listing().await;
        }
        self.run_span(&mut state, conference, span, &areas, flagged, &mode)
            .await
    }

    /// Inline `N …` forms (captures N7a–N7r): resolve the request,
    /// default the span to the Upload dir (N7a), feed the preamble
    /// COUNTED through the pager (N7c's 29-from-reset page-1 pin), and
    /// scan.
    async fn new_files_scan(
        &mut self,
        session: &mut MenuSession,
        spec: NewFilesSpec,
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.areas_in_conference(conference);
        let now = self.services.clock.now();
        let last_call = session.user().last_call();
        let Some(kind) = resolve_request(spec.request, now, last_call) else {
            // A parseable-but-unresolvable date (e.g. `N 13-40-26`)
            // takes the argument-error envelope, provisionally
            // (TO-CONFIRM #7 — every captured bad inline form did).
            return self.new_files_argument_error().await;
        };
        let span = spec.span.unwrap_or(FileSpan::Upload);
        let flagged = session.flagged_files_mut();
        let mut state = ScanState::new(spec.non_stop, conference);
        let directory_order = match &kind {
            ScanKind::Full { reverse: true } => DirectoryOrder::Reverse,
            _ => DirectoryOrder::Forward,
        };
        let mode = ScanMode {
            kind,
            directory_order,
            quick: spec.quick,
        };
        if self
            .begin_listing(&mut state, wire::NEW_FILES_BANNER, flagged)
            .await?
            == ScanFlow::Quit
        {
            return self.finish_listing().await;
        }
        self.run_span(&mut state, conference, span, &areas, flagged, &mode)
            .await
    }

    /// The single-shot `Error in date!` envelope (capture N5): blank,
    /// the literal, blank, one reset — back to the menu. The internal
    /// command's *looping* date prompt is the shadowed stock path
    /// (diff record only).
    async fn error_in_date(&mut self) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        self.terminal.write(CRLF).await?;
        self.terminal.write(wire::ERROR_IN_DATE).await?;
        self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
        Ok(self.terminal.flush().await?)
    }
}

/// Parses a non-empty `Date:` prompt answer. The 2026-07-04 re-probe
/// (`ae_tierd_newfiles2.txt`/`ae_tierd_newfiles3.txt`) closed
/// TO-CONFIRM #2/#11: the door accepts `mm-dd[-yy]`, `-x`, `R`, `T`,
/// `S` and `!x` here — everything after a recognised first token is
/// tolerated and discarded (captured for `R 12-30-26`/`R 01-01-26`,
/// N4b, and `01-01-26 X`, P11f) — while `Y` alone is rejected (P2y),
/// even though inline `N Y` scans since yesterday (N7y). `None` is the
/// `Error in date!` path.
fn parse_date_answer(answer: &str) -> Option<ScanRequest> {
    let first = answer.split_ascii_whitespace().next()?;
    if first.eq_ignore_ascii_case("R") {
        return Some(ScanRequest::Reverse);
    }
    if first.eq_ignore_ascii_case("T") {
        return Some(ScanRequest::Today);
    }
    if first.eq_ignore_ascii_case("S") {
        return Some(ScanRequest::SinceLastCall);
    }
    crate::app::menu_command::parse_days_back(first)
        .map(ScanRequest::DaysBack)
        .or_else(|| {
            crate::app::menu_command::parse_newest_count(first).map(ScanRequest::NewestLast)
        })
        .or_else(|| crate::app::menu_command::parse_date_token(first))
}

/// Resolves a [`ScanRequest`] to the engine [`ScanKind`], with all day
/// arithmetic in **UTC** (the `dir_row` rendering precedent; recorded
/// in `COMMAND_PARITY.md`). The `NewSince` cutoff is UTC midnight of the
/// target day — the filter is inclusive (`uploaded_at >= cutoff`,
/// `express.e:27976-27986` `ddt>=day`). `None` when the request cannot
/// resolve to a real date (calendar-invalid, or day arithmetic out of
/// range).
///
/// Date sources: `SinceLastCall` (and the prompt's Enter default) uses
/// the day of `last_call`, falling back to today for a first-time
/// caller (TO-CONFIRM #12); `Today`/`Yesterday`/`DaysBack` count from
/// today; an explicit year pivots `yy > 77 → 19yy else 20yy`
/// (`axconsts.e:41` TWODIGITYEARSWITCHOVER, `MiscFuncs.e:400`), and an
/// omitted year is the current year (TO-CONFIRM #6).
fn resolve_request(
    request: ScanRequest,
    now: SystemTime,
    last_call: Option<SystemTime>,
) -> Option<ScanKind> {
    let today = || time::OffsetDateTime::from(now).date();
    let date = match request {
        ScanRequest::Reverse => return Some(ScanKind::Full { reverse: true }),
        ScanRequest::NewestLast(count) => return Some(ScanKind::NewestLast { count }),
        ScanRequest::SinceLastCall => time::OffsetDateTime::from(last_call.unwrap_or(now)).date(),
        ScanRequest::Today => today(),
        ScanRequest::Yesterday => today().checked_sub(time::Duration::days(1))?,
        ScanRequest::DaysBack(days) => {
            today().checked_sub(time::Duration::days(i64::from(days)))?
        }
        ScanRequest::Date { month, day, year } => {
            let year = match year {
                Some(yy) => i32::from(pivot_year(yy)),
                None => today().year(),
            };
            let month = time::Month::try_from(month).ok()?;
            time::Date::from_calendar_date(year, month, day).ok()?
        }
    };
    let cutoff = SystemTime::from(date.midnight().assume_utc());
    Some(ScanKind::NewSince {
        cutoff,
        label: dir_row::format_dir_date(cutoff),
    })
}

/// The legacy two-digit-year pivot (`axconsts.e:41`): `yy > 77` reads
/// as 19yy, otherwise 20yy.
fn pivot_year(yy: u16) -> u16 {
    if yy > 77 {
        1900 + yy
    } else {
        2000 + yy
    }
}

#[cfg(test)]
mod tests;
