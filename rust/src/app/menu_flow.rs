//! Menu sub-flow: command loop and dispatch.
//!
//! Runs once the session is onboarded and joined to a conference.
//! Reads command lines, dispatches the supported ones (Phase 4
//! supports `G` for logoff and `J <num>` for explicit conference
//! join; Phase 6 adds `R <num>` for reading a single message) and
//! reports back to the driver when the loop terminates.

use std::time::SystemTime;

use crate::app::menu_command::{parse_menu_command, MenuCommand, NumberArg, PostArg, ScanArg};
use crate::app::services::AppServices;
use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_mail_body, render_mail_header, render_post_success, render_scan_summary,
    DELETED_MESSAGE_LINE, GOODBYE_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, JOIN_REQUIRES_NUMBER_LINE, MAIL_STORE_ERROR_LINE, MENU_PROMPT,
    MESSAGE_NOT_FOUND_LINE, NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, NO_CONFERENCE_ACCESS_LINE,
    NO_MAIL_BASE_LINE, NO_SYSOP_LINE, POST_ABORTED_LINE, POST_ACCESS_DENIED_LINE,
    POST_ADDRESSING_NOT_ALLOWED_LINE, POST_BODY_PROMPT, POST_PRIVATE_PROMPT,
    POST_RECIPIENT_NO_ACCESS_LINE, POST_SUBJECT_PROMPT, POST_TO_PROMPT, POST_UNKNOWN_USER_LINE,
    READ_DENIED_LINE, READ_REQUIRES_NUMBER_LINE, UNKNOWN_COMMAND_LINE,
};
use crate::domain::conference::{AllowedAddressing, MessageBaseRef};
use crate::domain::messaging::mail::BroadcastTo;
use crate::domain::messaging::post_mail::{PostMailDraft, PostMailError};
use crate::domain::messaging::read_mail::ReadMailError;
use crate::domain::session::typed::{
    ExplicitJoinTransition, LoggingOffSession, MenuSession, ScanOnJoin,
};
use crate::domain::user_repository::NameLookupResult;

/// Outcome of [`MenuFlow::handle_explicit_join`]. The success branch
/// returns the still-Menu-state session so the menu loop continues;
/// failure terminates with `LogoffReason::NoConferenceAccess`.
enum ExplicitJoinResult {
    /// The user is now attached to a (possibly fallback) conference.
    Joined(MenuSession),
    /// The user lost their last membership; the session is closing.
    NoAccess(LoggingOffSession),
}

/// Menu sub-flow.
pub(crate) struct MenuFlow<'a, T>
where
    T: Terminal,
{
    terminal: &'a mut T,
    services: &'a AppServices,
}

impl<'a, T> MenuFlow<'a, T>
where
    T: Terminal,
{
    /// Constructs a flow that drives `terminal` against the supplied
    /// driven adapters.
    pub(crate) fn new(terminal: &'a mut T, services: &'a AppServices) -> Self {
        Self { terminal, services }
    }

    /// Runs the menu loop until the session reaches a logoff state.
    pub(crate) async fn run(
        &mut self,
        mut session: MenuSession,
    ) -> Result<LoggingOffSession, T::Error> {
        loop {
            let access_level = session.user().access_level();
            let menu_bytes = match session.current_conference_number() {
                Some(conf) => {
                    self.services
                        .screens()
                        .conference_menu(conf, access_level)
                        .await
                }
                None => self.services.screens().default_menu(access_level).await,
            };
            self.terminal.write(&menu_bytes).await?;
            let read = self
                .read_prompted(MENU_PROMPT, TerminalEcho::Visible)
                .await?;
            let line = match read {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    line
                }
                TerminalRead::Eof => return Ok(session.into_active().apply_carrier_loss()),
                TerminalRead::IdleTimedOut => {
                    let logoff = session.into_active().apply_idle_timeout(
                        self.services.session_policy().treat_timeout_as_logoff(),
                    );
                    self.write_and_flush(IDLE_TIMEOUT_LINE).await?;
                    return Ok(logoff);
                }
            };
            let trimmed = line.trim();
            match parse_menu_command(trimmed) {
                MenuCommand::Logoff => {
                    let logging_off = session.user_requests_logoff();
                    self.write_and_flush(GOODBYE_LINE).await?;
                    return Ok(logging_off);
                }
                MenuCommand::Join(arg) => match arg {
                    NumberArg::Number(n) => {
                        session = match self.handle_explicit_join(session, n).await? {
                            ExplicitJoinResult::Joined(menu) => menu,
                            ExplicitJoinResult::NoAccess(logging_off) => {
                                return Ok(logging_off);
                            }
                        };
                    }
                    NumberArg::Missing => {
                        self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?;
                    }
                    NumberArg::Invalid => {
                        self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
                    }
                },
                MenuCommand::Read(arg) => match arg {
                    NumberArg::Number(n) => {
                        self.handle_read_mail(&mut session, n).await?;
                    }
                    NumberArg::Missing => {
                        self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?;
                    }
                    NumberArg::Invalid => {
                        self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
                    }
                },
                MenuCommand::Scan(scan) => {
                    self.handle_scan_mail(&mut session, scan).await?;
                }
                MenuCommand::Post(post) => {
                    self.handle_post_mail(&mut session, post).await?;
                }
                MenuCommand::CommentToSysop => {
                    self.handle_comment_to_sysop(&mut session).await?;
                }
                MenuCommand::Unknown => {
                    self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
                }
            }
        }
    }

    /// Handles an `M` / `N` command from the menu (Slice 40). Walks
    /// the current conference's message base counting unread mail
    /// for the bound user, advances the user's `last_scanned`
    /// pointer (lazy-creating the row if needed), and renders the
    /// summary line.
    async fn handle_scan_mail(
        &mut self,
        session: &mut MenuSession,
        scan: ScanArg,
    ) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(store) = self.services.mail_stores().for_msgbase(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let from_message = match scan {
            // N => start from `last_scanned + 1` (the "new mail since"
            // semantics the spec encodes with `from_message = 0`).
            ScanArg::New => 0,
            // M => start from message 1 (caller-controlled walk).
            ScanArg::All => 1,
        };
        let scope = self
            .msgbase_all_scan_scope(visit_msgbase)
            .unwrap_or_default();

        let guard = store.lock().await;
        let result = match session.scan_mail(
            &**guard,
            visit_msgbase,
            scope,
            from_message,
            SystemTime::now(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("scan_mail failed: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                return Ok(());
            }
        };
        drop(guard);

        let summary = render_scan_summary(result.unread_count, result.first_unread_number);
        self.write_and_flush(&summary).await?;
        Ok(())
    }

    /// Handles an `R <num>` command from the menu (Slice 39). Loads
    /// the requested message from the current conference's mail
    /// store, applies the `ReadMail` rule (mutating both the user's
    /// read pointers and the mail's `received_at`), persists the
    /// mail back, and renders the header + body to the terminal.
    async fn handle_read_mail(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(store) = self.services.mail_stores().for_msgbase(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        // Resolve the conference name up-front so the immutable borrow
        // on `services.conferences()` doesn't overlap the mutable
        // borrows below.
        let conf_name = self
            .services
            .conferences()
            .iter()
            .find(|c| c.number() == visit_msgbase.conference_number())
            .map(|c| c.name().to_string())
            .unwrap_or_default();

        let mut guard = store.lock().await;
        let mut mail = match guard.load(number) {
            Ok(Some(mail)) => mail,
            Ok(None) => {
                self.write_and_flush(MESSAGE_NOT_FOUND_LINE).await?;
                return Ok(());
            }
            Err(err) => {
                eprintln!("R command: failed to load mail #{number}: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                return Ok(());
            }
        };

        match session.read_mail(&mut mail, SystemTime::now()) {
            Ok(()) => {}
            Err(ReadMailError::Deleted) => {
                self.write_and_flush(DELETED_MESSAGE_LINE).await?;
                return Ok(());
            }
            Err(
                ReadMailError::AccessDenied
                | ReadMailError::NotPermitted
                | ReadMailError::NoMembership,
            ) => {
                self.write_and_flush(READ_DENIED_LINE).await?;
                return Ok(());
            }
        }

        if let Err(err) = guard.save(&mail) {
            eprintln!("R command: failed to save mail #{number}: {err}");
            self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            return Ok(());
        }
        // Lock can be released before rendering — the mail is owned.
        drop(guard);

        let header = render_mail_header(&mail, &conf_name);
        let body = render_mail_body(mail.body());
        self.terminal.write(&header).await?;
        self.terminal.write(&body).await?;
        self.terminal.flush().await?;
        Ok(())
    }

    /// Reads a single non-empty trimmed line in response to `prompt`,
    /// stamping the idle clock. Returns `None` (and writes the abort
    /// notice) when the user submits an empty line, an EOF, or an
    /// idle timeout — the post-mail composer treats these the same.
    async fn read_required_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
    ) -> Result<Option<String>, T::Error> {
        match self.read_prompted(prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    self.write_and_flush(POST_ABORTED_LINE).await?;
                    return Ok(None);
                }
                Ok(Some(trimmed.to_string()))
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                self.write_and_flush(POST_ABORTED_LINE).await?;
                Ok(None)
            }
        }
    }

    /// Drives the line-mode editor's body input loop. Returns the
    /// concatenated body on `.`-on-its-own-line, and `None` (after
    /// writing the abort notice) on `/A`, EOF, or idle timeout.
    async fn read_post_body(
        &mut self,
        session: &mut MenuSession,
    ) -> Result<Option<String>, T::Error> {
        self.write_and_flush(POST_BODY_PROMPT).await?;
        let mut body = String::new();
        loop {
            match self.read_prompted(b"", TerminalEcho::Visible).await? {
                TerminalRead::Line(line) => {
                    session.record_input(SystemTime::now());
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("/A") {
                        self.write_and_flush(POST_ABORTED_LINE).await?;
                        return Ok(None);
                    }
                    if trimmed == "." {
                        return Ok(Some(body));
                    }
                    body.push_str(&line);
                    body.push('\n');
                }
                TerminalRead::Eof | TerminalRead::IdleTimedOut => {
                    self.write_and_flush(POST_ABORTED_LINE).await?;
                    return Ok(None);
                }
            }
        }
    }

    /// Handles an `E` / `E <to>` command from the menu (Slice 42).
    /// Drives the line-mode editor: prompts for the recipient (when
    /// not supplied inline), subject, private flag and body, resolves
    /// the addressee through the user repository, then calls the
    /// `PostMail` rule via the typed session.
    async fn handle_post_mail(
        &mut self,
        session: &mut MenuSession,
        arg: PostArg,
    ) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(store) = self.services.mail_stores().for_msgbase(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

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

        // Step 2: classify the recipient and (for individual addressees)
        // resolve them through the user repository.
        let (broadcast_to, to_name, addressee_slot, addressee_handle) =
            match classify_recipient(&typed_to) {
                Recipient::Broadcast(kind, label) => (kind, label, None, None),
                Recipient::Individual(typed) => {
                    let resolved = match self.services.user_repo().find_by_handle(&typed) {
                        NameLookupResult::Found(user) => *user,
                        NameLookupResult::NotFound => {
                            self.write_and_flush(POST_UNKNOWN_USER_LINE).await?;
                            return Ok(());
                        }
                    };
                    let Some(conference) = self
                        .services
                        .conferences()
                        .iter()
                        .find(|c| c.number() == visit_msgbase.conference_number())
                    else {
                        self.write_and_flush(NO_MAIL_BASE_LINE).await?;
                        return Ok(());
                    };
                    if !resolved.has_membership(conference) {
                        self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
                        return Ok(());
                    }
                    let handle = resolved.handle().to_string();
                    (
                        BroadcastTo::None,
                        handle.clone(),
                        Some(resolved.slot_number()),
                        Some(handle),
                    )
                }
            };
        let _ = addressee_handle; // currently unused beyond the lookup

        // Resolve the per-msgbase addressing policy from the conference
        // catalogue (Slice 43). Unknown msgbase coordinates fall through
        // to NO_MAIL_BASE_LINE in case of misconfiguration.
        let Some(allowed_addressing) = self.msgbase_allowed_addressing(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        // Step 3: subject prompt. Empty subject aborts (mirrors
        // `amiexpress/express.e:10854-10857`).
        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT)
            .await?
        else {
            return Ok(());
        };

        // Step 4: private flag. Default is N if the user just hits CR.
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

        // Step 5: body. Slice 42 ships a minimal line-mode editor —
        // each line is read until the user types `.` on its own line,
        // or `/A` to abort. The full editor (numbered line edits,
        // `/S` save, quoting) arrives in Phase 8.
        let Some(body) = self.read_post_body(session).await? else {
            return Ok(());
        };

        // Step 6: post. Lock the msgbase, call the rule, render the
        // outcome. The `display_name_of` black box currently honours
        // only `NameType::Handle`; real-name / internet-name
        // promotion lands with the user profile fields in a later
        // slice.
        let author_handle = session.user().handle().to_string();

        let mut guard = store.lock().await;
        let result = session.post_mail(
            visit_msgbase,
            allowed_addressing,
            &mut **guard,
            PostMailDraft {
                to_name,
                broadcast_to,
                addressee_slot,
                from_name: author_handle,
                subject,
                body,
                private,
                posted_at: SystemTime::now(),
            },
        );
        drop(guard);

        self.render_post_result(result, "E").await
    }

    /// Renders the outcome of a [`PostMail`](crate::domain::messaging::post_mail::post_mail)
    /// or `PostCommentToSysop` invocation to the terminal. Shared
    /// between the `E` and `C` handlers so a single edit moves both
    /// wire surfaces in lockstep.
    async fn render_post_result(
        &mut self,
        result: Result<crate::domain::messaging::mail::Mail, PostMailError>,
        command_label: &str,
    ) -> Result<(), T::Error> {
        match result {
            Ok(mail) => {
                let line = render_post_success(mail.number());
                self.write_and_flush(&line).await?;
            }
            Err(PostMailError::AccessDenied) => {
                self.write_and_flush(POST_ACCESS_DENIED_LINE).await?;
            }
            Err(PostMailError::NoMembership) => {
                // The poster's own membership is missing. The
                // auto-rejoin would normally have caught this on
                // logon, so reaching it here means the sysop revoked
                // mid-session — same wire surface as
                // POST_RECIPIENT_NO_ACCESS_LINE keeps the listener
                // honest about why the post failed.
                self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
            }
            Err(PostMailError::EmptyAddressee | PostMailError::AddresseeMismatch) => {
                // Defensive: we've already gated empty recipients
                // upstream (and the empty-to-ALL reroute means the
                // rule never sees an empty `to_name` from the menu).
                // The rule's gates fire only if a future refactor
                // lets an invalid combination slip past the editor.
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            Err(PostMailError::AddressingNotAllowed) => {
                self.write_and_flush(POST_ADDRESSING_NOT_ALLOWED_LINE)
                    .await?;
            }
            Err(PostMailError::Store(err)) => {
                eprintln!("{command_label} command: failed to persist mail: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    /// Returns the [`AllowedAddressing`] for the supplied msgbase, or
    /// `None` if the coordinate isn't in the conference catalogue.
    fn msgbase_allowed_addressing(
        &self,
        visit_msgbase: MessageBaseRef,
    ) -> Option<AllowedAddressing> {
        self.services
            .conferences()
            .iter()
            .find(|c| c.number() == visit_msgbase.conference_number())?
            .msgbases()
            .iter()
            .find(|m| m.number() == visit_msgbase.msgbase_number())
            .map(crate::domain::conference::MessageBase::allowed_addressing)
    }

    /// Returns the [`AllScanScope`] for the supplied msgbase, or
    /// `None` if the coordinate isn't in the conference catalogue.
    fn msgbase_all_scan_scope(
        &self,
        visit_msgbase: MessageBaseRef,
    ) -> Option<crate::domain::conference::AllScanScope> {
        self.services
            .conferences()
            .iter()
            .find(|c| c.number() == visit_msgbase.conference_number())?
            .msgbases()
            .iter()
            .find(|m| m.number() == visit_msgbase.msgbase_number())
            .map(crate::domain::conference::MessageBase::all_scan_scope)
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

    /// Handles a `J <num>` command from the menu (Slice 32). Writes
    /// the legacy "no access" notice when the resolver fell through,
    /// the inline `Joining Conference: <name>` announcement on
    /// success, any name-type promotion screen (Slice 34), and then
    /// fires Slice 41's `ScanMailOnJoin` against the new visit.
    async fn handle_explicit_join(
        &mut self,
        session: MenuSession,
        target_conference_number: u32,
    ) -> Result<ExplicitJoinResult, T::Error> {
        let conferences = self.services.conferences();
        let outcome = session.explicit_join_conference(
            target_conference_number,
            conferences,
            SystemTime::now(),
        );
        match outcome {
            ExplicitJoinTransition::Joined {
                mut session,
                conference_number,
                msgbase_number,
                matched_request,
                name_type_promoted_to,
                ..
            } => {
                // Compute the announcement bytes up-front so the
                // immutable borrow on `self.services.conferences()`
                // doesn't overlap the mutable borrows below.
                let line =
                    format_explicit_join_line(conferences, conference_number, msgbase_number);
                if !matched_request {
                    self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                        .await?;
                }
                self.write_and_flush(&line).await?;
                render_name_type_promotion(
                    self.terminal,
                    self.services.screens(),
                    name_type_promoted_to,
                )
                .await?;
                crate::app::mail_scan_on_join::scan_mail_on_join(
                    self.terminal,
                    self.services,
                    &mut session,
                    crate::app::mail_scan_on_join::JoinScanMode::FollowPointer,
                )
                .await?;
                Ok(ExplicitJoinResult::Joined(session))
            }
            ExplicitJoinTransition::NoAccess(logging_off) => {
                self.write_and_flush(NO_CONFERENCE_ACCESS_LINE).await?;
                Ok(ExplicitJoinResult::NoAccess(logging_off))
            }
        }
    }

    /// Handles a `C` command from the menu (Slice 44). Resolves the
    /// sysop through the user repository, walks subject/body prompts
    /// (no recipient prompt, no private toggle — the rule fixes both),
    /// and invokes `messaging.allium:PostCommentToSysop` via the typed
    /// session.
    async fn handle_comment_to_sysop(&mut self, session: &mut MenuSession) -> Result<(), T::Error> {
        let Some(visit_msgbase) = session
            .current_msgbase()
            .map(|(conf, mb)| MessageBaseRef::new(conf, mb))
        else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(store) = self.services.mail_stores().for_msgbase(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let sysop = match self.services.user_repo().find_sysop() {
            NameLookupResult::Found(user) => *user,
            NameLookupResult::NotFound => {
                self.write_and_flush(NO_SYSOP_LINE).await?;
                return Ok(());
            }
        };

        let Some(allowed_addressing) = self.msgbase_allowed_addressing(visit_msgbase) else {
            self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            return Ok(());
        };

        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT)
            .await?
        else {
            return Ok(());
        };
        let Some(body) = self.read_post_body(session).await? else {
            return Ok(());
        };

        let from_name = session.user().handle().to_string();
        let sysop_slot = sysop.slot_number();
        let mut guard = store.lock().await;
        let result = session.post_comment_to_sysop(
            visit_msgbase,
            allowed_addressing,
            &mut **guard,
            crate::domain::messaging::post_comment_to_sysop::CommentToSysopDraft {
                sysop_slot,
                from_name,
                subject,
                body,
                posted_at: SystemTime::now(),
            },
        );
        drop(guard);

        self.render_post_result(result, "C").await
    }

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        let timeout = self.services.session_policy().input_timeout();
        crate::app::terminal::read_prompted(self.terminal, prompt, echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        crate::app::terminal::write_and_flush(self.terminal, bytes).await
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
