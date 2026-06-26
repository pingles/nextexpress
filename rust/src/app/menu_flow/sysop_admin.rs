//! Sysop mail-admin read sub-prompt commands (Slice 49b):
//! `K <num>` (kill / delete), `MV <num>` (move) and `EH <num>`
//! (edit header).
//!
//! The terminal-free cores ([`delete_mail`], [`move_mail`],
//! [`edit_mail_header`]) own store/repository resolution and the
//! domain-rule invocation through the typed [`MenuSession`]; the
//! `MenuFlow` handlers below drive the prompt loops and render the
//! outcomes.

use crate::app::mail_stores::{MailStorePairLockOutcome, MailStores};
use crate::app::menu_flow::mail_text::{
    FORWARD_UNKNOWN_USER_LINE, MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, POST_ABORTED_LINE,
    SOURCE_NOT_FOUND_LINE,
};
use crate::app::terminal::Terminal;
use crate::app::wire_text::INVALID_MESSAGE_NUMBER_LINE;
use crate::domain::conference::{Conference, MessageBaseRef};
use crate::domain::messaging::delete_mail::{delete_mail as delete_mail_rule, DeleteMailError};
use crate::domain::messaging::edit_mail_header::{
    edit_mail_header as edit_mail_header_rule, EditMailHeaderError,
};
use crate::domain::messaging::mail::Mail;
use crate::domain::messaging::move_mail::{move_mail as move_mail_rule, MoveMailError};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::{NameLookupResult, UserRepository, UserRepositoryError};

/// Sent when the `MV` sub-command's target cannot be parsed as a
/// conference number.
const INVALID_CONFERENCE_NUMBER_LINE: &[u8] = b"\r\nInvalid conference number.\r\n";

/// Prompt asking the user to confirm a destructive operation
/// (Slice 49b's `K` delete). Defaults to `N` so an idle CR is safe.
#[allow(dead_code, reason = "Wired up in Slice 49b")]
const CONFIRM_DELETE_PROMPT: &[u8] = b"Delete message (y/N)? ";

/// Sent when a sysop-only command (Slice 49b) is invoked by a user
/// without the required access level / right.
const SYSOP_ONLY_LINE: &[u8] = b"\r\nYou do not have permission to perform that operation.\r\n";

/// Prompt for the target conference number of an `MV <num>` move
/// (Slice 49b).
const MOVE_TARGET_CONFERENCE_PROMPT: &[u8] = b"\r\nTarget conference number: ";

/// Prompt for the target msgbase number of an `MV <num>` move
/// (Slice 49b).
const MOVE_TARGET_MSGBASE_PROMPT: &[u8] = b"Target msgbase number: ";

/// Sent when an `MV <num>` references a target msgbase that's not
/// registered with the running BBS.
const MOVE_UNKNOWN_TARGET_LINE: &[u8] = b"\r\nNo such target message base.\r\n";

/// Confirmation line printed after a successful `K <num>` delete.
const DELETE_DONE_LINE: &[u8] = b"\r\nMessage deleted.\r\n";

/// Confirmation line printed after a successful `MV <num>` move.
/// Includes the new number so the user can navigate to it.
const MOVE_DONE_PREFIX: &[u8] = b"\r\nMessage moved. New number ";

/// Confirmation line printed after a successful `EH <num>` edit.
const EDIT_HEADER_DONE_LINE: &[u8] = b"\r\nHeader updated.\r\n";

/// Prompt for the new subject during an `EH <num>` header edit.
/// Empty input keeps the current subject.
const EDIT_HEADER_SUBJECT_PROMPT: &[u8] = b"New subject (blank = unchanged): ";

/// Prompt for the new addressee during an `EH <num>` header edit.
/// Empty input keeps the current addressee.
const EDIT_HEADER_TO_PROMPT: &[u8] = b"New To (blank = unchanged): ";

/// Outcome of a `K <num>` command.
enum DeleteOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The command was completed.
    Done,
    /// The domain rule rejected the request.
    Rejected(DeleteMailError),
}

/// Already-collected fields for an `MV <num>` command.
struct MoveInput {
    /// Source mail number in the current msgbase.
    source_number: u32,
    /// Target conference number.
    target_conference: u32,
    /// Target msgbase number inside `target_conference`.
    target_msgbase: u32,
}

/// Outcome of an `MV <num>` command.
enum MoveOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The supplied target msgbase coordinate is not registered.
    UnknownTarget,
    /// The mail was moved; `mail` is the new row at the target.
    Moved(Mail),
    /// The domain rule rejected the request.
    Rejected(MoveMailError),
}

/// Already-collected fields for an `EH <num>` command.
struct EditHeaderInput {
    /// Source mail number in the current msgbase.
    source_number: u32,
    /// New subject; `None` leaves the subject unchanged.
    new_subject: Option<String>,
    /// New addressee handle; `None` leaves the addressee unchanged.
    /// The repository lookup is performed by this use case.
    new_to_name: Option<String>,
}

/// Outcome of an `EH <num>` command.
enum EditHeaderOutcome {
    /// The session has no current message base.
    NoMailBase,
    /// The supplied new addressee could not be resolved.
    UnknownAddressee,
    /// A repository lookup failed while resolving the new addressee.
    LookupFailed(UserRepositoryError),
    /// The edit was applied.
    Done,
    /// The domain rule rejected the request.
    Rejected(EditMailHeaderError),
}

/// Runs the delete-mail use case (Slice 49 wired) without terminal I/O.
async fn delete_mail<M>(session: &mut MenuSession, mail_stores: &M, number: u32) -> DeleteOutcome
where
    M: MailStores + ?Sized,
{
    let Some((_, mut guard)) = super::lock_current_base(session, mail_stores).await else {
        return DeleteOutcome::NoMailBase;
    };
    let result = delete_mail_rule(session.user_mut(), &mut *guard, number);
    drop(guard);
    match result {
        Ok(()) => DeleteOutcome::Done,
        Err(err) => DeleteOutcome::Rejected(err),
    }
}

/// Runs the move-mail use case (Slice 49 wired) without terminal I/O.
async fn move_mail<M>(session: &mut MenuSession, mail_stores: &M, input: MoveInput) -> MoveOutcome
where
    M: MailStores + ?Sized,
{
    let Some(source_msgbase) = super::current_base(session) else {
        return MoveOutcome::NoMailBase;
    };
    let target_msgbase = MessageBaseRef::new(input.target_conference, input.target_msgbase);
    let (mut source_guard, mut target_guard) =
        match mail_stores.lock_pair(source_msgbase, target_msgbase).await {
            MailStorePairLockOutcome::MissingSource => return MoveOutcome::NoMailBase,
            MailStorePairLockOutcome::MissingTarget => return MoveOutcome::UnknownTarget,
            // The domain rule rejects same-msgbase moves; the registry
            // short-circuits before locking so it can't deadlock on a
            // shared mutex. Surface the rejection through the existing
            // domain error variant for callers.
            MailStorePairLockOutcome::SameStore => {
                return MoveOutcome::Rejected(MoveMailError::SameMsgbase);
            }
            MailStorePairLockOutcome::Locked { source, target } => (source, target),
        };
    let result = move_mail_rule(
        session.user_mut(),
        &mut *source_guard,
        &mut *target_guard,
        input.source_number,
    );
    drop(target_guard);
    drop(source_guard);
    match result {
        Ok(mail) => MoveOutcome::Moved(mail),
        Err(err) => MoveOutcome::Rejected(err),
    }
}

/// Runs the edit-mail-header use case (Slice 49 wired).
async fn edit_mail_header<R, M>(
    session: &mut MenuSession,
    user_repo: &R,
    mail_stores: &M,
    _conferences: &[Conference],
    input: EditHeaderInput,
) -> EditHeaderOutcome
where
    R: UserRepository + ?Sized,
    M: MailStores + ?Sized,
{
    let Some((_, mut guard)) = super::lock_current_base(session, mail_stores).await else {
        return EditHeaderOutcome::NoMailBase;
    };

    let new_to = if let Some(typed) = input.new_to_name {
        let trimmed = typed.trim();
        if trimmed.is_empty() {
            None
        } else {
            match user_repo.find_by_handle(trimmed) {
                Ok(NameLookupResult::Found(user)) => {
                    Some((user.handle().to_string(), Some(user.slot_number())))
                }
                Ok(NameLookupResult::NotFound) => return EditHeaderOutcome::UnknownAddressee,
                Err(error) => return EditHeaderOutcome::LookupFailed(error),
            }
        }
    } else {
        None
    };

    let result = edit_mail_header_rule(
        session.user_mut(),
        &mut *guard,
        input.source_number,
        input.new_subject,
        new_to,
    );
    drop(guard);
    match result {
        Ok(()) => EditHeaderOutcome::Done,
        Err(err) => EditHeaderOutcome::Rejected(err),
    }
}

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Handles `K <num>` — prompt for confirmation, then delete.
    pub(super) async fn handle_kill(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        let Some(line) = self
            .read_required_line(session, CONFIRM_DELETE_PROMPT, false)
            .await?
        else {
            return Ok(());
        };
        if !matches!(line.chars().next(), Some('y' | 'Y')) {
            self.write_and_flush(POST_ABORTED_LINE).await?;
            return Ok(());
        }

        let outcome = delete_mail(session, self.services.mail_stores.as_ref(), number).await;
        match outcome {
            DeleteOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            DeleteOutcome::Done => {
                self.write_and_flush(DELETE_DONE_LINE).await?;
            }
            DeleteOutcome::Rejected(err) => {
                self.render_delete_error(err).await?;
            }
        }
        Ok(())
    }

    /// Handles `MV <num>` — prompt for the target conference +
    /// msgbase numbers, then move. Returns `true` only when the
    /// message was actually moved, so the read sub-prompt can honour
    /// the legacy "advance only on a successful move" navigation
    /// (`express.e:12172`); every abort / rejection returns `false`.
    pub(super) async fn handle_move_mail(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<bool, T::Error> {
        let Some(conf_line) = self
            .read_required_line(session, MOVE_TARGET_CONFERENCE_PROMPT, false)
            .await?
        else {
            return Ok(false);
        };
        let Ok(target_conf) = conf_line.parse::<u32>() else {
            self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?;
            return Ok(false);
        };
        let Some(mb_line) = self
            .read_required_line(session, MOVE_TARGET_MSGBASE_PROMPT, false)
            .await?
        else {
            return Ok(false);
        };
        let Ok(target_mb) = mb_line.parse::<u32>() else {
            self.write_and_flush(INVALID_MESSAGE_NUMBER_LINE).await?;
            return Ok(false);
        };

        let outcome = move_mail(
            session,
            self.services.mail_stores.as_ref(),
            MoveInput {
                source_number: number,
                target_conference: target_conf,
                target_msgbase: target_mb,
            },
        )
        .await;
        let moved = match outcome {
            MoveOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
                false
            }
            MoveOutcome::UnknownTarget => {
                self.write_and_flush(MOVE_UNKNOWN_TARGET_LINE).await?;
                false
            }
            MoveOutcome::Moved(mail) => {
                let mut line = MOVE_DONE_PREFIX.to_vec();
                line.extend_from_slice(mail.number().to_string().as_bytes());
                line.extend_from_slice(b".\r\n");
                self.write_and_flush(&line).await?;
                true
            }
            MoveOutcome::Rejected(err) => {
                self.render_move_error(err).await?;
                false
            }
        };
        Ok(moved)
    }

    /// Handles `EH <num>` — prompt for new subject and/or new
    /// addressee (blank keeps), then edit.
    pub(super) async fn handle_edit_header(
        &mut self,
        session: &mut MenuSession,
        number: u32,
    ) -> Result<(), T::Error> {
        let Some(new_subject) = self
            .read_optional_unchanged_line(session, EDIT_HEADER_SUBJECT_PROMPT)
            .await?
        else {
            return Ok(());
        };
        let Some(new_to_name) = self
            .read_optional_unchanged_line(session, EDIT_HEADER_TO_PROMPT)
            .await?
        else {
            return Ok(());
        };

        let outcome = edit_mail_header(
            session,
            self.services.user_repo.as_ref(),
            self.services.mail_stores.as_ref(),
            self.services.conferences.as_ref(),
            EditHeaderInput {
                source_number: number,
                new_subject,
                new_to_name,
            },
        )
        .await;

        match outcome {
            EditHeaderOutcome::NoMailBase => {
                self.write_and_flush(NO_MAIL_BASE_LINE).await?;
            }
            EditHeaderOutcome::UnknownAddressee => {
                self.write_and_flush(FORWARD_UNKNOWN_USER_LINE).await?;
            }
            EditHeaderOutcome::LookupFailed(error) => {
                eprintln!("EH command: failed to resolve user: {error}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
            EditHeaderOutcome::Done => {
                self.write_and_flush(EDIT_HEADER_DONE_LINE).await?;
            }
            EditHeaderOutcome::Rejected(err) => {
                self.render_edit_header_error(err).await?;
            }
        }
        Ok(())
    }

    async fn render_delete_error(&mut self, err: DeleteMailError) -> Result<(), T::Error> {
        match err {
            DeleteMailError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            DeleteMailError::AlreadyDeleted => {
                // Mirror SOURCE_DELETED_LINE-ish surface: the user
                // tried to delete a deleted mail. Re-using
                // POST_ABORTED for now; bespoke wording can land
                // later.
                self.write_and_flush(POST_ABORTED_LINE).await?;
            }
            DeleteMailError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            DeleteMailError::Store(err) => {
                eprintln!("K command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    async fn render_move_error(&mut self, err: MoveMailError) -> Result<(), T::Error> {
        match err {
            MoveMailError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            MoveMailError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            MoveMailError::SameMsgbase => {
                self.write_and_flush(MOVE_UNKNOWN_TARGET_LINE).await?;
            }
            MoveMailError::Store(err) => {
                eprintln!("MV command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    async fn render_edit_header_error(&mut self, err: EditMailHeaderError) -> Result<(), T::Error> {
        match err {
            EditMailHeaderError::NotFound(_) => {
                self.write_and_flush(SOURCE_NOT_FOUND_LINE).await?;
            }
            EditMailHeaderError::NotPermitted => {
                self.write_and_flush(SYSOP_ONLY_LINE).await?;
            }
            EditMailHeaderError::Store(err) => {
                eprintln!("EH command: store error: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
            }
        }
        Ok(())
    }

    /// Reads a single trimmed line for an `EH` header field that may be
    /// left unchanged. The three-state result distinguishes the cases the
    /// caller must treat differently:
    ///
    /// * `Some(None)` — blank input: keep the current field value.
    /// * `Some(Some(value))` — a new value was supplied.
    /// * `None` — EOF / idle timeout: the edit is **aborted**.
    ///
    /// A dropped carrier or idle timeout must abort the whole edit rather
    /// than silently keep the field and commit. The legacy `editHeader`
    /// (`express.e:11602`) does `IF (stat < 0) THEN RETURN stat` on every
    /// prompt's `lineInput` timeout — a *silent* return, distinct from the
    /// blank-line keep branch — so the abort writes nothing here (the same
    /// convention as the `R`-sub-prompt reply / forward commands, B6).
    ///
    /// # Errors
    /// Returns the concrete terminal error if a write, flush, or read fails.
    async fn read_optional_unchanged_line(
        &mut self,
        session: &mut MenuSession,
        prompt: &[u8],
    ) -> Result<Option<Option<String>>, T::Error> {
        use crate::app::terminal::{TerminalEcho, TerminalRead};
        use std::time::SystemTime;
        match self.read_prompted(prompt, TerminalEcho::Visible).await? {
            TerminalRead::Line(line) => {
                session.record_input(SystemTime::now());
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    Ok(Some(None))
                } else {
                    Ok(Some(Some(trimmed.to_string())))
                }
            }
            TerminalRead::Eof | TerminalRead::IdleTimedOut => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::adapters::file_screen_repository::FileScreenRepository;
    use crate::adapters::in_memory_caller_log::InMemoryCallerLog;
    use crate::adapters::in_memory_mail_stores::InMemoryMailStores;
    use crate::adapters::in_memory_user_repository::InMemoryUserRepository;
    use crate::adapters::pbkdf2_password_hasher::Pbkdf2PasswordHasher;
    use crate::app::services::AppServices;
    use crate::app::session_flow::{DefaultRatio, NewUserGateConfig};
    use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
    use crate::domain::password::PasswordHashKind;
    use crate::domain::session::typed::MenuSession;
    use crate::domain::session::{apply_password_match, LogonChannel, Session, SessionPolicy};
    use crate::domain::user::{NewUserDraft, RatioMode, User};

    use super::{EDIT_HEADER_SUBJECT_PROMPT, EDIT_HEADER_TO_PROMPT, NO_MAIL_BASE_LINE};

    /// A terminal double that replays a scripted sequence of line reads
    /// (defaulting to `Eof` once exhausted) and records every written byte.
    #[derive(Default)]
    struct ScriptedTerminal {
        output: Vec<u8>,
        inputs: VecDeque<TerminalRead>,
    }

    impl Terminal for ScriptedTerminal {
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

    fn terminal_with(inputs: impl IntoIterator<Item = TerminalRead>) -> ScriptedTerminal {
        ScriptedTerminal {
            output: Vec::new(),
            inputs: inputs.into_iter().collect(),
        }
    }

    fn test_services() -> AppServices {
        AppServices {
            user_repo: Arc::new(InMemoryUserRepository::default()),
            hasher: Arc::new(Pbkdf2PasswordHasher::new()),
            caller_log: Arc::new(InMemoryCallerLog::new()),
            screens: Arc::new(FileScreenRepository::new(std::env::temp_dir())),
            conferences: Arc::new(Vec::new()),
            mail_stores: Arc::new(InMemoryMailStores::new()),
            file_repo: Arc::new(
                crate::adapters::in_memory_file_repository::InMemoryFileRepository::new(
                    Vec::new(),
                    Vec::new(),
                ),
            ),
            flagged_store: Arc::new(
                crate::adapters::in_memory_flagged_store::InMemoryFlaggedStore::new(),
            ),
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

    /// A Menu-phase session. The `EH` field prompts fire before any
    /// access gate or base lookup, so the session tier is irrelevant to
    /// the abort behaviour under test.
    fn menu_session() -> MenuSession {
        let user = User::register_new(
            7,
            NewUserDraft {
                handle: "sysop".to_string(),
                location: Some("Townsville".to_string()),
                phone_number: Some("555-0123".to_string()),
                email: Some("sysop@example.com".to_string()),
                password_hash: "hash".to_string(),
                password_salt: Some("salt".to_string()),
                password_hash_kind: PasswordHashKind::Pbkdf210000,
                line_length: 80,
                ansi_colour: true,
                flags: BTreeSet::new(),
                ratio_mode: RatioMode::Disabled,
                ratio_value: 0,
                now: SystemTime::UNIX_EPOCH,
            },
        )
        .expect("valid new user");
        let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
        session.prompt_for_name().expect("prompt");
        session
            .record_identified_user("sysop", user)
            .expect("identify");
        apply_password_match(
            &mut session,
            SessionPolicy::default(),
            SystemTime::UNIX_EPOCH,
        )
        .expect("password match");
        session.enter_menu(SystemTime::UNIX_EPOCH).expect("menu");
        MenuSession::from_session(session)
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[tokio::test]
    async fn edit_header_aborts_when_the_subject_prompt_times_out() {
        // A dropped carrier / idle timeout at the first `EH` prompt must
        // abort the edit outright — the legacy `editHeader`
        // (`express.e:11602`) does `IF (stat < 0) THEN RETURN stat` on the
        // prompt's `lineInput` timeout, NOT treat it as "keep the current
        // subject". The abort is silent, so only the subject prompt is
        // written: the addressee prompt never appears and the edit never
        // reaches the domain rule.
        let services = test_services();
        let mut terminal = terminal_with([TerminalRead::IdleTimedOut]);
        let mut session = menu_session();
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.handle_edit_header(&mut session, 1)
                .await
                .expect("handle_edit_header");
        }
        assert_eq!(
            terminal.output.as_slice(),
            EDIT_HEADER_SUBJECT_PROMPT,
            "a subject-prompt timeout must abort silently, writing only the prompt; got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }

    #[tokio::test]
    async fn edit_header_aborts_when_the_addressee_prompt_times_out() {
        // A timeout at the *second* `EH` prompt (after a real subject was
        // typed) must also abort silently — the partially-collected edit is
        // discarded and the domain rule is never reached. So the wire shows
        // exactly the two prompts and nothing after them.
        let services = test_services();
        let mut terminal = terminal_with([
            TerminalRead::Line("Updated subject".to_string()),
            TerminalRead::IdleTimedOut,
        ]);
        let mut session = menu_session();
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.handle_edit_header(&mut session, 1)
                .await
                .expect("handle_edit_header");
        }
        let expected = [EDIT_HEADER_SUBJECT_PROMPT, EDIT_HEADER_TO_PROMPT].concat();
        assert_eq!(
            terminal.output, expected,
            "an addressee-prompt timeout must abort silently after the two prompts, never reaching the edit rule; got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }

    #[tokio::test]
    async fn edit_header_keeps_both_fields_on_blank_input_and_proceeds() {
        // Regression guard for the keep-current path the abort fix must not
        // disturb: blank input is "leave unchanged" (NOT abort), so the edit
        // proceeds into the domain rule. With no joined base that surfaces as
        // the no-mail-base outcome, proving the edit was not short-circuited.
        let services = test_services();
        let mut terminal = terminal_with([
            TerminalRead::Line(String::new()),
            TerminalRead::Line("   ".to_string()),
        ]);
        let mut session = menu_session();
        {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.handle_edit_header(&mut session, 1)
                .await
                .expect("handle_edit_header");
        }
        let out = &terminal.output;
        assert!(
            contains(out, EDIT_HEADER_TO_PROMPT),
            "a blank subject keeps the current value and proceeds to the addressee prompt; got {:?}",
            String::from_utf8_lossy(out)
        );
        assert!(
            contains(out, NO_MAIL_BASE_LINE),
            "the kept-fields edit must reach the domain rule (no-base outcome here); got {:?}",
            String::from_utf8_lossy(out)
        );
    }
}
