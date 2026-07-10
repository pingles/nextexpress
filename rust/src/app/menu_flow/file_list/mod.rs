//! The `F` command — `NextScan` file listings (slice D2).
//!
//! Parity target: the `AquaScan` v1.0 door experience with `NextScan`
//! branding (`comparison/evidence-tierD/live-observations.md`;
//! cleanest captures in `comparison/transcripts/ae_tierd_aquascan3.txt`).
//! The shadowed internal `internalCommandF`
//! (`amiexpress/express.e:24877`) is the stock diff record only.

mod dir_row;
mod new_files;
mod scan;
#[cfg(test)]
mod test_support;
mod wire;

use self::scan::{DirectoryOrder, ScanFlow, ScanKind, ScanMode, ScanState};
use crate::app::menu_command::{FileListArg, FileSpan, ZippyArg};
use crate::app::terminal::Terminal;
use crate::app::wire_text::CRLF;
use crate::domain::files::area::{FileArea, FileAreaRef};
use crate::domain::files::file::File;
use crate::domain::files::repository::FileRepositoryError;
use crate::domain::session::typed::MenuSession;

/// Logs a repository failure and renders the catalogue as empty — the
/// wire the legacy shows for an unreadable DIR file is the empty
/// listing (headers followed by `Nothing found!`); the sysop learns of
/// the backend failure from the log. Shared by the three read helpers
/// below so the policy lives in one place.
fn empty_on_error<V>(what: &str, error: &FileRepositoryError) -> Vec<V> {
    eprintln!("file repository: {what} failed: {error}");
    Vec::new()
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// The conference's areas under the row-5 error policy
    /// ([`empty_on_error`]).
    fn areas_in_conference(&self, conference: u32) -> Vec<FileArea> {
        self.services
            .file_repo
            .areas_in_conference(conference)
            .unwrap_or_else(|error| empty_on_error("areas_in_conference", &error))
    }

    /// One area's listing-visible files under the row-5 error policy
    /// ([`empty_on_error`]).
    fn files_in_area(&self, area: FileAreaRef) -> Vec<File> {
        self.services
            .file_repo
            .find_in_area(area)
            .unwrap_or_else(|error| empty_on_error("find_in_area", &error))
    }

    /// The conference's held files under the row-5 error policy
    /// ([`empty_on_error`]).
    fn held_files(&self, conference: u32) -> Vec<File> {
        self.services
            .file_repo
            .list_held(conference)
            .unwrap_or_else(|error| empty_on_error("list_held", &error))
    }

    /// One area's listing-visible files uploaded on/after `since`
    /// (inclusive — the `N` date filter), under the row-5 error policy
    /// ([`empty_on_error`]).
    fn new_files_in_area(&self, area: FileAreaRef, since: std::time::SystemTime) -> Vec<File> {
        self.services
            .file_repo
            .list_new_since(area, since)
            .unwrap_or_else(|error| empty_on_error("list_new_since", &error))
    }

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
                quick,
                fr_banner,
            } => {
                let mode = ScanMode {
                    kind: ScanKind::Full { reverse },
                    directory_order: if reverse {
                        DirectoryOrder::Reverse
                    } else {
                        DirectoryOrder::Forward
                    },
                    quick,
                };
                self.file_list_span(session, span, &mode, non_stop, fr_banner)
                    .await
            }
            FileListArg::Prompt { reverse, fr_banner } => {
                self.file_list_prompt(session, reverse, fr_banner).await
            }
            FileListArg::Help => {
                // `F ?` (`ae_tierd_aquascan3.txt` S1).
                self.terminal.write(wire::HELP_SCREEN.as_bytes()).await?;
                self.terminal.flush().await
            }
        }
    }

    /// Bare `F` / bare `FR` / spaced `F R`: the door's own
    /// `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=None ? `
    /// line prompt (`ae_tierd_aquascan3.txt:163`; Visible read — the
    /// answer echo is the adapter's). Enter aborts silently; junk
    /// answers `Error in input!`; valid answers run the same spans as
    /// arguments. `reverse` (`FR`, or the spaced `R` marker — captured
    /// `ae_tierd_fr_probe.txt` FR1) reverse-walks the chosen span;
    /// `fr_banner` follows the typed head (the door keeps `'f ?'` for
    /// `F R`). Bare-`FR`-prompts follows `express.e`'s `getDirSpan('')`
    /// over the `AquaScan` capture, which skips the prompt for `FR`
    /// (S2/S3, A2, U5–U7).
    async fn file_list_prompt(
        &mut self,
        session: &mut MenuSession,
        reverse: bool,
        fr_banner: bool,
    ) -> Result<(), T::Error> {
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);
        let mut state = ScanState::new(false, conference);

        // The banner emits borrow the flag set only within this block,
        // so the prompt read below can take `session` mutably (the
        // merged reader stamps the idle clock on it).
        {
            let flagged = session.flagged_files_mut();
            if self
                .begin_listing(&mut state, wire::listing_banner(fr_banner), flagged)
                .await?
                == ScanFlow::Quit
            {
                return self.finish_listing().await;
            }
        }
        let answer = match self
            .prompt_line(
                session,
                &wire::directories_prompt(max),
                super::EmptyMeaning::Verbatim,
                super::AbortNotice::Silent,
            )
            .await?
        {
            super::PromptLine::Entered(answer) => answer,
            super::PromptLine::Aborted => return self.terminal.flush().await,
            super::PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
        };
        if answer.is_empty() {
            // Enter = None: blank + a single reset (S3 — the abort
            // tail, not the listing tail).
            self.terminal.write(b"\r\n\x1b[0m\r\n").await?;
            return self.terminal.flush().await;
        }
        let Some(span) = crate::app::menu_command::parse_span_token(&answer) else {
            self.terminal.write(CRLF).await?;
            self.terminal.write(wire::ERROR_IN_INPUT).await?;
            self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
            return self.terminal.flush().await;
        };
        self.terminal.write(CRLF).await?;
        // The chosen span runs forward for bare `F`, reverse for bare
        // `FR` (`express.e` `displayFileList` passes the `reverse` flag
        // straight through the prompt path).
        // Reborrow the flag set for the span walk (the renderer reads
        // it to mark rows; the `F`/`R` pager verbs mutate it).
        let flagged = session.flagged_files_mut();
        let mode = ScanMode {
            kind: ScanKind::Full { reverse },
            directory_order: if reverse {
                DirectoryOrder::Reverse
            } else {
                DirectoryOrder::Forward
            },
            quick: false,
        };
        self.run_span(&mut state, conference, span, &areas, flagged, &mode)
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

    /// Runs an immediate scan over `span`'s directories under `mode` —
    /// forward or reverse, optionally quick. `fr_banner` picks the
    /// banner label (it follows the typed head, not the mode —
    /// `ae_tierd_fr_probe.txt` FR1/FR2).
    async fn file_list_span(
        &mut self,
        session: &mut MenuSession,
        span: FileSpan,
        mode: &ScanMode,
        non_stop: bool,
        fr_banner: bool,
    ) -> Result<(), T::Error> {
        // Per-task session isolation: the menu loop guarantees a
        // joined conference before any command dispatches.
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.areas_in_conference(conference);
        // Mutable flag set for the whole span: the renderer reborrows
        // it immutably at the assemble call, the `F`/`R` verbs mutate
        // it. `session` is otherwise untouched from here on.
        let flagged = session.flagged_files_mut();
        let mut state = ScanState::new(non_stop, conference);

        // Entry preamble — every argument form (§1.1). Counted: the
        // captured page-1 More? boundary includes these lines.
        if self
            .begin_listing(&mut state, wire::listing_banner(fr_banner), flagged)
            .await?
            == ScanFlow::Quit
        {
            return self.finish_listing().await;
        }
        self.run_span(&mut state, conference, span, &areas, flagged, mode)
            .await
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
                let answer = match self
                    .prompt_line(
                        session,
                        wire::ZIPPY_SEARCH_PROMPT,
                        super::EmptyMeaning::Verbatim,
                        super::AbortNotice::Silent,
                    )
                    .await?
                {
                    super::PromptLine::Entered(answer) => answer,
                    super::PromptLine::Aborted => return self.terminal.flush().await,
                    super::PromptLine::Kept => {
                        unreachable!("Verbatim prompts have no keep branch")
                    }
                };
                // express.e:26154 — blank after the search-string read.
                self.terminal.write(CRLF).await?;
                if answer.is_empty() {
                    // express.e:26155-26156 — StrLen=0 returns to the menu.
                    return self.terminal.flush().await;
                }
                (answer, None)
            }
        };

        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);

        // The directory span: supplied inline (`getDirSpan(item(1))`,
        // express.e:26162-26163 — no prompt, slice D7) or read from the
        // interactive `getDirSpan('')` Directories prompt (`:26165`),
        // where a blank answer is `(Enter)=none` and aborts (`:26871-26873`).
        let answer = if let Some(span) = inline_span {
            span
        } else {
            match self
                .prompt_line(
                    session,
                    &wire::zippy_directories_prompt(max),
                    super::EmptyMeaning::Verbatim,
                    super::AbortNotice::Silent,
                )
                .await?
            {
                super::PromptLine::Entered(answer) if answer.is_empty() => {
                    self.terminal.write(CRLF).await?;
                    return self.terminal.flush().await;
                }
                super::PromptLine::Entered(answer) => answer,
                super::PromptLine::Aborted => return self.terminal.flush().await,
                super::PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
            }
        };

        // Resolve the answer to a span (getDirSpan, express.e:26881-26908);
        // unrecognised / out-of-range is "No such directory." (`:26905`).
        // No `.trim()`: `prompt_line` already trims its answers, and an
        // inline token arrives whitespace-split.
        let Some(span) = resolve_zippy_span(&answer, max) else {
            return self.zippy_no_such_directory().await;
        };

        // express.e:26172 — blank after a successful getDirSpan.
        self.terminal.write(CRLF).await?;

        let needle = query.to_ascii_uppercase().into_bytes();
        match span {
            ZippySpan::Hold => {
                self.terminal.write(wire::ZIPPY_SCANNING_HOLD).await?;
                self.terminal.write(CRLF).await?;
                let files = self.held_files(conference);
                self.zippy_dump_matches(&files, &needle).await?;
            }
            ZippySpan::Dirs(dirs) => {
                for dir in dirs {
                    self.terminal
                        .write(&wire::zippy_scanning_dir_header(dir))
                        .await?;
                    self.terminal.write(CRLF).await?;
                    let files = self.files_in_area(FileAreaRef::new(conference, dir));
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
    // Token recognition is the shared item-17 resolver; the dense
    // `1..=max` expansion and the internal range check stay here (the
    // `F` span expands `All` via catalogue area numbers instead —
    // divergent on sparse sets — and answers out-of-range dirs with its
    // own highest-dir envelope).
    match crate::app::menu_command::parse_span_token(answer)? {
        FileSpan::All => Some(ZippySpan::Dirs((1..=max).collect())),
        FileSpan::Upload => Some(ZippySpan::Dirs(vec![max])),
        FileSpan::Hold => Some(ZippySpan::Hold),
        FileSpan::Dir(requested) => {
            if requested < 1 || requested > i64::from(max) {
                None
            } else {
                Some(ZippySpan::Dirs(vec![
                    u32::try_from(requested).expect("range-checked above")
                ]))
            }
        }
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

#[cfg(test)]
mod tests;
