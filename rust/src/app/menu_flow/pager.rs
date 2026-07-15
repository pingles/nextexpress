//! Output pager — the legacy `checkForPause` `[more]`-style pagination
//! (`amiexpress/express.e:5181`) used by message and file listings.
//!
//! A [`Pager`] tracks how many lines have scrolled since the last pause.
//! After each emitted line the caller invokes
//! [`MenuFlow::page_break`][pb]; once the page fills it prints the legacy
//! `(Pause)...More(y/n/ns)? ` prompt and reads the reader's choice — `n`
//! aborts the listing, `ns` switches to non-stop (no further pauses),
//! and anything else continues.
//!
//! [pb]: super::MenuFlow::page_break

use crate::app::terminal::{Terminal, TerminalEcho};

/// Per-listing pagination state.
pub(super) struct Pager {
    /// Lines scrolled since the last pause.
    lines: u32,
    /// Page height — the reader's `userLineLen`, defaulting to 22.
    page_length: u32,
    /// Set once the reader answers `ns`; suppresses all further pauses.
    non_stop: bool,
}

/// What the caller should do after a [`MenuFlow::page_break`][pb].
///
/// [pb]: super::MenuFlow::page_break
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PageBreak {
    /// Keep listing.
    Continue,
    /// The reader asked to stop (`n`). Connection exits propagate separately.
    Abort,
}

impl Pager {
    /// Builds a pager for a reader whose terminal shows `user_line_len`
    /// lines per page (legacy `userLineLen`, defaulting to 22 when unset
    /// — `express.e:5188`).
    pub(super) fn new(user_line_len: u32) -> Self {
        Self {
            lines: 0,
            page_length: if user_line_len == 0 {
                22
            } else {
                user_line_len
            },
            non_stop: false,
        }
    }

    /// Adds `lines` to the counter without pausing — used for a
    /// multi-line block such as a table header (`express.e:8860`).
    pub(super) fn add_lines(&mut self, lines: u32) {
        if !self.non_stop {
            self.lines += lines;
        }
    }
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Counts one emitted line and, when the page is full, prints the
    /// legacy pause prompt and reads the reader's choice
    /// (`express.e:5190-5200`). Returns whether the caller should keep
    /// listing.
    pub(super) async fn page_break(
        &mut self,
        pager: &mut Pager,
    ) -> crate::app::menu_flow::MenuFlowResult<PageBreak, T::Error> {
        if pager.non_stop {
            return Ok(PageBreak::Continue);
        }
        pager.lines += 1;
        if pager.lines < pager.page_length {
            return Ok(PageBreak::Continue);
        }
        pager.lines = 0;
        let answer = self
            .read_prompted(b"(Pause)...More(y/n/ns)? ", TerminalEcho::Visible)
            .await?;
        let mut chars = answer.trim().chars();
        if matches!(chars.next(), Some('n' | 'N')) {
            if matches!(chars.next(), Some('s' | 'S')) {
                pager.non_stop = true;
            } else {
                // `n` aborts and leaves the prompt on screen — the legacy
                // returns immediately at `express.e:5197`, before the
                // erase.
                return Ok(PageBreak::Abort);
            }
        }
        // Erase the pause prompt line (`express.e:5199`, `[1A[K`).
        self.write_and_flush(b"\x1b[1A\x1b[K").await?;
        Ok(PageBreak::Continue)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::time::Duration;

    use super::{PageBreak, Pager};
    use crate::app::menu_flow::test_support::test_services;
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};

    #[derive(Default)]
    struct CaptureTerminal {
        output: Vec<u8>,
        inputs: VecDeque<TerminalRead>,
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

    fn line(text: &str) -> TerminalRead {
        TerminalRead::Line(text.to_string())
    }

    const PAUSE_PROMPT: &[u8] = b"(Pause)...More(y/n/ns)? ";
    const ERASE: &[u8] = b"\x1b[1A\x1b[K";

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[tokio::test]
    async fn pauses_only_once_the_page_fills_then_continues() {
        let services = test_services();
        let mut terminal = CaptureTerminal {
            inputs: VecDeque::from([line("y")]),
            ..Default::default()
        };
        let outcome = {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            let mut pager = Pager::new(3);
            // Two lines fit the page (no prompt yet); the third fills it.
            assert_eq!(
                flow.page_break(&mut pager).await.unwrap(),
                PageBreak::Continue
            );
            assert_eq!(
                flow.page_break(&mut pager).await.unwrap(),
                PageBreak::Continue
            );
            assert!(
                !contains(&flow.terminal.output, PAUSE_PROMPT),
                "must not pause before the page fills"
            );
            flow.page_break(&mut pager).await.unwrap()
        };
        assert_eq!(outcome, PageBreak::Continue, "`y` keeps listing");
        assert!(
            contains(&terminal.output, PAUSE_PROMPT),
            "page-full must pause"
        );
        assert!(
            contains(&terminal.output, ERASE),
            "the prompt must be erased"
        );
    }

    #[tokio::test]
    async fn n_aborts_without_erasing_the_prompt() {
        let services = test_services();
        let mut terminal = CaptureTerminal {
            inputs: VecDeque::from([line("n")]),
            ..Default::default()
        };
        let outcome = {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            let mut pager = Pager::new(1);
            flow.page_break(&mut pager).await.unwrap()
        };
        assert_eq!(outcome, PageBreak::Abort, "`n` stops the listing");
        assert!(contains(&terminal.output, PAUSE_PROMPT));
        assert!(
            !contains(&terminal.output, ERASE),
            "an aborting `n` leaves the prompt on screen (express.e:5197)"
        );
    }

    #[tokio::test]
    async fn ns_goes_non_stop_and_suppresses_later_pauses() {
        let services = test_services();
        let mut terminal = CaptureTerminal {
            inputs: VecDeque::from([line("ns")]),
            ..Default::default()
        };
        let prompt_count = {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            let mut pager = Pager::new(1);
            // First line fills the one-line page and prompts; `ns` sets
            // non-stop, so the next several lines never pause again.
            assert_eq!(
                flow.page_break(&mut pager).await.unwrap(),
                PageBreak::Continue
            );
            for _ in 0..3 {
                assert_eq!(
                    flow.page_break(&mut pager).await.unwrap(),
                    PageBreak::Continue
                );
            }
            flow.terminal
                .output
                .windows(PAUSE_PROMPT.len())
                .filter(|w| *w == PAUSE_PROMPT)
                .count()
        };
        assert_eq!(prompt_count, 1, "`ns` must pause exactly once");
    }

    #[tokio::test]
    async fn add_lines_brings_the_pause_forward() {
        let services = test_services();
        let mut terminal = CaptureTerminal {
            inputs: VecDeque::from([line("y")]),
            ..Default::default()
        };
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            let mut pager = Pager::new(3);
            // Pre-charging two header lines means a single content line
            // fills the three-line page and pauses — without the
            // pre-charge that first `page_break` would not pause.
            pager.add_lines(2);
            assert_eq!(
                flow.page_break(&mut pager).await.unwrap(),
                PageBreak::Continue
            );
        }
        assert!(
            contains(&terminal.output, PAUSE_PROMPT),
            "add_lines must count toward the page so the pause arrives early"
        );
    }
}
