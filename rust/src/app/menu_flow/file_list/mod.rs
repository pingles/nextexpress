//! The `F` command — `NextScan` file listings (slice D2).
//!
//! Parity target: the `AquaScan` v1.0 door experience with `NextScan`
//! branding (`comparison/evidence-tierD/live-observations.md`;
//! cleanest captures in `comparison/transcripts/ae_tierd_aquascan3.txt`).
//! The shadowed internal `internalCommandF`
//! (`amiexpress/express.e:24877`) is the stock diff record only.

mod dir_row;
mod wire;

use crate::app::menu_command::{FileListArg, FileSpan, ZippyArg};
use crate::app::terminal::{KeyEvent, KeyRead, Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::CRLF;
use crate::domain::files::area::FileArea;
use crate::domain::files::file::File;
use crate::domain::session::typed::MenuSession;

/// Whether a paged listing keeps streaming or the user quit out.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ScanFlow {
    Continue,
    Quit,
}

/// The two-reset tail every listing-shaped exit emits before the menu
/// prompt (`ae_tierd_aquascan3.txt:163`; per-path tails are a pinned
/// asymmetry — aborts and argument errors emit one reset only).
const LISTING_EXIT_TAIL: &[u8] = b"\x1b[0m\r\n\x1b[0m\r\n";

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the `F` menu command — the `NextScan` lister.
    pub(super) async fn handle_file_list(
        &mut self,
        session: &mut MenuSession,
        arg: FileListArg,
    ) -> Result<(), T::Error> {
        match arg {
            FileListArg::Invalid => self.file_list_argument_error().await,
            FileListArg::Span {
                span,
                non_stop,
                reverse,
            } => self.file_list_span(session, span, non_stop, reverse).await,
            FileListArg::Prompt { reverse } => self.file_list_prompt(session, reverse).await,
            FileListArg::Help => {
                // `F ?` (`ae_tierd_aquascan3.txt` S1).
                self.terminal.write(wire::HELP_SCREEN.as_bytes()).await?;
                self.terminal.flush().await
            }
        }
    }

    /// Bare `F` / bare `FR`: the door's own
    /// `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=None ? `
    /// line prompt (`ae_tierd_aquascan3.txt:163`; Visible read — the
    /// answer echo is the adapter's). Enter aborts silently; junk
    /// answers `Error in input!`; valid answers run the same spans as
    /// arguments. `reverse` (bare `FR`) flexes the banner to `'fr ?'`
    /// and reverse-walks the chosen span — following `express.e`'s
    /// `getDirSpan('')` over the `AquaScan` capture, which skips the
    /// prompt for `FR` (S2/S3, A2, U5–U7).
    async fn file_list_prompt(
        &mut self,
        session: &mut MenuSession,
        reverse: bool,
    ) -> Result<(), T::Error> {
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.services.file_repo.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);
        // The renderer reads the flag set immutably to mark rows, but
        // the `F`/`R` pager verbs mutate it — so borrow it mutably for
        // the whole span and reborrow immutably only at the assemble
        // call. `session` is otherwise untouched after this point.
        let flagged = session.flagged_files_mut();
        let mut state = ScanState::new(false);

        for line in [&b"\x1b[0m"[..], wire::listing_banner(reverse), b""] {
            if self
                .emit_scan_line(&mut state, wire::ScanLine::raw(line.to_vec()), flagged)
                .await?
                == ScanFlow::Quit
            {
                return self.finish_listing().await;
            }
        }
        let read = self
            .read_prompted(&wire::directories_prompt(max), TerminalEcho::Visible)
            .await?;
        let TerminalRead::Line(answer) = read else {
            return self.terminal.flush().await;
        };
        let answer = answer.trim();
        if answer.is_empty() {
            // Enter = None: blank + a single reset (S3 — the abort
            // tail, not the listing tail).
            self.terminal.write(b"\r\n\x1b[0m\r\n").await?;
            return self.terminal.flush().await;
        }
        let span = if answer.eq_ignore_ascii_case("A") {
            FileSpan::All
        } else if answer.eq_ignore_ascii_case("U") {
            FileSpan::Upload
        } else if answer.eq_ignore_ascii_case("H") {
            FileSpan::Hold
        } else if answer.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            FileSpan::Dir(crate::app::menu_command::val_prefix(answer))
        } else {
            self.terminal.write(CRLF).await?;
            self.terminal.write(wire::ERROR_IN_INPUT).await?;
            self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
            return self.terminal.flush().await;
        };
        self.terminal.write(CRLF).await?;
        // The chosen span runs forward for bare `F`, reverse for bare
        // `FR` (`express.e` `displayFileList` passes the `reverse` flag
        // straight through the prompt path).
        self.run_span(&mut state, conference, span, &areas, flagged, reverse)
            .await
    }

    /// `Argument error! Type 'f ?' for help.` under the help banner —
    /// the captured response to unsupported argument forms
    /// (`ae_tierd_aquascan4.txt` U4; single-reset tail).
    async fn file_list_argument_error(&mut self) -> Result<(), T::Error> {
        self.terminal.write(b"\x1b[0m\r\n").await?;
        self.terminal.write(wire::HELP_BANNER.as_bytes()).await?;
        self.terminal.write(b"\r\n\r\n").await?;
        self.terminal.write(wire::ARGUMENT_ERROR).await?;
        self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
        self.terminal.flush().await
    }

    /// Runs an immediate scan over `span`'s directories, forward (`F`)
    /// or reverse (`FR`).
    async fn file_list_span(
        &mut self,
        session: &mut MenuSession,
        span: FileSpan,
        non_stop: bool,
        reverse: bool,
    ) -> Result<(), T::Error> {
        // Per-task session isolation: the menu loop guarantees a
        // joined conference before any command dispatches.
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.services.file_repo.areas_in_conference(conference);
        // Mutable flag set for the whole span: the renderer reborrows
        // it immutably at the assemble call, the `F`/`R` verbs mutate
        // it. `session` is otherwise untouched from here on.
        let flagged = session.flagged_files_mut();
        let mut state = ScanState::new(non_stop);

        // Entry preamble — every argument form (§1.1). Counted: the
        // captured page-1 More? boundary includes these lines.
        for line in [&b"\x1b[0m"[..], wire::listing_banner(reverse), b""] {
            if self
                .emit_scan_line(&mut state, wire::ScanLine::raw(line.to_vec()), flagged)
                .await?
                == ScanFlow::Quit
            {
                return self.finish_listing().await;
            }
        }
        self.run_span(&mut state, conference, span, &areas, flagged, reverse)
            .await
    }

    /// Resolves `span` and scans its directories — everything after
    /// the entry preamble, shared by the argument and prompt paths
    /// (the prompt-initiated scan re-emits no banner, S2).
    async fn run_span(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        span: FileSpan,
        areas: &[FileArea],
        flagged: &mut crate::domain::files::flagged::FlaggedFiles,
        reverse: bool,
    ) -> Result<(), T::Error> {
        let max = areas.last().map_or(0, FileArea::number);
        let mut dirs: Vec<u32> = match span {
            FileSpan::Dir(n) => {
                if n < 1 || n > i64::from(max) {
                    self.terminal.write(&wire::highest_dir_error(max)).await?;
                    self.terminal.write(CRLF).await?;
                    return self.finish_listing().await;
                }
                vec![u32::try_from(n).expect("range-checked above")]
            }
            FileSpan::All => areas.iter().map(FileArea::number).collect(),
            FileSpan::Upload => vec![max],
            FileSpan::Hold => {
                let held = self.services.file_repo.list_held(conference);
                let header = wire::scanning_hold_header(!held.is_empty());
                if self
                    .emit_scan_line(state, wire::ScanLine::raw(header), flagged)
                    .await?
                    == ScanFlow::Quit
                {
                    return self.finish_listing().await;
                }
                // Held files key on area 0 (provisional; hold is single-dir, no re-list).
                if !held.is_empty()
                    && self
                        .stream_dir_body(state, conference, 0, &held, flagged)
                        .await?
                        == ScanFlow::Continue
                {
                    // Hold is a single-dir span: whatever the
                    // post-End verb says, the listing ends here.
                    let _ = self.post_end_pause(state, flagged).await?;
                }
                return self.finish_listing().await;
            }
        };
        if reverse {
            // `FR` walks a multi-dir span highest→lowest
            // (`express.e:27654`). Single-dir spans are a no-op.
            dirs.reverse();
        }

        for (index, dir) in dirs.iter().enumerate() {
            let mut files = self.services.file_repo.find_in_area(conference, *dir);
            // `FR` lists newest-first — the upload-writer appends rows
            // chronologically, so reversing the area's rows is the
            // reverse-chronological order (`express.e` `fileListReverse`
            // vs `displayIt`).
            if reverse {
                files.reverse();
            }
            let header = wire::scanning_dir_header(*dir, !files.is_empty(), reverse);
            if self
                .emit_scan_line(state, wire::ScanLine::raw(header), flagged)
                .await?
                == ScanFlow::Quit
            {
                return self.finish_listing().await;
            }
            if files.is_empty() {
                // A Nothing-found dir runs straight into the next
                // header — no blank, no More? between
                // (ae_tierd_aquascan5.txt V1).
                continue;
            }
            if self
                .stream_dir_body(state, conference, *dir, &files, flagged)
                .await?
                == ScanFlow::Quit
            {
                return self.finish_listing().await;
            }
            if self.post_end_pause(state, flagged).await? == ScanFlow::Quit {
                return self.finish_listing().await;
            }
            if index + 1 < dirs.len() {
                // Y at a non-last dir's post-End More?: the verb's
                // overprint clear, then CRLF, then the next Scanning
                // header (ae_tierd_aquascan3.txt S8 repr :673).
                self.terminal.write(CRLF).await?;
            }
        }
        self.finish_listing().await
    }

    /// The unconditional post-`End of File List` `More?` of paged
    /// mode (`ae_tierd_aquascan3.txt:157-158`; suppressed entirely in
    /// non-stop mode, S7 repr :490). Resets the page counter on
    /// resume — each dir pages afresh.
    async fn post_end_pause(
        &mut self,
        state: &mut ScanState,
        flagged: &mut crate::domain::files::flagged::FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        if state.non_stop {
            return Ok(ScanFlow::Continue);
        }
        let flow = self.scan_more_prompt(state, flagged).await?;
        state.emitted = 0;
        Ok(flow)
    }

    /// The blank line after the scan header, then the assembled body,
    /// through the counting pager.
    async fn stream_dir_body(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        area: u32,
        files: &[File],
        flagged: &mut crate::domain::files::flagged::FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        if self
            .emit_scan_line(state, wire::ScanLine::raw(Vec::new()), flagged)
            .await?
            == ScanFlow::Quit
        {
            return Ok(ScanFlow::Quit);
        }
        // Reborrow `flagged` immutably only for the assemble call: it
        // returns an owned `Vec`, so the immutable borrow ends here and
        // the pager loop below can hand the `&mut` to `emit_scan_line`.
        let lines = wire::assemble_dir_lines(files, conference, area, flagged);
        for line in lines {
            if self.emit_scan_line(state, line, flagged).await? == ScanFlow::Quit {
                return Ok(ScanFlow::Quit);
            }
        }
        Ok(ScanFlow::Continue)
    }

    /// Writes one listing line and, in paged mode, runs the `More?`
    /// interaction once the captured 29-line page fills
    /// (`ae_tierd_aquascan3.txt:212` — the threshold is a `NextScan`
    /// constant; `AquaScan` owns its paging via its own config, and
    /// positions from page 3 onward are a documented COSMETIC
    /// divergence).
    async fn emit_scan_line(
        &mut self,
        state: &mut ScanState,
        line: wire::ScanLine,
        flagged: &mut crate::domain::files::flagged::FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        self.terminal.write(&line.bytes).await?;
        self.terminal.write(CRLF).await?;
        // A listed file row joins the scan-wide registry (the F/R
        // verbs match against it) regardless of paging mode.
        if let Some(listed) = &line.listed {
            state.listed.push(listed.clone());
        }
        if state.non_stop {
            return Ok(ScanFlow::Continue);
        }
        if state.emitted == 0 {
            state.page.clear();
        }
        state.page.push(line);
        state.emitted += 1;
        if state.emitted < PAGE_LINES {
            return Ok(ScanFlow::Continue);
        }
        state.emitted = 0;
        self.scan_more_prompt(state, flagged).await
    }

    /// One `More?` interaction — true hotkeys (slice D2b): every verb
    /// acts on a single keypress with door-style immediate echo
    /// (`ae_tierd_aquascan3.txt` S2/S4-S7, `ae_tierd_aquascan4.txt`
    /// U1-U3, probe battery `ae_tierd_probes.txt` P1/P2).
    ///
    /// - `Q` echoes `Quit` and quits (`ae_tierd_aquascan3.txt:321`).
    /// - `C` form-feeds and resumes — no clear, no re-prompt
    ///   (`:292-321`).
    /// - `n` echoes immediately and **holds**: it is ambiguous
    ///   between `N` (= Quit) and the `ns` prefix, so the door waits
    ///   (U1, identical mid-list and post-End). The next key resolves
    ///   it: `s` wipes the prompt line and asks the Are-you-sure
    ///   confirm (U3); Enter quits with the CR echoed as `\r\n` — no
    ///   `Quit` word, no BS-SP-BS (probe P1); anything else erases
    ///   the held `n` with BS-SP-BS and runs as its own verb (U1).
    /// - `Y`, Enter, unknown keys clear with the captured 69-space
    ///   overprint and resume. A bare LF never reaches here — the
    ///   adapter swallows it (probe P2).
    /// - Case-insensitivity is door-wide inference (only `Q`/`Y`
    ///   upper and `n`/`ns` lower were captured).
    async fn scan_more_prompt(
        &mut self,
        state: &mut ScanState,
        flagged: &mut crate::domain::files::flagged::FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        self.terminal.write(wire::MORE_PROMPT).await?;
        let mut held_n = false;
        loop {
            let read = self.read_key().await?;
            let KeyRead::Key(mut key) = read else {
                // Carrier loss / idle at the pager aborts the listing.
                return Ok(ScanFlow::Quit);
            };
            if held_n {
                held_n = false;
                match key {
                    KeyEvent::Char(b's' | b'S') => {
                        // `ns`: wipe the prompt line (echoed n
                        // included) and confirm (U3). The n-echo +
                        // wipe aggregate is byte-identical to the old
                        // same-packet `ns` line.
                        self.terminal.write(&more_overprint_clear()).await?;
                        self.terminal.write(wire::NS_CONFIRM_PROMPT).await?;
                        let confirm = self.read_key().await?;
                        let KeyRead::Key(confirm) = confirm else {
                            return Ok(ScanFlow::Quit);
                        };
                        self.terminal.write(&more_overprint_clear()).await?;
                        if matches!(confirm, KeyEvent::Char(b'y' | b'Y')) {
                            state.non_stop = true;
                            return Ok(ScanFlow::Continue);
                        }
                        // Declined: More? redraws and paging stays on
                        // (U3).
                        self.terminal.write(wire::MORE_PROMPT).await?;
                        continue;
                    }
                    KeyEvent::Enter => {
                        // Probe P1 (ae_tierd_probes.txt:100-138):
                        // Enter after a held n quits with the CR
                        // echoed as \r\n and the exit tail following
                        // directly — no Quit word, no BS-SP-BS; the
                        // held n stays on the prompt line.
                        self.terminal.write(CRLF).await?;
                        return Ok(ScanFlow::Quit);
                    }
                    other => {
                        // The next key erases the held n, then runs as
                        // its own verb (U1): rebind the scrutinee and
                        // fall through to the verb match below.
                        self.terminal.write(b"\x08 \x08").await?;
                        key = other;
                    }
                }
            }
            match key {
                KeyEvent::Char(b'n' | b'N') => {
                    // Ambiguous N/ns prefix: echo and hold for the
                    // next key (U1; mid-list and post-End identical).
                    self.terminal.write(b"n").await?;
                    self.terminal.flush().await?;
                    held_n = true;
                }
                KeyEvent::Char(b'q' | b'Q') => {
                    self.terminal.write(b"Quit\r\n").await?;
                    return Ok(ScanFlow::Quit);
                }
                KeyEvent::Char(b'c' | b'C') => {
                    self.terminal.write(b"\r\x0c").await?;
                    return Ok(ScanFlow::Continue);
                }
                KeyEvent::Char(verb @ (b'f' | b'F' | b'r' | b'R')) => {
                    // Flagging is silent in the captures
                    // (`ae_tierd_aquascan3.txt` S4/S5): the entry
                    // echoes as typed (probe P3), is cleared with the
                    // wider overprint, and More? redraws — no new wire
                    // bytes. Only the session flag set changes; the
                    // in-place repaint of the newly flagged rows is
                    // Task 3.4b's work (`_newly` is its hook).
                    let by_number = matches!(verb, b'r' | b'R');
                    let prompt: &[u8] = if by_number {
                        wire::FLAG_BY_NUMBER_PROMPT
                    } else {
                        wire::FLAG_BY_NAME_PROMPT
                    };
                    self.terminal.write(&more_overprint_clear()).await?;
                    self.terminal.write(prompt).await?;
                    let Some(entry) = self.read_flag_entry().await? else {
                        return Ok(ScanFlow::Quit);
                    };
                    let newly = apply_flags(&entry, by_number, &state.listed, flagged);
                    self.terminal.write(&flag_overprint_clear()).await?;
                    self.repaint_flagged_rows(state, &newly).await?;
                    self.terminal.write(wire::MORE_PROMPT).await?;
                }
                KeyEvent::Char(b'?') => {
                    // The in-pager pause help, then a redraw of the
                    // current page (`ae_tierd_aquascan4.txt` U2; the
                    // door's redraw window drifts with its internal
                    // paging — NextScan redraws exactly the lines it
                    // showed, a documented COSMETIC divergence).
                    self.terminal.write(wire::PAUSE_HELP).await?;
                    let page = state.page.clone();
                    for line in &page {
                        self.terminal.write(&line.bytes).await?;
                        self.terminal.write(CRLF).await?;
                    }
                    self.terminal.write(wire::MORE_PROMPT).await?;
                }
                _ => {
                    // Y, Enter (no held n), Space, unknown keys: the
                    // captured overprint resume.
                    self.terminal.write(&more_overprint_clear()).await?;
                    return Ok(ScanFlow::Continue);
                }
            }
        }
    }

    /// Paints `[X]` into the marker slot of any newly flagged row
    /// still on the current page (slice D2f): for each such row,
    /// `\r`, cursor up to it, write the marker at its column, then
    /// cursor back to the prompt line. Aligned rows take the 4-col
    /// slot at visible column 14; over-long rows take a trailing
    /// ` [X]` after their last visible column. Rows that scrolled off
    /// earlier pages show their marker at next render. Suppressed when
    /// ANSI is off — the cursor CSI would garble a non-ANSI client.
    ///
    /// # Parameters
    /// - `state`: the current pager state; `state.page` supplies the
    ///   on-screen rows and their geometry.
    /// - `newly`: the keys [`apply_flags`] just turned on — only these
    ///   rows are repainted.
    ///
    /// # Errors
    /// Propagates the terminal's write error.
    async fn repaint_flagged_rows(
        &mut self,
        state: &ScanState,
        newly: &[crate::domain::files::flagged::FlaggedKey],
    ) -> Result<(), T::Error> {
        if newly.is_empty() || !self.terminal.ansi_colour() {
            return Ok(());
        }
        for (index, line) in state.page.iter().enumerate() {
            let Some(listed) = &line.listed else { continue };
            if !newly.contains(&listed.key) {
                continue;
            }
            let up = state.page.len() - index;
            let column_cmd = if listed.aligned {
                "\x1b[14G[X]".to_string()
            } else {
                format!("\x1b[{}G [X]", wire::visible_columns(&line.bytes) + 1)
            };
            let seq = format!("\r\x1b[{up}A{column_cmd}\r\x1b[{up}B");
            self.terminal.write(seq.as_bytes()).await?;
        }
        Ok(())
    }

    /// Hot-key line collector for the flag prompts: each printable
    /// echoes as it arrives (probe P3 — the door's flag read echoes
    /// per keystroke), Backspace erases with BS-SP-BS, and Enter
    /// finishes WITHOUT a terminator echo (the captured exchange has
    /// no CRLF before the 79-space overprint,
    /// `ae_tierd_aquascan3.txt` S4). The entry caps at
    /// `MAX_TERMINAL_LINE_BYTES`; further printables are dropped
    /// unechoed (a `NextExpress` bound, not captured). `None` = carrier
    /// loss / idle timeout.
    async fn read_flag_entry(&mut self) -> Result<Option<String>, T::Error> {
        let mut entry: Vec<u8> = Vec::new();
        loop {
            let read = self.read_key().await?;
            let KeyRead::Key(key) = read else {
                return Ok(None);
            };
            match key {
                KeyEvent::Enter => {
                    return Ok(Some(String::from_utf8_lossy(&entry).into_owned()));
                }
                KeyEvent::Backspace => {
                    if entry.pop().is_some() {
                        self.terminal.write(b"\x08 \x08").await?;
                    }
                }
                KeyEvent::Char(b)
                    if entry.len() < crate::app::input_limits::MAX_TERMINAL_LINE_BYTES =>
                {
                    entry.push(b);
                    self.terminal.write(&[b]).await?;
                }
                _ => {}
            }
        }
    }

    /// Drives the `Z` menu command — the internal zippy text search
    /// (slice D4, `internalCommandZ`, `amiexpress/express.e:26123`).
    ///
    /// `Z` is not shadowed by the `AquaScan` door, so this reproduces the
    /// genuine internal command's wire (plain rows, no `NextScan`
    /// frames), pinned to `comparison/transcripts/ae_tierd_zippy.txt` /
    /// `ae_tierd_zippy2.txt`. The flow: a leading blank, the search
    /// string (inline argument or the `Enter string to search for:`
    /// prompt), the internal `getDirSpan('')` `Directories:` prompt, then
    /// — for a valid directory answer — every matching file's raw DIR
    /// rows under a `Scanning directory N` header. Matching is the
    /// legacy `UpperStr` + `InStr` over each rendered row (filename
    /// included), so any line of a file's block that contains the
    /// upper-cased query dumps the whole block.
    ///
    /// D4 honours the interactive prompt's single-directory answers
    /// (number / `U` / `H`), `A` (all areas), the `=none` abort and the
    /// out-of-range error; the inline `item(1)` area-spec argument
    /// (`Z <q> <span>`) is deferred to slice D7.
    ///
    /// # Parameters
    /// - `session`: the live menu session — supplies the current
    ///   conference whose areas are searched.
    /// - `arg`: the parsed [`ZippyArg`] — an inline query or the
    ///   prompt-for-query marker.
    ///
    /// # Errors
    /// Propagates the terminal's write/read error.
    pub(super) async fn handle_zippy_search(
        &mut self,
        session: &mut MenuSession,
        arg: ZippyArg,
    ) -> Result<(), T::Error> {
        // express.e:26137 — a blank line precedes the search.
        self.terminal.write(CRLF).await?;

        // The query and an optional inline directory span
        // (express.e:26143-26163). Bare `Z` prompts for the query
        // (`:26150`); `Z <token>` supplies it inline; `Z <token> <span>`
        // supplies the directory span too. An empty prompt answer returns.
        let (query, inline_span) = match arg {
            ZippyArg::Query(query) => (query, None),
            ZippyArg::QueryInDir { query, span } => (query, Some(span)),
            ZippyArg::Prompt => {
                let read = self
                    .read_prompted(wire::ZIPPY_SEARCH_PROMPT, TerminalEcho::Visible)
                    .await?;
                let TerminalRead::Line(answer) = read else {
                    return self.terminal.flush().await;
                };
                // express.e:26154 — blank after the search-string read.
                self.terminal.write(CRLF).await?;
                let answer = answer.trim();
                if answer.is_empty() {
                    // express.e:26155-26156 — StrLen=0 returns to the menu.
                    return self.terminal.flush().await;
                }
                (answer.to_string(), None)
            }
        };

        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.services.file_repo.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);

        // The directory span: supplied inline (`getDirSpan(item(1))`,
        // express.e:26162-26163 — no prompt, slice D7) or read from the
        // interactive `getDirSpan('')` Directories prompt (`:26165`),
        // where a blank answer is `(Enter)=none` and aborts (`:26871-26873`).
        let answer = if let Some(span) = inline_span {
            span
        } else {
            let read = self
                .read_prompted(&wire::zippy_directories_prompt(max), TerminalEcho::Visible)
                .await?;
            let TerminalRead::Line(answer) = read else {
                return self.terminal.flush().await;
            };
            let answer = answer.trim();
            if answer.is_empty() {
                self.terminal.write(CRLF).await?;
                return self.terminal.flush().await;
            }
            answer.to_string()
        };

        // Resolve the answer to a span (getDirSpan, express.e:26881-26908);
        // unrecognised / out-of-range is "No such directory." (`:26905`).
        let Some(span) = resolve_zippy_span(answer.trim(), max) else {
            return self.zippy_no_such_directory().await;
        };

        // express.e:26172 — blank after a successful getDirSpan.
        self.terminal.write(CRLF).await?;

        let needle = query.to_ascii_uppercase().into_bytes();
        match span {
            ZippySpan::Hold => {
                self.terminal.write(wire::ZIPPY_SCANNING_HOLD).await?;
                self.terminal.write(CRLF).await?;
                let files = self.services.file_repo.list_held(conference);
                self.zippy_dump_matches(&files, &needle).await?;
            }
            ZippySpan::Dirs(dirs) => {
                for dir in dirs {
                    self.terminal
                        .write(&wire::zippy_scanning_dir_header(dir))
                        .await?;
                    self.terminal.write(CRLF).await?;
                    let files = self.services.file_repo.find_in_area(conference, dir);
                    self.zippy_dump_matches(&files, &needle).await?;
                }
            }
        }

        // express.e:26211 — trailing blank.
        self.terminal.write(CRLF).await?;
        self.terminal.flush().await
    }

    /// The internal `getDirSpan` out-of-range error, framed with the
    /// legacy leading and trailing blanks
    /// (`amiexpress/express.e:26905`).
    async fn zippy_no_such_directory(&mut self) -> Result<(), T::Error> {
        self.terminal.write(CRLF).await?;
        self.terminal.write(wire::ZIPPY_NO_SUCH_DIRECTORY).await?;
        self.terminal.write(b"\r\n\r\n").await?;
        self.terminal.flush().await
    }

    /// Dumps the raw DIR rows of every file in `files` whose block
    /// matches `needle_upper` — the legacy zippy block dump
    /// (`amiexpress/express.e:27529-27620`): a file matches when any of
    /// its rendered rows (filename row included) contains the
    /// upper-cased query, and the whole block is emitted on a hit.
    async fn zippy_dump_matches(
        &mut self,
        files: &[File],
        needle_upper: &[u8],
    ) -> Result<(), T::Error> {
        for file in files {
            let rows = dir_row::dir_row_lines(file);
            if rows.iter().any(|row| row_contains_ci(row, needle_upper)) {
                for row in rows {
                    self.terminal.write(&row).await?;
                    self.terminal.write(CRLF).await?;
                }
            }
        }
        Ok(())
    }

    /// The two-reset listing exit tail + flush.
    async fn finish_listing(&mut self) -> Result<(), T::Error> {
        self.terminal.write(LISTING_EXIT_TAIL).await?;
        self.terminal.flush().await
    }
}

/// The `NextScan` page height — fitted against every captured `More?`
/// boundary (29/29 exact on pages 1-2; see `designs/NEXTSCAN.md` §1.5).
const PAGE_LINES: u32 = 29;

/// `\r` + 69 spaces + `\r`: the captured `More?`/ns-confirm overprint
/// clear (never `ESC[K`).
fn more_overprint_clear() -> Vec<u8> {
    let mut bytes = vec![b'\r'];
    bytes.extend(std::iter::repeat_n(b' ', 69));
    bytes.push(b'\r');
    bytes
}

/// `\r` + 79 spaces + `\r`: the wider overprint after a flag entry
/// (`ae_tierd_aquascan3.txt` S4).
fn flag_overprint_clear() -> Vec<u8> {
    let mut bytes = vec![b'\r'];
    bytes.extend(std::iter::repeat_n(b' ', 79));
    bytes.push(b'\r');
    bytes
}

/// Flags the files named/numbered in a `More?` flag entry against the
/// scan's listed registry. `F` matches whitespace-separated names
/// (case-insensitively, via the uppercase-folded `FlaggedKey::name`);
/// `R` matches `[ File #N ]` numbers. Tokens that match nothing are
/// silently ignored (the door accepts junk silently — the accidental
/// capture fed `99`/`A`/`U` with no feedback). Returns the NEWLY
/// flagged keys (the repaint set — Task 3.4b consumes it).
fn apply_flags(
    entry: &str,
    by_number: bool,
    listed: &[wire::ListedRow],
    flagged: &mut crate::domain::files::flagged::FlaggedFiles,
) -> Vec<crate::domain::files::flagged::FlaggedKey> {
    let mut newly = Vec::new();
    for token in entry.split_whitespace() {
        let matched = if by_number {
            token
                .parse::<u32>()
                .ok()
                .and_then(|n| listed.iter().find(|row| row.number == Some(n)))
        } else {
            let wanted = token.to_ascii_uppercase();
            listed.iter().find(|row| row.key.name() == wanted)
        };
        if let Some(row) = matched {
            if flagged.flag(row.key.clone()) {
                newly.push(row.key.clone());
            }
        }
    }
    newly
}

/// The resolved `getDirSpan` answer for a zippy scan (slice D4): a list
/// of directory numbers (a single dir, the upload dir, or every dir for
/// `A`) or the hold dir.
enum ZippySpan {
    Dirs(Vec<u32>),
    Hold,
}

/// Resolves a `getDirSpan` answer — a Directories-prompt reply or an
/// inline `item(1)` token — to a [`ZippySpan`]
/// (`amiexpress/express.e:26881-26908`): `A` = all dirs, `U` = the upload
/// (highest) dir, `H` = the hold dir, a number = that single dir. Returns
/// `None` for the out-of-range / unrecognised case the caller renders as
/// `No such directory.` (`:26904-26906`). The blank `=none` answer is
/// handled by the prompt path before this and never reaches here.
///
/// `A`/`U`/`H` match the whole token (not the legacy first-char test) for
/// consistency with the `F` prompt and the captured answers; a token like
/// `Apple` therefore takes the `No such directory.` path rather than the
/// legacy all-dirs reading — an uncaptured edge.
fn resolve_zippy_span(answer: &str, max: u32) -> Option<ZippySpan> {
    if answer.eq_ignore_ascii_case("A") {
        Some(ZippySpan::Dirs((1..=max).collect()))
    } else if answer.eq_ignore_ascii_case("U") {
        Some(ZippySpan::Dirs(vec![max]))
    } else if answer.eq_ignore_ascii_case("H") {
        Some(ZippySpan::Hold)
    } else if answer.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let requested = crate::app::menu_command::val_prefix(answer);
        if requested < 1 || requested > i64::from(max) {
            None
        } else {
            Some(ZippySpan::Dirs(vec![
                u32::try_from(requested).expect("range-checked above")
            ]))
        }
    } else {
        None
    }
}

/// ASCII case-insensitive substring test for the zippy match: does `row`
/// (any case) contain `needle_upper` (already upper-cased)? Mirrors the
/// legacy `UpperStr` + `InStr` over each rendered DIR line
/// (`amiexpress/express.e:27597-27598`). An empty needle never reaches
/// here (an empty query returns before the scan), but is treated as a
/// universal match for totality.
fn row_contains_ci(row: &[u8], needle_upper: &[u8]) -> bool {
    if needle_upper.is_empty() {
        return true;
    }
    let upper: Vec<u8> = row.iter().map(u8::to_ascii_uppercase).collect();
    upper
        .windows(needle_upper.len())
        .any(|window| window == needle_upper)
}

/// Per-span pager state.
struct ScanState {
    /// Lines emitted since the last page boundary.
    emitted: u32,
    /// `NS` requested — no pauses at all.
    non_stop: bool,
    /// The current page's lines, for the `?` help's page redraw.
    page: Vec<wire::ScanLine>,
    /// Every listed file's identity, scan-wide — the registry the
    /// F/R flag verbs match against (slice D2f, Task 3.4). Populated
    /// as rows stream.
    listed: Vec<wire::ListedRow>,
}

impl ScanState {
    fn new(non_stop: bool) -> Self {
        Self {
            emitted: 0,
            non_stop,
            page: Vec::new(),
            listed: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
