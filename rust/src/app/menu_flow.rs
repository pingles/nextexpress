//! Menu sub-flow: command loop and dispatch.
//!
//! Runs once the session is onboarded and joined to a conference.
//! Reads command lines, dispatches the supported ones (Phase 4
//! supports `G` for logoff and `J <num>` for explicit conference
//! join; Phase 6 adds `R <num>` for reading a single message) and
//! reports back to the driver when the loop terminates.

use std::time::SystemTime;

use crate::app::services::AppServices;
use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::typed_session::{
    ExplicitJoinTransition, LoggingOffSession, MenuSession, ScanOnJoin,
};
use crate::app::wire_text::{
    render_mail_body, render_mail_header, render_post_success, render_scan_summary,
    DELETED_MESSAGE_LINE, GOODBYE_LINE, IDLE_TIMEOUT_LINE, INVALID_CONFERENCE_NUMBER_LINE,
    INVALID_MESSAGE_NUMBER_LINE, JOIN_REQUIRES_NUMBER_LINE, MAIL_STORE_ERROR_LINE, MENU_PROMPT,
    MESSAGE_NOT_FOUND_LINE, NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, NO_CONFERENCE_ACCESS_LINE,
    NO_MAIL_BASE_LINE, POST_ABORTED_LINE, POST_ACCESS_DENIED_LINE, POST_BODY_PROMPT,
    POST_PRIVATE_PROMPT, POST_RECIPIENT_NO_ACCESS_LINE, POST_SUBJECT_PROMPT, POST_TO_PROMPT,
    POST_UNKNOWN_USER_LINE, READ_DENIED_LINE, READ_REQUIRES_NUMBER_LINE, UNKNOWN_COMMAND_LINE,
};
use crate::domain::conference::MessageBaseRef;
use crate::domain::post_mail::{PostMailDraft, PostMailError};
use crate::domain::read_mail::ReadMailError;
use crate::domain::user_repository::NameLookupResult;

/// Parsed shape of a `J <number>` command. Returned by
/// [`parse_join_command`].
enum JoinArg {
    /// `J <n>` where `<n>` parsed as a `u32`.
    Number(u32),
    /// `J` (or `J ` / `J\t`) with no number.
    Missing,
    /// `J <token>` where `<token>` could not be parsed as a `u32`.
    Invalid,
}

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
            if trimmed.eq_ignore_ascii_case("G") {
                let logging_off = session.user_requests_logoff();
                self.write_and_flush(GOODBYE_LINE).await?;
                return Ok(logging_off);
            }
            if let Some(arg) = parse_join_command(trimmed) {
                match arg {
                    JoinArg::Number(n) => {
                        session = match self.handle_explicit_join(session, n).await? {
                            ExplicitJoinResult::Joined(menu) => menu,
                            ExplicitJoinResult::NoAccess(logging_off) => return Ok(logging_off),
                        };
                    }
                    JoinArg::Missing => {
                        self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?;
                    }
                    JoinArg::Invalid => {
                        self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
                    }
                }
                continue;
            }
            if let Some(arg) = parse_read_command(trimmed) {
                match arg {
                    ReadArg::Number(n) => {
                        self.handle_read_mail(&mut session, n).await?;
                    }
                    ReadArg::Missing => {
                        self.write_and_flush(READ_REQUIRES_NUMBER_LINE).await?;
                    }
                    ReadArg::Invalid => {
                        self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
                    }
                }
                continue;
            }
            if let Some(scan) = parse_scan_command(trimmed) {
                self.handle_scan_mail(&mut session, scan).await?;
                continue;
            }
            if let Some(post) = parse_post_command(trimmed) {
                self.handle_post_mail(&mut session, post).await?;
                continue;
            }
            self.terminal.write(UNKNOWN_COMMAND_LINE).await?;
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

        let guard = store.lock().await;
        let result =
            match session.scan_mail(&**guard, visit_msgbase, from_message, SystemTime::now()) {
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
        // inline; bare `E` prompts. Empty recipient aborts in Slice 42
        // — the ALL reroute lands in Slice 43.
        let to_name = match arg {
            PostArg::To(name) => name,
            PostArg::Missing => match self.read_required_line(session, POST_TO_PROMPT).await? {
                Some(name) => name,
                None => return Ok(()),
            },
        };

        // Step 2: resolve the addressee through the user repository
        // and confirm they have a granted membership for the current
        // conference.
        let addressee = match self.services.user_repo().find_by_handle(&to_name) {
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
        if !addressee.has_membership(conference) {
            self.write_and_flush(POST_RECIPIENT_NO_ACCESS_LINE).await?;
            return Ok(());
        }

        // Step 3: subject prompt. Empty subject aborts (mirrors
        // `amiexpress/express.e:10854-10857`).
        let Some(subject) = self
            .read_required_line(session, POST_SUBJECT_PROMPT)
            .await?
        else {
            return Ok(());
        };

        // Step 4: private flag. Default is N if the user just hits CR.
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
        let addressee_slot = addressee.slot_number();
        let addressee_handle = addressee.handle().to_string();

        let mut guard = store.lock().await;
        let result = session.post_mail(
            visit_msgbase,
            &mut **guard,
            PostMailDraft {
                to_name: addressee_handle,
                addressee_slot,
                from_name: author_handle,
                subject,
                body,
                private,
                posted_at: SystemTime::now(),
            },
        );
        drop(guard);

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
            Err(PostMailError::EmptyAddressee) => {
                // Defensive: we've already gated empty recipients
                // upstream. The rule's gate fires only if a future
                // refactor lets an empty name slip past the editor.
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            Err(PostMailError::Store(err)) => {
                eprintln!("E command: failed to persist mail: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
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

    async fn read_prompted(
        &mut self,
        prompt: &[u8],
        echo: TerminalEcho,
    ) -> Result<TerminalRead, T::Error> {
        self.terminal.write(prompt).await?;
        self.terminal.flush().await?;
        let timeout = self.services.session_policy().input_timeout();
        self.terminal.read_line(echo, timeout).await
    }

    async fn write_and_flush(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        self.terminal.write(bytes).await?;
        self.terminal.flush().await
    }
}

/// Recognises the Phase-4 `J` / `J <num>` menu command. Returns
/// `None` for any other typed line so the menu loop can fall
/// through to its existing dispatch (currently only `G`). Mirrors
/// the legacy parsing in `amiexpress/express.e:25140` modulo the
/// `getInverse` macro, which Phase 4 doesn't model yet.
fn parse_join_command(line: &str) -> Option<JoinArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("J") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(JoinArg::Missing);
    };
    if tokens.next().is_some() {
        // Extra trailing tokens are treated as a malformed argument
        // rather than silently ignored.
        return Some(JoinArg::Invalid);
    }
    match arg.parse::<u32>() {
        Ok(n) => Some(JoinArg::Number(n)),
        Err(_) => Some(JoinArg::Invalid),
    }
}

/// Parsed shape of an `R <number>` command (Slice 39). Returned by
/// [`parse_read_command`].
enum ReadArg {
    /// `R <n>` where `<n>` parsed as a `u32`.
    Number(u32),
    /// `R` (or `R ` / `R\t`) with no number.
    Missing,
    /// `R <token>` where `<token>` could not be parsed as a `u32`.
    Invalid,
}

/// Parsed shape of an `M` / `N` command (Slice 40). Returned by
/// [`parse_scan_command`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanArg {
    /// `N` — scan from `last_scanned + 1`. Surfaces unread mail
    /// the user has not yet been alerted to.
    New,
    /// `M` — scan from message 1. Lists every message visible to
    /// the user in the current msgbase as the unread set.
    All,
}

/// Recognises the Phase-6 `M` / `N` menu commands. Returns `None`
/// for any other typed line, including the legacy `M` (toggle ANSI
/// colour) and `N` (list new files) which the Rust port still
/// owes a slice — Slice 40 routes `M` and `N` to mail-scan as the
/// spec's `ScanMail` rule demands; the legacy mappings will be
/// renamed if/when those features land.
fn parse_scan_command(line: &str) -> Option<ScanArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if tokens.next().is_some() {
        // `M ...` and `N ...` reject extra tokens; the simplified
        // dispatcher does not yet accept a numeric `from_message`
        // argument from the wire. Add as a future refinement.
        return None;
    }
    if head.eq_ignore_ascii_case("M") {
        Some(ScanArg::All)
    } else if head.eq_ignore_ascii_case("N") {
        Some(ScanArg::New)
    } else {
        None
    }
}

/// Parsed shape of an `E` / `E <to>` command (Slice 42). Returned by
/// [`parse_post_command`].
enum PostArg {
    /// `E <to>` where `<to>` is one-or-more tokens after the command
    /// (kept verbatim — real-name conferences accept multi-word
    /// handles like "John Smith").
    To(String),
    /// `E` with no inline recipient. The handler prompts for it.
    Missing,
}

/// Recognises the Phase-7 `E` / `E <to>` menu command (Slice 42).
/// Returns `None` for any other typed line. Mirrors the legacy
/// `enterMSG` inline-recipient shortcut at
/// `amiexpress/express.e:10765-10773`.
fn parse_post_command(line: &str) -> Option<PostArg> {
    let mut chars = line.chars();
    let head = chars.next()?;
    if !matches!(head, 'E' | 'e') {
        return None;
    }
    let rest: String = chars.collect();
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        // Bare `E` (or `E ` / `E\t`): no inline recipient — the
        // handler prompts. But reject anything else that starts with
        // 'E' but has no whitespace separator, e.g. `EM`.
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(PostArg::Missing);
        }
        return None;
    }
    if !rest.starts_with(char::is_whitespace) {
        // `EM`, `Edit`, etc — not the `E` command.
        return None;
    }
    Some(PostArg::To(trimmed.to_string()))
}

/// Recognises the Phase-6 `R` / `R <num>` menu command. Returns
/// `None` for any other typed line. Mirrors the parameter shape of
/// [`parse_join_command`]; the read sub-flow that accepts `+` / `-`
/// step forms (`amiexpress/express.e:12002`) lands in a future
/// slice that models the read-message navigation loop.
fn parse_read_command(line: &str) -> Option<ReadArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("R") {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(ReadArg::Missing);
    };
    if tokens.next().is_some() {
        return Some(ReadArg::Invalid);
    }
    match arg.parse::<u32>() {
        Ok(n) => Some(ReadArg::Number(n)),
        Err(_) => Some(ReadArg::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_read_command_accepts_uppercase_and_lowercase() {
        let matches = ["R 7", "r 7"];
        for input in &matches {
            match parse_read_command(input) {
                Some(ReadArg::Number(7)) => {}
                other => panic!("expected R 7 to parse as Number(7), got {other:?} for {input}"),
            }
        }
    }

    #[test]
    fn parse_read_command_returns_missing_for_bare_r() {
        match parse_read_command("R") {
            Some(ReadArg::Missing) => {}
            other => panic!("expected Missing, got {other:?}"),
        }
    }

    #[test]
    fn parse_read_command_treats_non_numeric_arg_as_invalid() {
        match parse_read_command("R foo") {
            Some(ReadArg::Invalid) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn parse_read_command_rejects_extra_trailing_tokens() {
        match parse_read_command("R 1 2") {
            Some(ReadArg::Invalid) => {}
            other => panic!("expected Invalid for extra tokens, got {other:?}"),
        }
    }

    #[test]
    fn parse_read_command_returns_none_for_unrelated_commands() {
        // `G`, `J 1`, etc. must fall through so the existing
        // dispatch paths still match them.
        assert!(parse_read_command("G").is_none());
        assert!(parse_read_command("J 1").is_none());
        assert!(parse_read_command("").is_none());
        assert!(parse_read_command("Read 1").is_none());
    }

    #[test]
    fn parse_scan_command_recognises_m_as_scan_all() {
        assert_eq!(parse_scan_command("M"), Some(ScanArg::All));
        assert_eq!(parse_scan_command("m"), Some(ScanArg::All));
    }

    #[test]
    fn parse_scan_command_recognises_n_as_scan_new() {
        assert_eq!(parse_scan_command("N"), Some(ScanArg::New));
        assert_eq!(parse_scan_command("n"), Some(ScanArg::New));
    }

    #[test]
    fn parse_scan_command_returns_none_for_unrelated_commands() {
        assert!(parse_scan_command("G").is_none());
        assert!(parse_scan_command("J 1").is_none());
        assert!(parse_scan_command("R 1").is_none());
        assert!(parse_scan_command("MS").is_none());
        assert!(parse_scan_command("").is_none());
    }

    #[test]
    fn parse_scan_command_rejects_extra_tokens() {
        // Future enhancement: `M <from>` accepted at the wire.
        // Until then, extra tokens make the command fall through.
        assert!(parse_scan_command("M 1").is_none());
        assert!(parse_scan_command("N 7").is_none());
    }

    #[test]
    fn parse_read_command_zero_is_a_valid_number_to_parse_but_will_404_at_load_time() {
        // `R 0` parses as u32(0). The mail store always uses
        // 1-indexed numbering so a `load(0)` returns `None` and the
        // dispatch surfaces `Message not found`. Pin the parse so a
        // future regression that rejected `0` at parse time would
        // observe.
        match parse_read_command("R 0") {
            Some(ReadArg::Number(0)) => {}
            other => panic!("expected Number(0), got {other:?}"),
        }
    }

    // `ReadArg` is a non-public enum without Debug; provide a manual
    // impl for the test panic messages above. (Keeping the variant
    // names off the production type avoids spreading derives that
    // production callers don't need.)
    impl std::fmt::Debug for ReadArg {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Number(n) => write!(f, "Number({n})"),
                Self::Missing => write!(f, "Missing"),
                Self::Invalid => write!(f, "Invalid"),
            }
        }
    }

    impl std::fmt::Debug for PostArg {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::To(name) => write!(f, "To({name:?})"),
                Self::Missing => write!(f, "Missing"),
            }
        }
    }

    #[test]
    fn parse_post_command_recognises_bare_e_with_no_addressee() {
        // Slice 42: typing `E` alone enters the line-mode editor;
        // the handler prompts for the recipient interactively.
        assert!(matches!(parse_post_command("E"), Some(PostArg::Missing)));
        assert!(matches!(parse_post_command("e"), Some(PostArg::Missing)));
    }

    #[test]
    fn parse_post_command_accepts_inline_addressee() {
        // Slice 42: `E <handle>` skips the To: prompt and treats the
        // argument as the recipient name. Matches the legacy
        // `enterMSG` shortcut at `amiexpress/express.e:10765-10773`.
        match parse_post_command("E bob") {
            Some(PostArg::To(name)) if name == "bob" => {}
            other => panic!("expected To(bob), got {other:?}"),
        }
        match parse_post_command("e Bob") {
            Some(PostArg::To(name)) if name == "Bob" => {}
            other => panic!("expected To(Bob), got {other:?}"),
        }
    }

    #[test]
    fn parse_post_command_joins_multi_word_addressees() {
        // Real names (`John Smith`) are valid handles in `RealName`
        // conferences. The parser must keep every token after `E` so
        // the resolver sees the full string.
        match parse_post_command("E John Smith") {
            Some(PostArg::To(name)) if name == "John Smith" => {}
            other => panic!("expected To(John Smith), got {other:?}"),
        }
    }

    #[test]
    fn parse_post_command_returns_none_for_unrelated_commands() {
        assert!(parse_post_command("G").is_none());
        assert!(parse_post_command("J 1").is_none());
        assert!(parse_post_command("R 1").is_none());
        assert!(parse_post_command("").is_none());
        assert!(parse_post_command("EM").is_none());
    }
}
