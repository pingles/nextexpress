//! `E` (Enter Mail) and `C` (Comment to Sysop) menu commands
//! (Slices 42 / 43 / 44).
//!
//! The terminal-free cores ([`post_mail`], [`post_comment_to_sysop`])
//! own store/repository resolution and the `messaging.allium` rule
//! invocation; the `MenuFlow` handlers below drive the minimal
//! line-mode editor (To: / Subject: / Private (y/N) / body lines
//! terminated by `.` on its own line) and render the outcomes. The two
//! handlers share the `Subject:` and body editor prompts plus the
//! wire-rendering of post outcomes.

use std::time::SystemTime;

use crate::app::input_limits::append_line_with_newline;
use crate::app::mail_stores::MailStores;
use crate::app::menu_command::PostArg;
use crate::app::menu_flow::mail_text::{
    render_post_success, MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, POST_ABORTED_LINE,
    POST_ACCESS_DENIED_LINE, POST_ADDRESSING_NOT_ALLOWED_LINE, POST_RECIPIENT_NO_ACCESS_LINE,
};
use crate::app::terminal::{Terminal, TerminalEcho};
use crate::domain::conference::Conference;
use crate::domain::messaging::limits::MAX_MAIL_BODY_BYTES;
use crate::domain::messaging::mail::{BroadcastTo, Mail};
use crate::domain::messaging::post_comment_to_sysop::{
    post_comment_to_sysop as post_comment_to_sysop_rule, CommentToSysopDraft,
};
use crate::domain::messaging::post_mail::{
    post_mail as post_mail_rule, PostMailDraft, PostMailError,
};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository, UserRepositoryError};

/// Prompt shown when the `E` command needs the recipient handle.
/// The legacy `enterMSG` uses the bare `To:` line that
/// `msgToHeader` paints (`amiexpress/express.e:10778`).
const POST_TO_PROMPT: &[u8] = b"\r\nTo: ";

/// Prompt for the subject during line-mode mail composition.
/// Simplified from `amiexpress/express.e:10847` (the legacy form
/// adds an ANSI-coloured "(Blank)=abort?" hint).
const POST_SUBJECT_PROMPT: &[u8] = b"Subject: ";

/// Prompt asking whether the new mail should be private.
/// Verbatim text from `amiexpress/express.e:10861`'s `Private` prompt
/// modulo the colour escapes the legacy `yesNo` macro renders.
const POST_PRIVATE_PROMPT: &[u8] = b"Private (y/N)? ";

/// Instructions printed before the body input loop. Slice 42 uses a
/// minimal line-mode editor — a full editor (`/S` save, `/A` abort,
/// numbered line edits) arrives in Phase 8. Still used by the `R`
/// sub-prompt reply (B6); `E` / `C` use the ruler editor below.
const POST_BODY_PROMPT: &[u8] =
    b"Enter your message. End with a single '.' on a line by itself; '/A' aborts.\r\n";

/// The `E` / `C` ruler-editor intro: the "Enter your text" instruction
/// and the 75-column ruler (`amiexpress/express.e:10146-10152`, the
/// repeating `|-------` pattern truncated to `maxLineLen`=75). Input
/// ends on a blank line.
const EDITOR_INTRO: &[u8] =
    b"\r\n   Enter your text. (Enter) alone to end. (75 chars/line)\r\n   (|-------|-------|-------|-------|-------|-------|-------|-------|-------|--)\r\n";

/// The ruler editor's `Msg. Options:` save-menu prompt, shown after a
/// blank line ends input (`amiexpress/express.e:10375-10379`, rendered
/// for the no-file-attach case so `F`/`X` are absent). `S` saves, `A`
/// aborts (with confirm), `C` continues editing, `L` lists, `?` shows
/// the verb help. `D`/`E` (delete / edit lines) are advertised but
/// deferred.
const EDITOR_MSG_OPTIONS_PROMPT: &[u8] =
    b"\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m,\x1b[33mC\x1b[36m,\x1b[33mD\x1b[36m,\x1b[33mE\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mS\x1b[36m,\x1b[33m? \x1b[0m>:";

/// The expanded `Msg. Options:` help list shown after `?`
/// (`amiexpress/express.e:10381-10389`, no-file-attach case). It ends
/// in its own ` >: ` prompt and reads the next verb directly.
const EDITOR_MSG_OPTIONS_HELP: &[u8] = b"\r\n\x1b[33mA\x1b[32m>\x1b[36mbort\x1b[0m\r\n\x1b[33mC\x1b[32m>\x1b[36montinue\x1b[0m\r\n\x1b[33mD\x1b[32m>\x1b[36melete Lines\x1b[0m\r\n\x1b[33mE\x1b[32m>\x1b[36mdit\x1b[0m\r\n\x1b[33mL\x1b[32m>\x1b[36mist\x1b[0m\r\n\x1b[33mS\x1b[32m>\x1b[36mave\x1b[0m\r\n\x1b[0m >: ";

/// The `A`bort confirmation prompt from the save menu
/// (`amiexpress/express.e:10568`). A `y` answer abandons the message.
const EDITOR_ABORT_CONFIRM_PROMPT: &[u8] = b"\r\nAbort message entry (y/n)? ";

/// Renders one ruler-editor line prompt, `<n>> ` with the number
/// left-justified to a 2-character field for lines 1..=99 and a
/// 3-character field beyond (legacy `\d[2]> ` / `\d[3]> ` at
/// `amiexpress/express.e:10180-10184`). Line 1 renders `"1 > "`,
/// line 10 `"10> "`, line 100 `"100> "`.
#[must_use]
fn render_editor_line_prompt(line_number: usize) -> Vec<u8> {
    if line_number <= 99 {
        format!("{line_number:<2}> ").into_bytes()
    } else {
        format!("{line_number:<3}> ").into_bytes()
    }
}

/// Renders the ruler editor's `L`ist output: a leading CRLF, then each
/// stored line as `<n>> <text>` followed by CRLF
/// (`amiexpress/express.e:10496-10504`).
#[must_use]
fn render_editor_listing(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\r\n");
    for (index, line) in lines.iter().enumerate() {
        out.extend_from_slice(&render_editor_line_prompt(index + 1));
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Sent when the typed recipient can't be resolved against the user
/// repository. Mirrors the legacy `User does not exist!!` notice
/// (`amiexpress/express.e:10814`).
const POST_UNKNOWN_USER_LINE: &[u8] = b"\r\nUnknown user.\r\n";

/// Sent when the `C` (comment to sysop) command can't resolve a slot-1
/// sysop user (e.g. a fresh installation that never seeded one). The
/// legacy BBS always has a sysop on disk; this notice surfaces the
/// misconfiguration so the operator can run the seed.
const NO_SYSOP_LINE: &[u8] = b"\r\nNo sysop is configured on this BBS.\r\n";

/// Already-collected fields for an `E` command.
struct PostMailInput {
    /// Raw recipient line, either supplied inline or via the `To:`
    /// prompt.
    typed_to: String,
    /// Subject line.
    subject: String,
    /// Whether the user requested private visibility.
    private: bool,
    /// Message body.
    body: String,
    /// Posting timestamp.
    posted_at: SystemTime,
}

/// Already-collected fields for a `C` command.
struct CommentToSysopInput {
    /// Subject line.
    subject: String,
    /// Message body.
    body: String,
    /// Posting timestamp.
    posted_at: SystemTime,
}

/// The wire line for a static post outcome — a pure mapping so a plain
/// `#[test]` pins each byte choice without a capture terminal or async
/// runtime (SYSTEM.md item 10). `Posted` renders a dynamic success
/// line and maps to `None`; the two log side effects (`LookupFailed`,
/// `Rejected(Store)`) stay in the async handler.
fn post_outcome_line(outcome: &PostMailOutcome) -> Option<&'static [u8]> {
    match outcome {
        PostMailOutcome::NoMailBase => Some(NO_MAIL_BASE_LINE),
        PostMailOutcome::UnknownUser => Some(POST_UNKNOWN_USER_LINE),
        PostMailOutcome::RecipientNoAccess => Some(POST_RECIPIENT_NO_ACCESS_LINE),
        PostMailOutcome::NoSysop => Some(NO_SYSOP_LINE),
        // Both store-shaped failures share the fixed notify-the-sysop
        // surface; their distinct log lines live in the handler.
        PostMailOutcome::LookupFailed(_) | PostMailOutcome::Rejected(PostMailError::Store(_)) => {
            Some(MAIL_STORE_ERROR_LINE)
        }
        PostMailOutcome::Posted(_) => None,
        PostMailOutcome::Rejected(PostMailError::AccessDenied) => Some(POST_ACCESS_DENIED_LINE),
        // The poster's own membership is missing. The auto-rejoin would
        // normally have caught this on logon, so reaching it here means
        // the sysop revoked mid-session — same wire surface as
        // POST_RECIPIENT_NO_ACCESS_LINE keeps the listener honest about
        // why the post failed.
        PostMailOutcome::Rejected(PostMailError::NoMembership) => {
            Some(POST_RECIPIENT_NO_ACCESS_LINE)
        }
        // Defensive: the editor gates empty recipients and oversized
        // input upstream. The rule's gates fire only if a future
        // refactor lets an invalid draft slip past.
        PostMailOutcome::Rejected(
            PostMailError::EmptyAddressee
            | PostMailError::AddresseeMismatch
            | PostMailError::SubjectTooLong
            | PostMailError::BodyTooLong,
        ) => Some(POST_ABORTED_LINE),
        PostMailOutcome::Rejected(PostMailError::AddressingNotAllowed) => {
            Some(POST_ADDRESSING_NOT_ALLOWED_LINE)
        }
    }
}

/// Outcome of a terminal-free post command.
enum PostMailOutcome {
    /// The session has no usable message base.
    NoMailBase,
    /// The named addressee does not exist.
    UnknownUser,
    /// The named addressee is not a member of the current conference.
    RecipientNoAccess,
    /// No sysop user exists for `C`.
    NoSysop,
    /// A repository lookup failed while resolving an addressee.
    LookupFailed(UserRepositoryError),
    /// The message was persisted.
    Posted(Mail),
    /// The domain rule rejected the draft, or the store failed.
    Rejected(PostMailError),
}

/// Runs the post-mail use case without terminal I/O.
async fn post_mail<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    conferences: &[Conference],
    input: PostMailInput,
) -> PostMailOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some((visit_msgbase, mut guard)) = super::lock_current_base(session, mail_stores).await
    else {
        return PostMailOutcome::NoMailBase;
    };

    let (broadcast_to, to_name, addressee_slot) = match classify_recipient(&input.typed_to) {
        Recipient::Broadcast(kind, label) => (kind, label, None),
        Recipient::Individual(typed) => {
            let resolved = match user_repo.find_by_handle(&typed) {
                Ok(NameLookupResult::Found(user)) => *user,
                Ok(NameLookupResult::NotFound) => return PostMailOutcome::UnknownUser,
                Err(error) => return PostMailOutcome::LookupFailed(error),
            };
            let Some(conference) = conferences
                .iter()
                .find(|c| c.number() == visit_msgbase.conference_number())
            else {
                return PostMailOutcome::NoMailBase;
            };
            if !resolved.has_membership(conference) {
                return PostMailOutcome::RecipientNoAccess;
            }
            (
                BroadcastTo::None,
                resolved.handle().to_string(),
                Some(resolved.slot_number()),
            )
        }
    };

    let Some(allowed_addressing) = super::allowed_addressing_for(conferences, visit_msgbase) else {
        return PostMailOutcome::NoMailBase;
    };

    let author_handle = session.user().handle().to_string();
    let result = post_mail_rule(
        session.user_mut(),
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        PostMailDraft {
            to_name,
            broadcast_to,
            addressee_slot,
            from_name: author_handle,
            subject: input.subject,
            body: input.body,
            private: input.private,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => PostMailOutcome::Posted(mail),
        Err(err) => PostMailOutcome::Rejected(err),
    }
}

/// Runs the comment-to-sysop use case without terminal I/O.
async fn post_comment_to_sysop<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    conferences: &[Conference],
    input: CommentToSysopInput,
) -> PostMailOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some((visit_msgbase, mut guard)) = super::lock_current_base(session, mail_stores).await
    else {
        return PostMailOutcome::NoMailBase;
    };

    let sysop = match user_repo.find_sysop() {
        Ok(NameLookupResult::Found(user)) => *user,
        Ok(NameLookupResult::NotFound) => return PostMailOutcome::NoSysop,
        Err(error) => return PostMailOutcome::LookupFailed(error),
    };

    let Some(allowed_addressing) = super::allowed_addressing_for(conferences, visit_msgbase) else {
        return PostMailOutcome::NoMailBase;
    };

    let from_name = session.user().handle().to_string();
    let result = post_comment_to_sysop_rule(
        session.user_mut(),
        visit_msgbase,
        allowed_addressing,
        &mut *guard,
        CommentToSysopDraft {
            sysop_slot: sysop.slot_number(),
            from_name,
            subject: input.subject,
            body: input.body,
            posted_at: input.posted_at,
        },
    );
    drop(guard);

    match result {
        Ok(mail) => PostMailOutcome::Posted(mail),
        Err(err) => PostMailOutcome::Rejected(err),
    }
}

/// Outcome of classifying the recipient typed at the `To:` prompt.
enum Recipient {
    /// `ALL` or `EALL` (case-insensitive). The `label` is the
    /// upper-case form used as `Mail.to_name`.
    Broadcast(BroadcastTo, String),
    /// A literal user handle the caller must resolve through the user
    /// repository.
    Individual(String),
}

/// Maps a typed `To:` line to a [`Recipient`]. An empty line reroutes
/// to ALL, matching legacy `enterMSG`
/// (`amiexpress/express.e:10827`).
fn classify_recipient(typed: &str) -> Recipient {
    let trimmed = typed.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("ALL") {
        return Recipient::Broadcast(BroadcastTo::All, "ALL".to_string());
    }
    if trimmed.eq_ignore_ascii_case("EALL") {
        return Recipient::Broadcast(BroadcastTo::Eall, "EALL".to_string());
    }
    Recipient::Individual(trimmed.to_string())
}

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
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        // Step 1: collect the recipient name. `E <to>` provides it
        // inline; bare `E` prompts. An empty prompt response reroutes
        // to ALL, mirroring legacy `enterMSG`
        // (`amiexpress/express.e:10827`) where the default address is
        // ALL when the user submits a blank `To:` line.
        let typed_to = match arg {
            PostArg::To(name) => name,
            PostArg::Missing => match self.read_optional_line(session, POST_TO_PROMPT).await? {
                Some(line) => line,
                // Reserved local-cancellation branch. EOF and idle timeout
                // propagate from the shared reader as connection exits.
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
        let private_line = self
            .read_prompted(POST_PRIVATE_PROMPT, TerminalEcho::Visible)
            .await?;
        session.record_input(self.services.clock.now());
        let private = matches!(private_line.trim().chars().next(), Some('y' | 'Y'));

        // Step 4: body via the ruler / numbered-line editor with the
        // `Msg. Options:` save menu (Fix 4). The full-screen editor fork
        // is skipped.
        let Some(body) = self.read_editor_body(session).await? else {
            return Ok(());
        };

        let outcome = post_mail(
            session,
            self.services.user_repo.as_ref(),
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            PostMailInput {
                typed_to,
                subject,
                private,
                body,
                posted_at: self.services.clock.now(),
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
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT, false)
            .await?
        else {
            return Ok(());
        };

        let Some(body) = self.read_editor_body(session).await? else {
            return Ok(());
        };

        let outcome = post_comment_to_sysop(
            session,
            self.services.user_repo.as_ref(),
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            CommentToSysopInput {
                subject,
                body,
                posted_at: self.services.clock.now(),
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
    ) -> crate::app::menu_flow::MenuFlowResult<(), T::Error> {
        match &outcome {
            PostMailOutcome::LookupFailed(error) => {
                eprintln!("{command_label} command: failed to resolve user: {error}");
            }
            PostMailOutcome::Rejected(PostMailError::Store(error)) => {
                eprintln!("{command_label} command: failed to persist mail: {error}");
            }
            _ => {}
        }
        if let Some(line) = post_outcome_line(&outcome) {
            return self.write_and_flush(line).await;
        }
        let PostMailOutcome::Posted(mail) = outcome else {
            unreachable!("only Posted renders a dynamic line");
        };
        let line = render_post_success(mail.number());
        self.write_and_flush(&line).await
    }

    /// Reads a single non-empty trimmed line in response to `prompt`,
    /// stamping the idle clock. Returns `None` only when the user submits a
    /// blank line. `silent = false` writes the `Message aborted.` notice for
    /// that local cancellation (the `E` / `C` composer); `silent = true`
    /// suppresses it (the `readMSG` sub-prompt reply / forward, which abort
    /// silently — B6). EOF and idle timeout instead propagate as connection
    /// exits without a command-specific abort notice.
    pub(super) async fn read_required_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
        silent: bool,
    ) -> crate::app::menu_flow::MenuFlowResult<Option<String>, T::Error> {
        let notice = if silent {
            super::AbortNotice::Silent
        } else {
            super::AbortNotice::MessageAborted
        };
        match self
            .prompt_line(session, prompt, super::EmptyMeaning::Abort, notice)
            .await?
        {
            super::PromptLine::Entered(line) => Ok(Some(line)),
            super::PromptLine::Aborted => Ok(None),
            super::PromptLine::Kept => unreachable!("Abort prompts have no keep branch"),
        }
    }

    /// Reads a single trimmed line in response to `prompt`, returning the line
    /// even when it is empty (the legacy `To:` reroute to ALL relies on that
    /// local blank answer). EOF and idle timeout do not return `None`; they
    /// propagate as connection exits.
    async fn read_optional_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
    ) -> crate::app::menu_flow::MenuFlowResult<Option<String>, T::Error> {
        match self
            .prompt_line(
                session,
                prompt,
                super::EmptyMeaning::Verbatim,
                super::AbortNotice::MessageAborted,
            )
            .await?
        {
            super::PromptLine::Entered(line) => Ok(Some(line)),
            super::PromptLine::Aborted => Ok(None),
            super::PromptLine::Kept => unreachable!("Verbatim prompts have no keep branch"),
        }
    }

    /// Drives the `E` / `C` ruler / numbered-line editor (Fix 4):
    /// prints the ruler intro, reads numbered lines until a blank line
    /// ends input, then offers the `Msg. Options:` save menu. Returns
    /// the assembled body on `S`ave, and `None` on a confirmed `A`bort or an
    /// over-length body — writing the `Message aborted.` notice on those
    /// command-local paths. EOF and idle timeout propagate immediately as
    /// connection exits without that notice. `C`ontinue resumes
    /// input, `L`ist shows the entered lines, `?` shows the verb help;
    /// `D`/`E`/`F`/`X` are advertised but deferred, and the full-screen
    /// editor fork (`amiexpress/express.e:10095-10100`) is skipped.
    pub(super) async fn read_editor_body(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<Option<String>, T::Error> {
        self.write_and_flush(EDITOR_INTRO).await?;
        // `lines` drives the numbered prompts and the `L`ist view;
        // `body` is the assembled message, capped by the same helper the
        // `.` editor uses (the PostMail `BodyTooLong` gate is the
        // backstop, so an over-length body yields the same notice).
        let mut lines: Vec<String> = Vec::new();
        let mut body = String::new();
        'editor: loop {
            // Input phase: numbered lines until a blank line ends input
            // (the legacy "(Enter) alone to end").
            loop {
                let prompt = render_editor_line_prompt(lines.len() + 1);
                let line = self.read_prompted(&prompt, TerminalEcho::Visible).await?;
                session.record_input(self.services.clock.now());
                if line.is_empty() {
                    break;
                }
                if !append_line_with_newline(&mut body, &line, MAX_MAIL_BODY_BYTES) {
                    self.write_and_flush(POST_ABORTED_LINE).await?;
                    return Ok(None);
                }
                lines.push(line);
            }

            // Save-menu phase. `?` swaps the next prompt for the verb
            // help list, which carries its own ` >: ` prompt.
            let mut show_help = false;
            loop {
                let prompt: &[u8] = if show_help {
                    EDITOR_MSG_OPTIONS_HELP
                } else {
                    EDITOR_MSG_OPTIONS_PROMPT
                };
                show_help = false;
                let verb = self.read_prompted(prompt, TerminalEcho::Visible).await?;
                session.record_input(self.services.clock.now());
                match verb.trim().chars().next().map(|c| c.to_ascii_lowercase()) {
                    // S>ave: return the body assembled so far.
                    Some('s') => return Ok(Some(body)),
                    // A>bort: confirm, then abandon on a `y`.
                    Some('a') => {
                        if self.confirm_editor_abort(session).await? {
                            self.write_and_flush(POST_ABORTED_LINE).await?;
                            return Ok(None);
                        }
                    }
                    // C>ontinue: resume the input phase.
                    Some('c') => continue 'editor,
                    // L>ist the lines entered so far.
                    Some('l') => {
                        self.write_and_flush(&render_editor_listing(&lines)).await?;
                    }
                    // `?` shows the verb help as the next prompt.
                    Some('?') => show_help = true,
                    // D/E/F/X and anything else: deferred — re-prompt.
                    _ => {}
                }
            }
        }
    }

    /// Reads the `Abort message entry (y/n)?` answer from the save menu
    /// (`amiexpress/express.e:10568`). Returns `true` (abandon) on a
    /// `y`/`Y` answer; any other submitted answer keeps editing. A disconnect
    /// or idle timeout propagates as a connection exit.
    async fn confirm_editor_abort(
        &mut self,
        session: &mut MenuSession,
    ) -> crate::app::menu_flow::MenuFlowResult<bool, T::Error> {
        let line = self
            .read_prompted(EDITOR_ABORT_CONFIRM_PROMPT, TerminalEcho::Visible)
            .await?;
        session.record_input(self.services.clock.now());
        Ok(matches!(line.trim().chars().next(), Some('y' | 'Y')))
    }

    /// Drives the line-mode editor's body input loop. Returns the concatenated
    /// body on `.`-on-its-own-line, and `None` on `/A` or an over-length body.
    /// `silent = false` writes the abort notice on those command-local paths;
    /// `silent = true` suppresses it (the sub-prompt reply — B6). EOF and idle
    /// timeout propagate as connection exits.
    pub(super) async fn read_post_body(
        &mut self,
        session: &mut MenuSession,
        silent: bool,
    ) -> crate::app::menu_flow::MenuFlowResult<Option<String>, T::Error> {
        self.write_and_flush(POST_BODY_PROMPT).await?;
        let mut body = String::new();
        loop {
            let line = self.read_prompted(b"", TerminalEcho::Visible).await?;
            session.record_input(self.services.clock.now());
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("/A") {
                self.abort_notice(if silent {
                    super::AbortNotice::Silent
                } else {
                    super::AbortNotice::MessageAborted
                })
                .await?;
                return Ok(None);
            }
            if trimmed == "." {
                return Ok(Some(body));
            }
            if !append_line_with_newline(&mut body, &line, MAX_MAIL_BODY_BYTES) {
                self.abort_notice(if silent {
                    super::AbortNotice::Silent
                } else {
                    super::AbortNotice::MessageAborted
                })
                .await?;
                return Ok(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn line_mode_body_collects_lines_until_the_dot_terminator() {
        let services = crate::app::menu_flow::test_support::test_services();
        let mut terminal = crate::app::menu_flow::test_support::CaptureTerminal::with_lines(vec![
            crate::app::terminal::TerminalRead::Line("first line".to_string()),
            crate::app::terminal::TerminalRead::Line(".".to_string()),
        ]);
        let mut session = crate::app::menu_flow::test_support::menu_session();
        let mut flow = super::super::MenuFlow {
            terminal: &mut terminal,
            services: &services,
        };

        let body = flow
            .read_post_body(&mut session, false)
            .await
            .expect("scripted terminal is infallible");

        assert_eq!(body.as_deref(), Some("first line\n"));
        assert_eq!(terminal.output, POST_BODY_PROMPT);
    }

    #[test]
    fn static_post_outcome_lines_pin_the_wire_bytes() {
        // Item 10's line_for extraction: every static outcome arm is a
        // pure mapping, pinned with plain byte asserts (no capture
        // terminal, no async runtime). `Posted` renders a dynamic
        // success line and maps to `None` — covered by the handler
        // tests.
        fn store_error() -> crate::domain::messaging::mail_store::MailStoreError {
            crate::domain::messaging::mail_store::MailStoreError::Backend {
                source: "backing store unavailable".into(),
            }
        }

        assert_eq!(
            post_outcome_line(&PostMailOutcome::NoMailBase),
            Some(NO_MAIL_BASE_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::UnknownUser),
            Some(POST_UNKNOWN_USER_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::RecipientNoAccess),
            Some(POST_RECIPIENT_NO_ACCESS_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::NoSysop),
            Some(NO_SYSOP_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::LookupFailed(
                UserRepositoryError::Storage {
                    context: "test",
                    message: "down".to_string(),
                }
            )),
            Some(MAIL_STORE_ERROR_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::Rejected(PostMailError::AccessDenied)),
            Some(POST_ACCESS_DENIED_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::Rejected(PostMailError::NoMembership)),
            Some(POST_RECIPIENT_NO_ACCESS_LINE)
        );
        for defensive in [
            PostMailError::EmptyAddressee,
            PostMailError::AddresseeMismatch,
            PostMailError::SubjectTooLong,
            PostMailError::BodyTooLong,
        ] {
            assert_eq!(
                post_outcome_line(&PostMailOutcome::Rejected(defensive)),
                Some(POST_ABORTED_LINE)
            );
        }
        assert_eq!(
            post_outcome_line(&PostMailOutcome::Rejected(
                PostMailError::AddressingNotAllowed
            )),
            Some(POST_ADDRESSING_NOT_ALLOWED_LINE)
        );
        assert_eq!(
            post_outcome_line(&PostMailOutcome::Rejected(PostMailError::Store(
                store_error()
            ))),
            Some(MAIL_STORE_ERROR_LINE)
        );
    }

    fn assert_all(result: Recipient, context: &str) {
        if let Recipient::Broadcast(BroadcastTo::All, label) = result {
            assert_eq!(label, "ALL", "{context}: wrong ALL label");
        } else {
            panic!(
                "{context}: expected Recipient::Broadcast(All, _), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    fn assert_eall(result: Recipient, context: &str) {
        if let Recipient::Broadcast(BroadcastTo::Eall, label) = result {
            assert_eq!(label, "EALL", "{context}: wrong EALL label");
        } else {
            panic!(
                "{context}: expected Recipient::Broadcast(Eall, _), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    fn assert_individual(result: Recipient, expected: &str, context: &str) {
        if let Recipient::Individual(handle) = result {
            assert_eq!(handle, expected, "{context}: wrong handle");
        } else {
            panic!(
                "{context}: expected Recipient::Individual(_), got {:?}",
                recipient_kind(&result),
            );
        }
    }

    #[test]
    fn empty_recipient_reroutes_to_all() {
        // Legacy `enterMSG` reroute (`amiexpress/express.e:10827`).
        assert_all(classify_recipient(""), "empty");
        assert_all(classify_recipient("   "), "whitespace");
    }

    #[test]
    fn all_and_eall_are_case_insensitive() {
        for typed in ["ALL", "all", "All"] {
            assert_all(classify_recipient(typed), typed);
        }
        for typed in ["EALL", "eall", "EAll"] {
            assert_eall(classify_recipient(typed), typed);
        }
    }

    #[test]
    fn ordinary_handle_is_individual() {
        assert_individual(classify_recipient("alice"), "alice", "alice");
    }

    #[test]
    fn handle_is_trimmed() {
        assert_individual(classify_recipient("  alice  "), "alice", "trimmed alice");
    }

    fn recipient_kind(r: &Recipient) -> &'static str {
        match r {
            Recipient::Broadcast(BroadcastTo::None, _) => "broadcast(None)",
            Recipient::Broadcast(BroadcastTo::All, _) => "broadcast(All)",
            Recipient::Broadcast(BroadcastTo::Eall, _) => "broadcast(Eall)",
            Recipient::Individual(_) => "individual",
        }
    }

    #[test]
    fn editor_line_prompt_left_justifies_the_number() {
        // Legacy `\d[2]> ` (`amiexpress/express.e:10180`): the number is
        // left-justified to a 2-char field, so line 1 reads `"1 > "`.
        assert_eq!(render_editor_line_prompt(1), b"1 > ");
        assert_eq!(render_editor_line_prompt(9), b"9 > ");
        // Two digits fill the field exactly.
        assert_eq!(render_editor_line_prompt(10), b"10> ");
        assert_eq!(render_editor_line_prompt(99), b"99> ");
        // Beyond 99 the legacy widens to `\d[3]` (`:10182`).
        assert_eq!(render_editor_line_prompt(100), b"100> ");
    }

    #[test]
    fn editor_listing_numbers_each_line() {
        // Legacy `L` (`amiexpress/express.e:10496-10504`): leading CRLF,
        // then `<n>> <text>` + CRLF per line.
        let lines = vec!["first".to_string(), "second".to_string()];
        assert_eq!(
            render_editor_listing(&lines),
            b"\r\n1 > first\r\n2 > second\r\n"
        );
    }

    #[test]
    fn editor_listing_with_no_lines_is_just_a_crlf() {
        assert_eq!(render_editor_listing(&[]), b"\r\n");
    }
}
