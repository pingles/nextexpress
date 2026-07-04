//! The `NextScan` scan engine (SYSTEM.md item 17): the paged listing
//! machine `F`/`FR` drive and `N` (slice D9) reuses — span resolution,
//! per-dir streaming, the 29-line `More?` pager with its verb machine,
//! and the flag/repaint helpers.
//!
//! Engine methods stay on [`MenuFlow`](crate::app::menu_flow::MenuFlow)
//! in this second `impl` block (not a standalone struct): the flows
//! interleave engine emits with `prompt_line`, which needs `&mut self`
//! plus the session, so a struct borrowing the terminal out of
//! `MenuFlow` would deadlock that interleaving. Byte parity is pinned
//! by the capture-replay tests in `scan/tests.rs` and the smoke
//! `rust/tests/tierd_file_list_smoke.rs`.

use crate::app::menu_command::FileSpan;
use crate::app::terminal::{KeyEvent, KeyRead, Terminal};
use crate::app::wire_text::CRLF;
use crate::domain::files::area::{FileArea, FileAreaRef};
use crate::domain::files::file::File;
use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};

use super::wire;

/// Whether a paged listing keeps streaming or the user quit out —
/// plus the two dir-navigation verbs the door honours at `More?`
/// (`ae_tierd_help_audit.txt`, 2026-07-04).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum ScanFlow {
    Continue,
    Quit,
    /// `K`: abandon the rest of the current dir (footer and post-End
    /// `More?` included) and jump to the dir transition (PK).
    SkipDir,
    /// `L`: restart the current dir from its header on a fresh page
    /// (PL) — rows are re-fetched, so a changed repository re-lists.
    ReloadDir,
}

/// The two-reset tail every listing-shaped exit emits before the menu
/// prompt (`ae_tierd_aquascan3.txt:163`; per-path tails are a pinned
/// asymmetry — aborts and argument errors emit one reset only).
const LISTING_EXIT_TAIL: &[u8] = b"\x1b[0m\r\n\x1b[0m\r\n";

/// One assembled listing line plus, on a file's first row, its
/// catalogue identity — the pager records these for flag matching
/// and in-place repaint (slice D2f). Non-file lines carry `None`.
#[derive(Clone)]
pub(super) struct ScanLine {
    pub(super) bytes: Vec<u8>,
    pub(super) listed: Option<ListedRow>,
}

impl ScanLine {
    /// A non-file line (banner, header, separator, blank, footer).
    pub(super) fn raw(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            listed: None,
        }
    }
}

/// A listed file: its flag key, its `[ File #N ]` number (framed rows
/// only; plain rows consume no number), and whether it carries the
/// aligned marker slot (name < 13) vs a trailing ` [X]` (over-long).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedRow {
    pub(super) key: FlaggedKey,
    pub(super) number: Option<u32>,
    pub(super) aligned: bool,
}

/// What a `NextScan` walk lists per directory and how it titles it
/// (item 17's generalisation — a mode enum owned by the engine, so
/// per-dir row acquisition stays lazy and quit-stops-fetching holds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScanKind {
    /// `F` / `FR` / `N`'s `R` answer: the full listing, forward or
    /// newest-first.
    Full {
        /// List newest-first and walk multi-dir spans highest→lowest
        /// (`express.e:27654`).
        reverse: bool,
    },
    /// `N`: files uploaded on/after `cutoff` — **inclusive**
    /// (`express.e:27976-27986` `ddt>=day`; slice D9).
    NewSince {
        /// The UTC-midnight boundary instant.
        cutoff: std::time::SystemTime,
        /// The `MM-DD-YY` the headers echo — derived once from the
        /// same date via `dir_row::format_dir_date`, never recomputed.
        label: String,
    },
    /// `N !x`: the x newest files, ascending
    /// (`ae_tierd_newfiles.txt` N7n: MYDEMO then TOOLPACK).
    NewestLast {
        /// How many files from the tail of the dir.
        count: u32,
    },
}

/// A scan's mode: what each directory lists ([`ScanKind`]) plus `N`'s
/// `Q` quick flag (capture N7q — description continuations dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScanMode {
    pub(super) kind: ScanKind,
    pub(super) quick: bool,
}

impl ScanMode {
    /// Whether multi-dir spans walk highest→lowest (the `FR` walk).
    fn reverse_walk(&self) -> bool {
        matches!(self.kind, ScanKind::Full { reverse: true })
    }
}

impl<T> crate::app::menu_flow::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// The counted entry preamble every immediate scan emits — reset
    /// line, `banner`, blank — through the pager (the captured page-1
    /// More? boundary of the span paths includes these lines).
    pub(super) async fn begin_listing(
        &mut self,
        state: &mut ScanState,
        banner: &[u8],
        flagged: &mut FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        for line in [&b"\x1b[0m"[..], banner, b""] {
            // `SkipDir`/`ReloadDir` at a preamble More? have no dir to
            // act on yet — they fall through as a resume (provisional;
            // the door was only probed mid-dir and at post-End).
            if self
                .emit_scan_line(state, ScanLine::raw(line.to_vec()), flagged)
                .await?
                == ScanFlow::Quit
            {
                return Ok(ScanFlow::Quit);
            }
        }
        Ok(ScanFlow::Continue)
    }

    /// Resolves `span` and scans its directories under `mode` —
    /// everything after the entry preamble, shared by the argument and
    /// prompt paths (the prompt-initiated scan re-emits no banner, S2)
    /// — then emits the two-reset exit tail (the capture-confirmed
    /// invariant of every listing-shaped exit, quit or not).
    pub(super) async fn run_span(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        span: FileSpan,
        areas: &[FileArea],
        flagged: &mut FlaggedFiles,
        mode: &ScanMode,
    ) -> Result<(), T::Error> {
        self.walk_span(state, conference, span, areas, flagged, mode)
            .await?;
        self.finish_listing().await
    }

    /// The span walk proper — it returns when the span is exhausted,
    /// the user quits out of the pager, or an error path ends the walk;
    /// the [`Self::run_span`] wrapper appends the shared exit tail
    /// either way (which is why no flow value is surfaced).
    async fn walk_span(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        span: FileSpan,
        areas: &[FileArea],
        flagged: &mut FlaggedFiles,
        mode: &ScanMode,
    ) -> Result<(), T::Error> {
        let max = areas.last().map_or(0, FileArea::number);
        let mut dirs: Vec<u32> = match span {
            FileSpan::Dir(n) => {
                if n < 1 || n > i64::from(max) {
                    self.terminal.write(&wire::highest_dir_error(max)).await?;
                    self.terminal.write(CRLF).await?;
                    return Ok(());
                }
                vec![u32::try_from(n).expect("range-checked above")]
            }
            FileSpan::All => areas.iter().map(FileArea::number).collect(),
            FileSpan::Upload => vec![max],
            FileSpan::Hold => return self.walk_hold(state, conference, flagged, mode).await,
        };
        if mode.reverse_walk() {
            // `FR` walks a multi-dir span highest→lowest
            // (`express.e:27654`). Single-dir spans are a no-op.
            dirs.reverse();
        }

        'dirs: for (index, dir) in dirs.iter().enumerate() {
            // The dir loop: `ReloadDir` restarts it (rows re-fetched),
            // `SkipDir` breaks to the transition, `Quit` ends the walk.
            loop {
                let files = self.dir_rows(conference, *dir, mode);
                let header = dir_header(*dir, !files.is_empty(), mode);
                match self
                    .emit_scan_line(state, ScanLine::raw(header), flagged)
                    .await?
                {
                    ScanFlow::Quit => return Ok(()),
                    ScanFlow::ReloadDir => continue,
                    ScanFlow::SkipDir => break,
                    ScanFlow::Continue => {}
                }
                if files.is_empty() {
                    // A Nothing-found dir runs straight into the next
                    // header — no blank, no More?, no transition CRLF
                    // between (ae_tierd_aquascan5.txt V1).
                    continue 'dirs;
                }
                match self
                    .stream_dir_body(state, conference, &files, flagged, mode)
                    .await?
                {
                    ScanFlow::Quit => return Ok(()),
                    ScanFlow::ReloadDir => continue,
                    ScanFlow::SkipDir => break,
                    ScanFlow::Continue => {}
                }
                match self.post_end_pause(state, flagged).await? {
                    ScanFlow::Quit => return Ok(()),
                    // ReloadDir falls off the loop end and restarts
                    // the dir.
                    ScanFlow::ReloadDir => {}
                    // Continue — or a SkipDir with nothing left to
                    // skip: the dir is done either way.
                    ScanFlow::Continue | ScanFlow::SkipDir => break,
                }
            }
            if index + 1 < dirs.len() {
                // Y at a non-last dir's post-End More?: the verb's
                // overprint clear, then CRLF, then the next Scanning
                // header (ae_tierd_aquascan3.txt S8 repr :673). K's
                // mid-list skip shares the same transition (PK).
                self.terminal.write(CRLF).await?;
            }
        }
        Ok(())
    }

    /// The `H`old arm of the walk: the conference's held files under
    /// `mode`, with the dir→HOLD header substitution.
    async fn walk_hold(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        flagged: &mut FlaggedFiles,
        mode: &ScanMode,
    ) -> Result<(), T::Error> {
        // Hold is a single-dir span: `ReloadDir` restarts it, every
        // other verb ends the listing here.
        loop {
            let held = self.hold_rows(conference, mode);
            let header = hold_header(!held.is_empty(), mode);
            match self
                .emit_scan_line(state, ScanLine::raw(header), flagged)
                .await?
            {
                ScanFlow::ReloadDir => continue,
                ScanFlow::Continue => {}
                ScanFlow::Quit | ScanFlow::SkipDir => return Ok(()),
            }
            if held.is_empty() {
                return Ok(());
            }
            match self
                .stream_dir_body(state, conference, &held, flagged, mode)
                .await?
            {
                ScanFlow::ReloadDir => continue,
                ScanFlow::Continue => {}
                ScanFlow::Quit | ScanFlow::SkipDir => return Ok(()),
            }
            if self.post_end_pause(state, flagged).await? == ScanFlow::ReloadDir {
                continue;
            }
            return Ok(());
        }
    }

    /// One directory's rows under `mode` — the engine's lazy per-dir
    /// acquisition point (fetch happens only when the walk reaches the
    /// dir, so quitting stops fetching).
    fn dir_rows(&self, conference: u32, dir: u32, mode: &ScanMode) -> Vec<File> {
        match &mode.kind {
            ScanKind::Full { reverse } => {
                let mut files = self.files_in_area(FileAreaRef::new(conference, dir));
                // Reverse lists newest-first — the upload-writer
                // appends rows chronologically, so reversing the
                // area's rows is the reverse-chronological order
                // (`express.e` `fileListReverse` vs `displayIt`).
                if *reverse {
                    files.reverse();
                }
                files
            }
            ScanKind::NewSince { cutoff, .. } => {
                self.new_files_in_area(FileAreaRef::new(conference, dir), *cutoff)
            }
            // `!x`: the x newest = the ascending tail. Saturating so
            // `N !999` over a smaller dir lists the whole dir instead
            // of panicking (an unprobed edge — TO-CONFIRM #8).
            ScanKind::NewestLast { count } => {
                let mut files = self.files_in_area(FileAreaRef::new(conference, dir));
                let keep = files.len().saturating_sub(*count as usize);
                files.split_off(keep)
            }
        }
    }

    /// The hold dir's rows under `mode`. The `Full` arm ignores
    /// `reverse` — F's captured hold listing walks forward (S9), and
    /// the door never showed a reverse-hold variant.
    fn hold_rows(&self, conference: u32, mode: &ScanMode) -> Vec<File> {
        match &mode.kind {
            ScanKind::Full { .. } => self.held_files(conference),
            // `H` under a date/newest scan is UNCAPTURED (TO-CONFIRM
            // #1): the interim behaviour filters the held rows the way
            // the mode filters a dir — defined and non-panicking for a
            // prompt-advertised answer; PLAUSIBLE in COMMAND_PARITY.md.
            ScanKind::NewSince { cutoff, .. } => {
                let cutoff = *cutoff;
                self.held_files(conference)
                    .into_iter()
                    .filter(|file| file.uploaded_at() >= cutoff)
                    .collect()
            }
            ScanKind::NewestLast { count } => {
                let mut held = self.held_files(conference);
                let keep = held.len().saturating_sub(*count as usize);
                held.split_off(keep)
            }
        }
    }

    /// The unconditional post-`End of File List` `More?` of paged
    /// mode (`ae_tierd_aquascan3.txt:157-158`; suppressed entirely in
    /// non-stop mode, S7 repr :490). Resets the page counter on
    /// resume — each dir pages afresh.
    async fn post_end_pause(
        &mut self,
        state: &mut ScanState,
        flagged: &mut FlaggedFiles,
    ) -> Result<ScanFlow, T::Error> {
        if state.non_stop {
            return Ok(ScanFlow::Continue);
        }
        let flow = self.scan_more_prompt(state, flagged).await?;
        // A reload has already pre-counted its form-feed line —
        // resetting here would drop it (PL's captured boundary).
        if flow != ScanFlow::ReloadDir {
            state.emitted = 0;
        }
        Ok(flow)
    }

    /// The blank line after the scan header, then the assembled body,
    /// through the counting pager.
    async fn stream_dir_body(
        &mut self,
        state: &mut ScanState,
        conference: u32,
        files: &[File],
        flagged: &mut FlaggedFiles,
        mode: &ScanMode,
    ) -> Result<ScanFlow, T::Error> {
        let flow = self
            .emit_scan_line(state, ScanLine::raw(Vec::new()), flagged)
            .await?;
        if flow != ScanFlow::Continue {
            return Ok(flow);
        }
        // Reborrow `flagged` immutably only for the assemble call: it
        // returns an owned `Vec`, so the immutable borrow ends here and
        // the pager loop below can hand the `&mut` to `emit_scan_line`.
        let lines = wire::assemble_dir_lines(files, conference, flagged, mode.quick);
        for line in lines {
            let flow = self.emit_scan_line(state, line, flagged).await?;
            if flow != ScanFlow::Continue {
                return Ok(flow);
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
    pub(super) async fn emit_scan_line(
        &mut self,
        state: &mut ScanState,
        line: ScanLine,
        flagged: &mut FlaggedFiles,
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
        flagged: &mut FlaggedFiles,
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
                KeyEvent::Char(b'k' | b'K') => {
                    // Skip dir (ae_tierd_help_audit.txt PK): the
                    // verb's overprint clear here; the walk's
                    // dir-transition CRLF and the next header follow.
                    self.terminal.write(&more_overprint_clear()).await?;
                    return Ok(ScanFlow::SkipDir);
                }
                KeyEvent::Char(b'l' | b'L') => {
                    // Reload dir (PL): form feed + CRLF, then the walk
                    // restarts the current dir from its header. The FF
                    // line counts toward the fresh page (the door's
                    // More? fires 28 lines after it — PL's boundary),
                    // and the page buffer restarts with the cleared
                    // screen so `?`-redraw/repaint geometry stays true.
                    self.terminal.write(b"\x0c\r\n").await?;
                    state.page.clear();
                    state.emitted = 1;
                    return Ok(ScanFlow::ReloadDir);
                }
                KeyEvent::CtrlC => {
                    // Ctrl-C (PCC): the door's `**Break` line; the
                    // standard exit tail follows from the quit path.
                    self.terminal.write(b"\r\n\x1b[0m**Break\r\n").await?;
                    return Ok(ScanFlow::Quit);
                }
                KeyEvent::Char(verb @ (b'f' | b'F' | b'r' | b'R')) => {
                    // Flagging is silent in the captures
                    // (`ae_tierd_aquascan3.txt` S4/S5): the entry
                    // echoes as typed (probe P3), is cleared with the
                    // wider overprint, and More? redraws — no new wire
                    // bytes. Only the session flag set changes, plus
                    // the in-place repaint of the newly flagged rows.
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
        newly: &[FlaggedKey],
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

    /// The two-reset listing exit tail + flush.
    pub(super) async fn finish_listing(&mut self) -> Result<(), T::Error> {
        self.terminal.write(LISTING_EXIT_TAIL).await?;
        self.terminal.flush().await
    }
}

/// The `NextScan` page height — fitted against every captured `More?`
/// boundary (29/29 exact on pages 1-2; see `designs/NEXTSCAN.md` §1.5).
const PAGE_LINES: u32 = 29;

/// `\r` + `width` spaces + `\r`: the captured overprint clear shape
/// (never `ESC[K`) — 69 columns at `More?`/ns-confirm, 79 after a flag
/// entry.
fn overprint_clear(width: usize) -> Vec<u8> {
    let mut bytes = vec![b'\r'];
    bytes.extend(std::iter::repeat_n(b' ', width));
    bytes.push(b'\r');
    bytes
}

/// The captured `More?`/ns-confirm overprint clear.
fn more_overprint_clear() -> Vec<u8> {
    overprint_clear(69)
}

/// The wider overprint after a flag entry (`ae_tierd_aquascan3.txt` S4).
fn flag_overprint_clear() -> Vec<u8> {
    overprint_clear(79)
}

/// The `Scanning …` header for one directory under `mode`.
fn dir_header(dir: u32, found: bool, mode: &ScanMode) -> Vec<u8> {
    match &mode.kind {
        ScanKind::Full { reverse } => wire::scanning_dir_header(dir, found, *reverse),
        ScanKind::NewSince { label, .. } => wire::scanning_new_header(dir, label, found),
        ScanKind::NewestLast { count } => wire::scanning_newest_header(dir, *count, found),
    }
}

/// The HOLD-dir header under `mode` — F's dir→HOLD substitution
/// applied per mode.
fn hold_header(found: bool, mode: &ScanMode) -> Vec<u8> {
    match &mode.kind {
        ScanKind::Full { .. } => wire::scanning_hold_header(found),
        ScanKind::NewSince { label, .. } => wire::scanning_new_hold_header(label, found),
        ScanKind::NewestLast { count } => wire::scanning_newest_hold_header(*count, found),
    }
}

/// Flags the files named/numbered in a `More?` flag entry against the
/// scan's listed registry. `F` matches whitespace-separated names
/// (case-insensitively, via the uppercase-folded `FlaggedKey::name`);
/// `R` matches `[ File #N ]` numbers. Tokens that match nothing are
/// silently ignored (the door accepts junk silently — the accidental
/// capture fed `99`/`A`/`U` with no feedback). Returns the NEWLY
/// flagged keys (the repaint set).
pub(super) fn apply_flags(
    entry: &str,
    by_number: bool,
    listed: &[ListedRow],
    flagged: &mut FlaggedFiles,
) -> Vec<FlaggedKey> {
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

/// Per-span pager state.
pub(super) struct ScanState {
    /// Lines emitted since the last page boundary.
    emitted: u32,
    /// `NS` requested — no pauses at all.
    non_stop: bool,
    /// The current page's lines, for the `?` help's page redraw.
    page: Vec<ScanLine>,
    /// Every listed file's identity, scan-wide — the registry the
    /// F/R flag verbs match against (slice D2f, Task 3.4). Populated
    /// as rows stream.
    listed: Vec<ListedRow>,
}

impl ScanState {
    /// A fresh pager state; `non_stop` suppresses every `More?`.
    pub(super) fn new(non_stop: bool) -> Self {
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
