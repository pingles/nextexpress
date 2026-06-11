//! The `F` command — `NextScan` file listings (slice D2).
//!
//! Parity target: the `AquaScan` v1.0 door experience with `NextScan`
//! branding (`comparison/evidence-tierD/live-observations.md`;
//! cleanest captures in `comparison/transcripts/ae_tierd_aquascan3.txt`).
//! The shadowed internal `internalCommandF`
//! (`amiexpress/express.e:24877`) is the stock diff record only.

mod dir_row;
mod wire;

use crate::app::menu_command::{FileListArg, FileSpan};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
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
            FileListArg::Span { span, non_stop } => {
                self.file_list_span(session, span, non_stop).await
            }
            FileListArg::Prompt => self.file_list_prompt(session).await,
            FileListArg::Help => {
                // `F ?` (`ae_tierd_aquascan3.txt` S1).
                self.terminal.write(wire::HELP_SCREEN).await?;
                self.terminal.flush().await
            }
        }
    }

    /// Bare `F`: the door's own
    /// `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=None ? `
    /// line prompt (`ae_tierd_aquascan3.txt:163`; Visible read — the
    /// answer echo is the adapter's). Enter aborts silently; junk
    /// answers `Error in input!`; valid answers run the same spans as
    /// arguments with no banner re-emit (S2/S3, A2, U5–U7).
    async fn file_list_prompt(&mut self, session: &mut MenuSession) -> Result<(), T::Error> {
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.services.file_repo.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);
        let mut state = ScanState::new(false);

        for line in [&b"\x1b[0m"[..], wire::LISTING_BANNER, b""] {
            if self.emit_scan_line(&mut state, line).await? == ScanFlow::Quit {
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
            self.terminal.write(b"\r\n").await?;
            self.terminal.write(wire::ERROR_IN_INPUT).await?;
            self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
            return self.terminal.flush().await;
        };
        self.terminal.write(b"\r\n").await?;
        self.run_span(&mut state, conference, span, max, &areas)
            .await
    }

    /// `Argument error! Type 'f ?' for help.` under the help banner —
    /// the captured response to unsupported argument forms
    /// (`ae_tierd_aquascan4.txt` U4; single-reset tail).
    async fn file_list_argument_error(&mut self) -> Result<(), T::Error> {
        self.terminal.write(b"\x1b[0m\r\n").await?;
        self.terminal.write(wire::HELP_BANNER).await?;
        self.terminal.write(b"\r\n\r\n").await?;
        self.terminal.write(wire::ARGUMENT_ERROR).await?;
        self.terminal.write(b"\r\n\r\n\x1b[0m\r\n").await?;
        self.terminal.flush().await
    }

    /// Runs an immediate scan over `span`'s directories.
    async fn file_list_span(
        &mut self,
        session: &mut MenuSession,
        span: FileSpan,
        non_stop: bool,
    ) -> Result<(), T::Error> {
        // Per-task session isolation: the menu loop guarantees a
        // joined conference before any command dispatches.
        let conference = session.current_conference_number().unwrap_or(0);
        let areas = self.services.file_repo.areas_in_conference(conference);
        let max = areas.last().map_or(0, FileArea::number);
        let mut state = ScanState::new(non_stop);

        // Entry preamble — every argument form (§1.1). Counted: the
        // captured page-1 More? boundary includes these lines.
        for line in [&b"\x1b[0m"[..], wire::LISTING_BANNER, b""] {
            if self.emit_scan_line(&mut state, line).await? == ScanFlow::Quit {
                return self.finish_listing().await;
            }
        }
        self.run_span(&mut state, conference, span, max, &areas)
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
        max: u32,
        areas: &[FileArea],
    ) -> Result<(), T::Error> {
        let dirs: Vec<u32> = match span {
            FileSpan::Dir(n) => {
                if n < 1 || n > i64::from(max) {
                    self.terminal.write(&wire::highest_dir_error(max)).await?;
                    self.terminal.write(b"\r\n").await?;
                    return self.finish_listing().await;
                }
                vec![u32::try_from(n).expect("range-checked above")]
            }
            FileSpan::All => areas.iter().map(FileArea::number).collect(),
            FileSpan::Upload => vec![max],
            FileSpan::Hold => {
                let held = self.services.file_repo.list_held(conference);
                let header = wire::scanning_hold_header(!held.is_empty());
                if self.emit_scan_line(state, &header).await? == ScanFlow::Quit {
                    return self.finish_listing().await;
                }
                if !held.is_empty()
                    && self.stream_dir_body(state, &held).await? == ScanFlow::Continue
                {
                    // Hold is a single-dir span: whatever the
                    // post-End verb says, the listing ends here.
                    let _ = self.post_end_pause(state).await?;
                }
                return self.finish_listing().await;
            }
        };

        for (index, dir) in dirs.iter().enumerate() {
            let files = self.services.file_repo.find_in_area(conference, *dir);
            let header = wire::scanning_dir_header(*dir, !files.is_empty());
            if self.emit_scan_line(state, &header).await? == ScanFlow::Quit {
                return self.finish_listing().await;
            }
            if files.is_empty() {
                // A Nothing-found dir runs straight into the next
                // header — no blank, no More? between
                // (ae_tierd_aquascan5.txt V1).
                continue;
            }
            if self.stream_dir_body(state, &files).await? == ScanFlow::Quit {
                return self.finish_listing().await;
            }
            if self.post_end_pause(state).await? == ScanFlow::Quit {
                return self.finish_listing().await;
            }
            if index + 1 < dirs.len() {
                // Y at a non-last dir's post-End More?: the verb's
                // overprint clear, then CRLF, then the next Scanning
                // header (ae_tierd_aquascan3.txt S8 repr :673).
                self.terminal.write(b"\r\n").await?;
            }
        }
        self.finish_listing().await
    }

    /// The unconditional post-`End of File List` `More?` of paged
    /// mode (`ae_tierd_aquascan3.txt:157-158`; suppressed entirely in
    /// non-stop mode, S7 repr :490). Resets the page counter on
    /// resume — each dir pages afresh.
    async fn post_end_pause(&mut self, state: &mut ScanState) -> Result<ScanFlow, T::Error> {
        if state.non_stop {
            return Ok(ScanFlow::Continue);
        }
        let flow = self.scan_more_prompt(state).await?;
        state.emitted = 0;
        Ok(flow)
    }

    /// The blank line after the scan header, then the assembled body,
    /// through the counting pager.
    async fn stream_dir_body(
        &mut self,
        state: &mut ScanState,
        files: &[File],
    ) -> Result<ScanFlow, T::Error> {
        if self.emit_scan_line(state, b"").await? == ScanFlow::Quit {
            return Ok(ScanFlow::Quit);
        }
        for line in wire::assemble_dir_lines(files) {
            if self.emit_scan_line(state, &line).await? == ScanFlow::Quit {
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
        line: &[u8],
    ) -> Result<ScanFlow, T::Error> {
        self.terminal.write(line).await?;
        self.terminal.write(b"\r\n").await?;
        if state.non_stop {
            return Ok(ScanFlow::Continue);
        }
        if state.emitted == 0 {
            state.page.clear();
        }
        state.page.push(line.to_vec());
        state.emitted += 1;
        if state.emitted < PAGE_LINES {
            return Ok(ScanFlow::Continue);
        }
        state.emitted = 0;
        self.scan_more_prompt(state).await
    }

    /// One `More?` interaction (`ae_tierd_aquascan3.txt:158`): Silent
    /// line reads with handler-emitted echoes.
    ///
    /// - `Q` echoes `Quit` and quits (`:321`).
    /// - `C` form-feeds and resumes — no clear, no re-prompt
    ///   (`:292-321`).
    /// - `n` echoes and **holds**: it is ambiguous between `N` (=
    ///   Quit, per the in-pager help) and the `ns` prefix, so the
    ///   door waits; the next input first erases it with BS-SP-BS,
    ///   then runs as its own verb (`ae_tierd_aquascan4.txt` U1,
    ///   `ae_tierd_aquascan3.txt` S2 — identical mid-list and
    ///   post-End).
    /// - `ns` echoes the `n`, clears, and asks the Are-you-sure
    ///   confirm: `Y` (unechoed) clears and switches to non-stop;
    ///   declining clears and redraws `More?` (`ae_tierd_aquascan4.txt`
    ///   U3).
    /// - `Y`, empty and unknown keys clear with the captured
    ///   69-space overprint and resume.
    async fn scan_more_prompt(&mut self, state: &mut ScanState) -> Result<ScanFlow, T::Error> {
        let mut held_n = false;
        let mut show_prompt = true;
        loop {
            let prompt: &[u8] = if show_prompt { wire::MORE_PROMPT } else { b"" };
            let read = self.read_prompted(prompt, TerminalEcho::Silent).await?;
            let TerminalRead::Line(answer) = read else {
                // Carrier loss / idle at the pager aborts the listing.
                return Ok(ScanFlow::Quit);
            };
            let verb = answer.trim().to_ascii_uppercase();
            if held_n {
                self.terminal.write(b"\x08 \x08").await?;
                held_n = false;
            }
            match verb.as_str() {
                "N" => {
                    self.terminal.write(b"n").await?;
                    self.terminal.flush().await?;
                    held_n = true;
                    show_prompt = false;
                }
                "NS" => {
                    self.terminal.write(b"n").await?;
                    self.terminal.write(&more_overprint_clear()).await?;
                    let confirm = self
                        .read_prompted(wire::NS_CONFIRM_PROMPT, TerminalEcho::Silent)
                        .await?;
                    let TerminalRead::Line(confirm) = confirm else {
                        return Ok(ScanFlow::Quit);
                    };
                    self.terminal.write(&more_overprint_clear()).await?;
                    if confirm.trim().eq_ignore_ascii_case("Y") {
                        state.non_stop = true;
                        return Ok(ScanFlow::Continue);
                    }
                    show_prompt = true;
                }
                "Q" => {
                    self.terminal.write(b"Quit\r\n").await?;
                    return Ok(ScanFlow::Quit);
                }
                "C" => {
                    self.terminal.write(b"\r\x0c").await?;
                    return Ok(ScanFlow::Continue);
                }
                "F" | "R" => {
                    // Flagging is silent in the captures
                    // (`ae_tierd_aquascan3.txt` S4/S5): the entry is
                    // echoed verbatim, cleared with the wider
                    // overprint, and More? redraws. The input is read
                    // and discarded until slice D5 wires FlaggedFile —
                    // wire-identical either way.
                    let prompt = if verb == "F" {
                        wire::FLAG_BY_NAME_PROMPT
                    } else {
                        wire::FLAG_BY_NUMBER_PROMPT
                    };
                    self.terminal.write(&more_overprint_clear()).await?;
                    let entry = self.read_prompted(prompt, TerminalEcho::Silent).await?;
                    let TerminalRead::Line(entry) = entry else {
                        return Ok(ScanFlow::Quit);
                    };
                    self.terminal.write(entry.as_bytes()).await?;
                    self.terminal.write(&flag_overprint_clear()).await?;
                    show_prompt = true;
                }
                "?" => {
                    // The in-pager pause help, then a redraw of the
                    // current page (`ae_tierd_aquascan4.txt` U2; the
                    // door's redraw window drifts with its internal
                    // paging — NextScan redraws exactly the lines it
                    // showed, a documented COSMETIC divergence).
                    self.terminal.write(wire::PAUSE_HELP).await?;
                    let page = state.page.clone();
                    for line in &page {
                        self.terminal.write(line).await?;
                        self.terminal.write(b"\r\n").await?;
                    }
                    show_prompt = true;
                }
                _ => {
                    self.terminal.write(&more_overprint_clear()).await?;
                    return Ok(ScanFlow::Continue);
                }
            }
        }
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

/// Per-span pager state.
struct ScanState {
    /// Lines emitted since the last page boundary.
    emitted: u32,
    /// `NS` requested — no pauses at all.
    non_stop: bool,
    /// The current page's lines, for the `?` help's page redraw.
    page: Vec<Vec<u8>>,
}

impl ScanState {
    fn new(non_stop: bool) -> Self {
        Self {
            emitted: 0,
            non_stop,
            page: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::file_screen_repository::FileScreenRepository;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::menu_command::{FileListArg, FileSpan};
    use crate::app::seed;
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
    use crate::domain::conference::{Conference, ConferenceMembership, MessageBase};
    use crate::domain::password::{PasswordHashKind, PasswordHasher};
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::{RatioMode, User};

    struct CaptureTerminal {
        output: Vec<u8>,
        inputs: VecDeque<TerminalRead>,
    }

    impl CaptureTerminal {
        fn new(inputs: Vec<TerminalRead>) -> Self {
            Self {
                output: Vec::new(),
                inputs: inputs.into(),
            }
        }
    }

    impl Terminal for CaptureTerminal {
        type Error = Infallible;

        fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
            Box::pin(async move {
                self.output.extend_from_slice(bytes);
                Ok(())
            })
        }

        fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
            Box::pin(async { Ok(()) })
        }

        fn read_line(
            &mut self,
            _echo: TerminalEcho,
            _timeout: Duration,
        ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
            Box::pin(async move { Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof)) })
        }
    }

    fn conference(number: u32) -> Conference {
        Conference::new(
            number,
            format!("Conf {number}"),
            vec![MessageBase::new(number, 1, "main".to_string())],
        )
        .expect("valid conference")
    }

    fn test_user() -> User {
        let hasher = Pbkdf2PasswordHasher::new();
        let computed = hasher
            .compute_password_hash("pw", PasswordHashKind::Pbkdf210000)
            .expect("hash");
        let mut user = User::new(
            2,
            "alice".to_string(),
            PasswordHashKind::Pbkdf210000,
            computed.hash,
            computed.salt,
            SystemTime::UNIX_EPOCH,
            255,
        )
        .expect("valid user");
        user.upsert_membership(ConferenceMembership::new(1, true));
        user
    }

    fn menu_session() -> MenuSession {
        let conferences = vec![conference(1)];
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("alice", test_user())
            .expect("identify");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("password match");
        session
            .auto_rejoin_conference(&conferences, SystemTime::UNIX_EPOCH)
            .expect("rejoin");
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        MenuSession::from_session(session)
    }

    /// Services whose file catalogue is the seeded demo corpus in
    /// conference 1 (area 1: 27 files, area 2: 3 files).
    fn services_with_demo_catalogue() -> AppServices {
        let conferences = vec![conference(1)];
        let (areas, files) = seed::demo_file_catalogue(&conferences);
        services_with(InMemoryFileRepository::new(areas, files))
    }

    fn services_with(file_repo: InMemoryFileRepository) -> AppServices {
        AppServices {
            user_repo: Arc::new(InMemoryUserRepository::default()),
            hasher: Arc::new(Pbkdf2PasswordHasher::new()),
            caller_log: Arc::new(InMemoryCallerLog::new()),
            screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
            conferences: Arc::new(vec![conference(1)]),
            mail_stores: Arc::new(InMemoryMailStores::new()),
            file_repo: Arc::new(file_repo),
            session_policy: SessionPolicy::default(),
            default_ratio: DefaultRatio {
                mode: RatioMode::Disabled,
                value: 0,
            },
            new_user_gate: Arc::new(NewUserGateConfig {
                allow_new_users: true,
                new_user_password: None,
                max_new_user_password_attempts: 3,
            }),
            bbs_name: Arc::from("Test BBS"),
        }
    }

    async fn run_file_list(
        services: &AppServices,
        terminal: &mut CaptureTerminal,
        arg: FileListArg,
    ) {
        let mut session = menu_session();
        let mut flow = super::super::MenuFlow { terminal, services };
        flow.handle_file_list(&mut session, arg)
            .await
            .expect("file list");
    }

    /// `\x1b[0m\r\n` + listing banner + blank — §1.1's entry preamble
    /// for every argument form (`ae_tierd_aquascan3.txt:163/217`).
    fn listing_preamble() -> Vec<u8> {
        let mut bytes = b"\x1b[0m\r\n".to_vec();
        bytes.extend_from_slice(super::wire::LISTING_BANNER);
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes
    }

    const EXIT_TAIL: &[u8] = b"\x1b[0m\r\n\x1b[0m\r\n";

    #[tokio::test]
    async fn f_99_emits_the_highest_dir_error() {
        // ae_tierd_aquascan.txt:330-342 (A7), max flexed to the
        // conference's area count.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(99),
                non_stop: false,
            },
        )
        .await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(b"The highest directory number is 2!\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(terminal.output, expected);
    }

    #[tokio::test]
    async fn f_0_takes_the_highest_dir_error_unverified() {
        // UNVERIFIED: `F 0` was not captured live; the dispatch range
        // check `1..=max` routes it with the out-of-range arguments.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(0),
                non_stop: false,
            },
        )
        .await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(b"The highest directory number is 2!\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(terminal.output, expected);
    }

    #[tokio::test]
    async fn invalid_arguments_emit_argument_error_under_the_help_banner() {
        // ae_tierd_aquascan4.txt U4: the help-banner variant (no
        // form-feed) + the argument error + a single-reset tail.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(&services, &mut terminal, FileListArg::Invalid).await;
        let mut expected = b"\x1b[0m\r\n".to_vec();
        expected.extend_from_slice(super::wire::HELP_BANNER);
        expected.extend_from_slice(b"\r\n\r\n");
        expected.extend_from_slice(b"Argument error! Type 'f ?' for help.\r\n");
        expected.extend_from_slice(b"\r\n\x1b[0m\r\n");
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn f_2_ns_streams_the_trio_without_pausing() {
        // The S7-shape non-stop run over the captured Dir2 trio: scan
        // header, blank, assembled body, two-reset tail — and no
        // More? prompt anywhere.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(2),
                non_stop: true,
            },
        )
        .await;
        let conferences = vec![conference(1)];
        let (_, placements) = seed::demo_file_catalogue(&conferences);
        let trio: Vec<crate::domain::files::file::File> = placements
            .into_iter()
            .filter(|(_, area, _)| *area == 2)
            .map(|(_, _, f)| f)
            .collect();
        let mut expected = listing_preamble();
        expected.extend_from_slice(b"Scanning dir 2 from top... Ok!\r\n\r\n");
        for line in super::wire::assemble_dir_lines(&trio) {
            expected.extend_from_slice(&line);
            expected.extend_from_slice(b"\r\n");
        }
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    /// Every `\r\n`-terminated line the `F 1` span emits before its
    /// body pauses: reset-blank, banner, blank, scan header, blank,
    /// then the assembled dir-1 lines.
    fn f_1_emitted_lines(services: &AppServices) -> Vec<Vec<u8>> {
        let mut lines: Vec<Vec<u8>> = vec![
            b"\x1b[0m".to_vec(),
            super::wire::LISTING_BANNER.to_vec(),
            Vec::new(),
            b"Scanning dir 1 from top... Ok!".to_vec(),
            Vec::new(),
        ];
        lines.extend(super::wire::assemble_dir_lines(
            &services.file_repo.find_in_area(1, 1),
        ));
        lines
    }

    fn joined(lines: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for line in lines {
            bytes.extend_from_slice(line);
            bytes.extend_from_slice(b"\r\n");
        }
        bytes
    }

    /// `\r` + 69 spaces + `\r` — the captured `More?`/ns-confirm
    /// overprint clear (counted programmatically from the
    /// transcripts).
    fn more_clear() -> Vec<u8> {
        let mut bytes = vec![b'\r'];
        bytes.extend(std::iter::repeat_n(b' ', 69));
        bytes.push(b'\r');
        bytes
    }

    #[tokio::test]
    async fn paged_f_1_pauses_at_29_lines_and_q_quits() {
        // The captured page-1 boundary (ae_tierd_aquascan3.txt:212,
        // S4): More? fires after exactly 29 emitted lines — right
        // after the 02-03-26 separator block — and `Q` echoes `Quit`
        // with no clear, then the listing exit tail.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![TerminalRead::Line("Q".to_string())]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn y_at_more_clears_the_prompt_and_streams_a_fresh_page() {
        // `Y` (ae_tierd_aquascan3.txt S4): overprint clear, the
        // counter resets, and the next 29 lines stream to the next
        // More?.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(&joined(&lines[29..58]));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn c_at_more_form_feeds_and_resumes_without_reprompt() {
        // `C` (ae_tierd_aquascan3.txt:292-321, S6): `\r` + form feed,
        // no overprint clear, no prompt redraw — the listing resumes
        // immediately with a reset counter.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("C".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"\r\x0c");
        expected.extend_from_slice(&joined(&lines[29..58]));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn ns_at_more_confirms_then_streams_the_remainder_non_stop() {
        // ae_tierd_aquascan3.txt S7 (:361 + repr :490): `ns` echoes
        // just the `n`, clears, asks the Are-you-sure confirm; `Y`
        // (unechoed) clears and the rest of the listing streams with
        // no further More? — footer straight to the exit tail.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("ns".to_string()),
            TerminalRead::Line("Y".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"n");
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(super::wire::NS_CONFIRM_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(&joined(&lines[29..]));
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn declining_the_ns_confirm_redraws_more_and_stays_paged() {
        // ae_tierd_aquascan4.txt U3: `n` at the confirm (unechoed)
        // clears and More? redraws; paged mode continues.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("ns".to_string()),
            TerminalRead::Line("n".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"n");
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(super::wire::NS_CONFIRM_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn lone_n_at_more_is_held_and_erased_by_the_next_verb() {
        // ae_tierd_aquascan4.txt U1: `n` echoes and holds (ambiguous
        // N=Quit / ns prefix); the next input first erases it with
        // BS-SP-BS, then runs — `n` then `Q` gives `n` … `\x08 \x08Quit`.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("n".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"n");
        expected.extend_from_slice(b"\x08 \x08");
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    /// `\r` + 79 spaces + `\r` — the wider overprint after a flag
    /// entry (counted from `ae_tierd_aquascan3.txt` S4).
    fn flag_clear() -> Vec<u8> {
        let mut bytes = vec![b'\r'];
        bytes.extend(std::iter::repeat_n(b' ', 79));
        bytes.push(b'\r');
        bytes
    }

    #[tokio::test]
    async fn f_at_more_opens_the_flag_by_name_prompt_and_discards_silently() {
        // ae_tierd_aquascan3.txt S4 (:212-217): clear, the line-read
        // flag prompt, the input echoed verbatim with NO trailing
        // CRLF, the wider clear, More? redrawn — and no confirmation
        // text anywhere (flag state itself lands with slice D5).
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("F".to_string()),
            TerminalRead::Line("TERMV48.LHA".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(super::wire::FLAG_BY_NAME_PROMPT);
        expected.extend_from_slice(b"TERMV48.LHA");
        expected.extend_from_slice(&flag_clear());
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn r_at_more_opens_the_distinct_flag_by_number_prompt() {
        // ae_tierd_aquascan3.txt S5 (:252-257): `R` uses the
        // `File number(s) to flag:` wording.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("R".to_string()),
            TerminalRead::Line("2".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(super::wire::FLAG_BY_NUMBER_PROMPT);
        expected.extend_from_slice(b"2");
        expected.extend_from_slice(&flag_clear());
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn question_mark_at_more_shows_the_pause_help_and_redraws_the_page() {
        // ae_tierd_aquascan4.txt U2: `?` emits the in-pager pause
        // help (byte-exact, incl. the trailing `~SP|`+FF marker)
        // followed by a page redraw and More? again. COSMETIC
        // divergence: NextScan redraws exactly the lines of the
        // current page; the door redraws a drifted window (its
        // internal paging quirk — designs/NEXTSCAN.md §1.5/§9).
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("?".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let lines = f_1_emitted_lines(&services);
        let mut expected = joined(&lines[..29]);
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(super::wire::PAUSE_HELP);
        expected.extend_from_slice(&joined(&lines[..29]));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    /// A small two-area catalogue (1 file each) for choreography
    /// tests that must not hit the 29-line page boundary.
    fn services_with_two_small_areas() -> AppServices {
        use crate::domain::bytes::Bytes;
        use crate::domain::files::area::FileArea;
        use crate::domain::files::file::{File, FileStatus};
        let file = |name: &str| {
            File::new(
                name.to_string(),
                Bytes::new(1_000),
                FileStatus::Available,
                Some(b'P'),
                format!("{name} description"),
                SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
            )
        };
        services_with(InMemoryFileRepository::new(
            vec![
                FileArea::new(1, 1, "Main".to_string()),
                FileArea::new(1, 2, "Uploads".to_string()),
            ],
            vec![(1, 1, file("FIRST.LHA")), (1, 2, file("SECOND.LHA"))],
        ))
    }

    fn area_lines(services: &AppServices, area: u32) -> Vec<Vec<u8>> {
        super::wire::assemble_dir_lines(&services.file_repo.find_in_area(1, area))
    }

    #[tokio::test]
    async fn paged_listing_shows_the_post_end_more_and_held_n_then_q_exits() {
        // ae_tierd_aquascan3.txt S2 + :158-163: the More? appears
        // right after the footer even for a listing far below a
        // page; `n` is held and the following `Q` erases it —
        // `n` … `\x08 \x08Quit`.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("n".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(2),
                non_stop: false,
            },
        )
        .await;
        let mut expected = joined(&[
            b"\x1b[0m".to_vec(),
            super::wire::LISTING_BANNER.to_vec(),
            Vec::new(),
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]);
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"n");
        expected.extend_from_slice(b"\x08 \x08");
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn f_a_transitions_between_dirs_through_the_post_end_more() {
        // ae_tierd_aquascan3.txt S8 (repr :673): every non-empty dir
        // gets its own footer + post-End More?; `Y` at a non-last dir
        // clears, emits CRLF and the next Scanning header; `Y` at the
        // last dir clears straight into the exit tail
        // (ae_tierd_aquascan5.txt V1).
        let services = services_with_two_small_areas();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("Y".to_string()),
            TerminalRead::Line("Y".to_string()),
        ]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::All,
                non_stop: false,
            },
        )
        .await;
        let mut expected = joined(&[
            b"\x1b[0m".to_vec(),
            super::wire::LISTING_BANNER.to_vec(),
            Vec::new(),
            b"Scanning dir 1 from top... Ok!".to_vec(),
            Vec::new(),
        ]);
        expected.extend_from_slice(&joined(&area_lines(&services, 1)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(b"\r\n");
        expected.extend_from_slice(&joined(&[
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]));
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn f_a_with_an_empty_first_dir_runs_its_headers_back_to_back() {
        // ae_tierd_aquascan5.txt V1: the empty dir emits exactly its
        // Nothing-found line with the next dir's Scanning line
        // directly beneath — no blank, no More? between.
        use crate::domain::files::area::FileArea;
        let services = {
            use crate::domain::bytes::Bytes;
            use crate::domain::files::file::{File, FileStatus};
            services_with(InMemoryFileRepository::new(
                vec![
                    FileArea::new(1, 1, "Main".to_string()),
                    FileArea::new(1, 2, "Uploads".to_string()),
                ],
                vec![(
                    1,
                    2,
                    File::new(
                        "ONLY.LHA".to_string(),
                        Bytes::new(1_000),
                        FileStatus::Available,
                        Some(b'P'),
                        "Only file".to_string(),
                        SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
                    ),
                )],
            ))
        };
        let mut terminal = CaptureTerminal::new(vec![TerminalRead::Line("Y".to_string())]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::All,
                non_stop: false,
            },
        )
        .await;
        let mut expected = joined(&[
            b"\x1b[0m".to_vec(),
            super::wire::LISTING_BANNER.to_vec(),
            Vec::new(),
            b"Scanning dir 1 from top... Nothing found!".to_vec(),
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]);
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn bare_f_prompts_and_enter_aborts_with_a_single_reset() {
        // ae_tierd_aquascan3.txt S3 (:165-177): the door's own
        // Directories prompt; Enter alone aborts — blank, ONE reset,
        // menu (the per-path tail asymmetry).
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![TerminalRead::Line(String::new())]);
        run_file_list(&services, &mut terminal, FileListArg::Prompt).await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(&super::wire::directories_prompt(2));
        expected.extend_from_slice(b"\r\n\x1b[0m\r\n");
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn bare_f_junk_answer_errors_in_input() {
        // ae_tierd_aquascan.txt:109-120 (A2): a junk line at the
        // Directories prompt — blank, `Error in input!`, blank, ONE
        // reset, menu.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![TerminalRead::Line("XYZ".to_string())]);
        run_file_list(&services, &mut terminal, FileListArg::Prompt).await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(&super::wire::directories_prompt(2));
        expected.extend_from_slice(b"\r\nError in input!\r\n\r\n\x1b[0m\r\n");
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn bare_f_numeric_answer_scans_that_dir() {
        // ae_tierd_aquascan3.txt S2 (:131-163): `2` at the prompt —
        // blank, then the dir-2 scan with NO banner re-emit, through
        // the post-End More?.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("2".to_string()),
            TerminalRead::Line("Q".to_string()),
        ]);
        run_file_list(&services, &mut terminal, FileListArg::Prompt).await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(&super::wire::directories_prompt(2));
        expected.extend_from_slice(b"\r\n");
        expected.extend_from_slice(&joined(&[
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]));
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(b"Quit\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn bare_f_u_answer_scans_the_upload_dir() {
        // ae_tierd_aquascan4.txt U6: `U` at the prompt resolves to
        // the highest-numbered area.
        let services = services_with_two_small_areas();
        let mut terminal = CaptureTerminal::new(vec![
            TerminalRead::Line("U".to_string()),
            TerminalRead::Line("Y".to_string()),
        ]);
        run_file_list(&services, &mut terminal, FileListArg::Prompt).await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(&super::wire::directories_prompt(2));
        expected.extend_from_slice(b"\r\n");
        expected.extend_from_slice(&joined(&[
            b"Scanning dir 2 from top... Ok!".to_vec(),
            Vec::new(),
        ]));
        expected.extend_from_slice(&joined(&area_lines(&services, 2)));
        expected.extend_from_slice(super::wire::MORE_PROMPT);
        expected.extend_from_slice(&more_clear());
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(&expected),
        );
    }

    #[tokio::test]
    async fn paged_hold_listing_quits_cleanly_at_the_mid_list_more() {
        // Held files render through the same framed body as normal
        // listings (no live capture exists with held files — the
        // seed board held none — so this pins the unit-level
        // inference). A `Q` at the mid-list More? must end the
        // listing with the Quit echo and the exit tail and nothing
        // else: quitting must skip the post-End pause entirely.
        use crate::domain::bytes::Bytes;
        use crate::domain::files::area::FileArea;
        use crate::domain::files::file::{File, FileStatus};
        let held: Vec<(u32, u32, File)> = (0u64..30)
            .map(|i| {
                (
                    1,
                    1,
                    File::new(
                        format!("HELD{i:02}.LHA"),
                        Bytes::new(1_000),
                        FileStatus::HeldForReview,
                        Some(b'P'),
                        format!("Held file {i}"),
                        SystemTime::from(time::macros::datetime!(2026-05-01 12:00 UTC))
                            + Duration::from_secs(86_400 * i),
                    ),
                )
            })
            .collect();
        let services = services_with(InMemoryFileRepository::new(
            vec![FileArea::new(1, 1, "Main".to_string())],
            held,
        ));
        let mut terminal = CaptureTerminal::new(vec![TerminalRead::Line("Q".to_string())]);
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Hold,
                non_stop: false,
            },
        )
        .await;
        let output = String::from_utf8_lossy(&terminal.output);
        assert!(
            output.contains("Scanning HOLD dir from top... Ok!"),
            "held files report Ok!: {output}",
        );
        assert!(
            output.contains("HELD00.LHA"),
            "held files render framed rows: {output}",
        );
        let mut quit_tail = b"Quit\r\n".to_vec();
        quit_tail.extend_from_slice(EXIT_TAIL);
        assert!(
            terminal.output.ends_with(&quit_tail),
            "Q ends the listing immediately — no post-End pause after a quit: {output}",
        );
        let more_count = terminal
            .output
            .windows(super::wire::MORE_PROMPT.len())
            .filter(|w| *w == super::wire::MORE_PROMPT)
            .count();
        assert_eq!(more_count, 1, "exactly one More? before the quit: {output}");
    }

    #[tokio::test]
    async fn f_help_shows_the_nextscan_help_screen() {
        // ae_tierd_aquascan3.txt S1 (:100-129): form feed, the
        // Copyright help banner, the verbatim syntax text (with the
        // `- Configure NextScan` swap), and the captured epilogue.
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(&services, &mut terminal, FileListArg::Help).await;
        assert_eq!(
            String::from_utf8_lossy(&terminal.output),
            String::from_utf8_lossy(super::wire::HELP_SCREEN),
        );
        assert!(
            terminal
                .output
                .windows(super::wire::HELP_BANNER.len())
                .any(|w| w == super::wire::HELP_BANNER),
            "the help screen embeds the help banner",
        );
    }

    #[tokio::test]
    async fn empty_dir_reports_nothing_found_with_no_footer() {
        // ae_tierd_aquascan.txt:515-527 (E2): the Nothing-found line
        // goes straight to the exit tail — no blank, no footer.
        let services = services_with(InMemoryFileRepository::new(
            vec![crate::domain::files::area::FileArea::new(
                1,
                1,
                "Main".to_string(),
            )],
            Vec::new(),
        ));
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
            },
        )
        .await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(b"Scanning dir 1 from top... Nothing found!\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(terminal.output, expected);
    }

    #[tokio::test]
    async fn hold_span_reports_nothing_found_when_no_files_are_held() {
        // ae_tierd_aquascan3.txt:675-687 (S9).
        let services = services_with_demo_catalogue();
        let mut terminal = CaptureTerminal::new(Vec::new());
        run_file_list(
            &services,
            &mut terminal,
            FileListArg::Span {
                span: FileSpan::Hold,
                non_stop: false,
            },
        )
        .await;
        let mut expected = listing_preamble();
        expected.extend_from_slice(b"Scanning HOLD dir from top... Nothing found!\r\n");
        expected.extend_from_slice(EXIT_TAIL);
        assert_eq!(terminal.output, expected);
    }
}
