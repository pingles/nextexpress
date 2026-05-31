//! `E` (Enter Mail) and `C` (Comment to Sysop) menu commands
//! (Slices 42 / 43 / 44).
//!
//! Drives the minimal line-mode editor (To: / Subject: / Private (y/N) /
//! body lines terminated by `.` on its own line), delegates the
//! terminal-free command effect to [`crate::app::menu::post_mail`],
//! then renders the outcome. The two handlers share the `Subject:`
//! and body editor prompts plus the wire-rendering of post outcomes.

use std::time::SystemTime;

use crate::app::input_limits::append_line_with_newline;
use crate::app::menu::post_mail::{
    post_comment_to_sysop, post_mail, CommentToSysopInput, PostMailInput, PostMailOutcome,
};
use crate::app::menu_command::PostArg;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::render_post_success;
use crate::app::wire_text::{
    MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, NO_SYSOP_LINE, POST_ABORTED_LINE,
    POST_ACCESS_DENIED_LINE, POST_ADDRESSING_NOT_ALLOWED_LINE, POST_BODY_PROMPT,
    POST_PRIVATE_PROMPT, POST_RECIPIENT_NO_ACCESS_LINE, POST_SUBJECT_PROMPT, POST_TO_PROMPT,
    POST_UNKNOWN_USER_LINE,
};
use crate::domain::messaging::limits::MAX_MAIL_BODY_BYTES;
use crate::domain::messaging::post_mail::PostMailError;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles an `E` / `E <to>` command from the menu (Slice 42).
    /// Drives the line-mode editor: prompts for the recipient (when
    /// not supplied inline), subject, private flag and body, resolves
    /// the addressee through the user repository, then calls the
    /// `PostMail` rule via the typed session.
    #[allow(clippy::too_many_lines)] // Cohesive: each step is a distinct editor prompt.
    pub(super) async fn handle_post_mail(
        &mut self,
        session: &mut MenuSession,
        arg: PostArg,
    ) -> Result<(), T::Error> {
        // Step 1: collect the recipient name. `E <to>` provides it
        // inline; bare `E` prompts. An empty prompt response reroutes
        // to ALL, mirroring legacy `enterMSG`
        // (`amiexpress/express.e:10827`) where the default address is
        // ALL when the user submits a blank `To:` line.
        let typed_to = match arg {
            PostArg::To(name) => name,
            PostArg::Missing => match self.read_optional_line(session, POST_TO_PROMPT).await? {
                Some(line) => line,
                // Idle or EOF — bail out cleanly.
                None => return Ok(()),
            },
        };

        // Step 2: subject prompt. Empty subject aborts (mirrors
        // `amiexpress/express.e:10854-10857`).
        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT, false)
            .await?
        else {
            return Ok(());
        };

        // Step 3: private flag. Default is N if the user just hits CR.
        // EALL forces public visibility regardless of the answer, but
        // the legacy still prompts and the rule will normalise the
        // value.
        let private = match self
            .read_prompted(POST_PRIVATE_PROMPT, TerminalEcho::Visible)
            .await?
        {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                matches!(line.trim().chars().next(), Some('y' | 'Y'))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                self.write_and_flush(POST_ABORTED_LINE).await?;
                return Ok(());
            }
        };

        // Step 4: body. Slice 42 ships a minimal line-mode editor —
        // each line is read until the user types `.` on its own line,
        // or `/A` to abort. The full editor (numbered line edits,
        // `/S` save, quoting) arrives in Phase 8.
        let Some(body) = self.read_post_body(session, false).await? else {
            return Ok(());
        };

        let outcome = post_mail(
            session,
            self.services.user_repo(),
            self.services.mail_stores(),
            self.services.conferences(),
            PostMailInput {
                typed_to,
                subject,
                private,
                body,
                posted_at: SystemTime::now(),
            },
        )
        .await;

        self.render_post_outcome(outcome, "E").await
    }

    /// Handles a `C` command from the menu (Slice 44). Resolves the
    /// sysop through the user repository, walks subject/body prompts
    /// (no recipient prompt, no private toggle — the rule fixes both),
    /// and invokes `messaging.allium:PostCommentToSysop` via the typed
    /// session.
    pub(super) async fn handle_comment_to_sysop(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<(), T::Error> {
        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT, false)
            .await?
        else {
            return Ok(());
        };

        let Some(body) = self.read_post_body(session, false).await? else {
            return Ok(());
        };

        let outcome = post_comment_to_sysop(
            session,
            self.services.user_repo(),
            self.services.mail_stores(),
            self.services.conferences(),
            CommentToSysopInput {
                subject,
                body,
                posted_at: SystemTime::now(),
            },
        )
        .await;

        self.render_post_outcome(outcome, "C").await
    }

    /// Renders the terminal-free post outcome. Shared between the `E`
    /// and `C` handlers so a single edit moves both wire surfaces in
    /// lockstep.
    async fn render_post_outcome(
        &mut self,
        outcome: PostMailOutcome,
        command_label: &str,
    ) -> Result<(), T::Error> {
        match outcome {
            PostMailOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            PostMailOutcome::UnknownUser => {
                self.write_and_flush(POST_UNKNOWN_USER_LINE).await?;
            }
            PostMailOutcome::RecipientNoAccess => {
                self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
            }
            PostMailOutcome::NoSysop => {
                self.write_and_flush(NO_SYSOP_LINE).await?;
            }
            PostMailOutcome::Posted(mail) => {
                let line = render_post_success(mail.number());
                self.write_and_flush(&line).await?;
            }
            PostMailOutcome::Rejected(PostMailError::AccessDenied) => {
                self.write_and_flush(POST_ACCESS_DENIED_LINE).await?;
            }
            PostMailOutcome::Rejected(PostMailError::NoMembership) => {
                // The poster's own membership is missing. The
                // auto-rejoin would normally have caught this on
                // logon, so reaching it here means the sysop revoked
                // mid-session — same wire surface as
                // POST_RECIPIENT_NO_ACCESS_LINE keeps the listener
                // honest about why the post failed.
                self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
            }
            PostMailOutcome::Rejected(
                PostMailError::EmptyAddressee
                | PostMailError::AddresseeMismatch
                | PostMailError::SubjectTooLong
                | PostMailError::BodyTooLong,
            ) => {
                // Defensive: the editor gates empty recipients and
                // oversized input upstream. The rule's gates fire only
                // if a future refactor lets an invalid draft slip past.
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            PostMailOutcome::Rejected(PostMailError::AddressingNotAllowed) => {
                self.write_and_flush(POST_ADDRESSING_NOT_ALLOWED_LINE)
                    .await?;
            }
            PostMailOutcome::Rejected(PostMailError::Store(err)) => {
                eprintln!("{command_label} command: failed to persist mail: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    /// Reads a single non-empty trimmed line in response to `prompt`,
    /// stamping the idle clock. Returns `None` when the user submits an
    /// empty line, an EOF, or an idle timeout. `silent = false` writes
    /// the `Message aborted.` notice on that path (the `E` / `C`
    /// composer); `silent = true` suppresses it (the `readMSG` sub-prompt
    /// reply / forward, which abort silently — B6).
    pub(super) async fn read_required_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
        silent: bool,
    ) -> Result<Option<String>, T::Error> {
        match self.read_prompted(prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    self.write_abort_notice(silent).await?;
                    return Ok(None);
                }
                Ok(Some(trimmed.to_string()))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                self.write_abort_notice(silent).await?;
                Ok(None)
            }
        }
    }

    /// Writes the `Message aborted.` notice unless `silent`.
    async fn write_abort_notice(&mut self, silent: bool) -> Result<(), T::Error> {
        if !silent {
            self.write_and_flush(POST_ABORTED_LINE).await?;
        }
        Ok(())
    }

    /// Reads a single trimmed line in response to `prompt`, returning
    /// the line verbatim even when it's empty (the legacy `To:` reroute
    /// to ALL relies on the empty case being distinguishable from EOF /
    /// idle).
    async fn read_optional_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
    ) -> Result<Option<String>, T::Error> {
        match self.read_prompted(prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                Ok(Some(line.trim().to_string()))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                self.write_and_flush(POST_ABORTED_LINE).await?;
                Ok(None)
            }
        }
    }

    /// Drives the line-mode editor's body input loop. Returns the
    /// concatenated body on `.`-on-its-own-line, and `None` on `/A`, EOF,
    /// or idle timeout. `silent = false` writes the abort notice on that
    /// path; `silent = true` suppresses it (the sub-prompt reply — B6).
    pub(super) async fn read_post_body(
        &mut self,
        session: &mut MenuSession,
        silent: bool,
    ) -> Result<Option<String>, T::Error> {
        self.write_and_flush(POST_BODY_PROMPT).await?;
        let mut body = String::new();
        loop {
            match self.read_prompted(b"", TerminalEcho::Visible).await? {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("/A") {
                        self.write_abort_notice(silent).await?;
                        return Ok(None);
                    }
                    if trimmed == "." {
                        return Ok(Some(body));
                    }
                    if !append_line_with_newline(&mut body, &line, MAX_MAIL_BODY_BYTES) {
                        self.write_abort_notice(silent).await?;
                        return Ok(None);
                    }
                }
                TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                    self.write_abort_notice(silent).await?;
                    return Ok(None);
                }
            }
        }
    }
}
