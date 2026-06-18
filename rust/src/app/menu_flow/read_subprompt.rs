//! `R` read sub-prompt loop — the legacy `readMSG` primary mail-reading
//! UI (`amiexpress/express.e:11972`, the `cont:` loop at `:12008-12230`).
//!
//! The loop is entered two ways (slice B10): `R <num>` and the `MS`
//! read-it-now flow are READ-FIRST — `handle_read_mail` displays the
//! message, then enters with the pointer advanced past it; bare `R` is
//! PROMPT-FIRST — it renders the `Msg. Options:` prompt at the resume
//! range and reads the first message only on `<CR>`. The range lower
//! bound is always the NEXT message to read (the legacy increments
//! `msgNum` *after* `displayMessage`, `:12372`) and collapses to the
//! literal `( QUIT )` once the pointer passes the last message
//! (`:12010-12012`). `A` / `R` / `F` / `D` / `M` / `EH` operate on the
//! loaded message and are inert until one has been read (legacy
//! `tempFlag`, `:12087`); `<CR>` / `L` / `Q` / `?` / `??` are always
//! live.

use std::time::SystemTime;

use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_read_subprompt, render_read_subprompt_help, MAIL_STORE_ERROR_LINE,
};
use crate::domain::messaging::delete_mail::can_delete;
use crate::domain::messaging::edit_mail_header::can_edit_header;
use crate::domain::messaging::mail_store::MailStoreError;
use crate::domain::messaging::move_mail::can_move;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the read sub-prompt loop (legacy `readMSG` `cont:` loop,
    /// `express.e:12008-12230`). `next` is the next message to read (the
    /// legacy `msgNum` and the range's lower bound); `last_displayed` is
    /// the message currently loaded — the one `A`/`R`/`F`/`D`/`M`/`EH`
    /// act on — or `None` before any message has been shown (legacy
    /// `tempFlag = 0`, `:12087`). Bare `R` enters prompt-first with
    /// `last_displayed = None`; `R <num>` and the MS read-it-now flow
    /// enter after displaying the message with `next = num + 1`,
    /// `last_displayed = Some(num)`.
    pub(super) async fn run_read_subprompt(
        &mut self,
        session: &mut MenuSession,
        next: u32,
        last_displayed: Option<u32>,
    ) -> Result<(), T::Error> {
        let mut next = next;
        let mut last_displayed = last_displayed;
        // `?` / `??` set this so the next loop turn renders the short /
        // long help list instead of the option skeleton, then it clears.
        let mut pending_help: Option<bool> = None;
        loop {
            // Bounds are re-read every turn: `R`eply / `F`orward post new
            // messages while the reader holds the loop, so a value
            // captured on entry would go stale (the legacy reads the live
            // `mailStat` each pass).
            let Some((lowest, highest)) = (match self.current_base_bounds(session).await {
                Ok(bounds) => bounds,
                Err(error) => {
                    eprintln!("R sub-prompt: failed to determine message bounds: {error}");
                    self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                    return Ok(());
                }
            }) else {
                return Ok(());
            };
            // Range string (legacy `:12010-12012`): the lower bound is the
            // NEXT message to read (the legacy increments `msgNum` *after*
            // `displayMessage`, `:12372`), and an out-of-range pointer
            // collapses to the literal `QUIT`.
            let out_of_range = next > highest || next < lowest;
            let range: Vec<u8> = if out_of_range {
                b"QUIT".to_vec()
            } else {
                format!("{next}+{highest}").into_bytes()
            };
            // `D` / `M` are advertised for the message the option would
            // act on: the loaded message, else the one about to be read.
            // At the `QUIT`-from-start prompt there is no such message, so
            // neither is shown. (Per-message gating; the legacy per-user
            // `checkSecurity` ACS flags are slice B9.)
            let gate_msg = last_displayed.or((!out_of_range).then_some(next));
            let show_delete = match gate_msg {
                Some(m) => self.current_user_can_delete(session, m).await,
                None => false,
            };
            let show_move = gate_msg.is_some() && can_move(session.user());
            let prompt = match pending_help.take() {
                None => render_read_subprompt(&range, show_delete, show_move),
                Some(long) => render_read_subprompt_help(
                    long,
                    &range,
                    show_delete,
                    show_move,
                    can_edit_header(session.user()),
                ),
            };
            // A disconnected or idle caller leaves the sub-prompt; the
            // menu loop's next read applies carrier-loss / idle-timeout,
            // matching the other interactive handlers.
            let TerminalRead::Line(line) =
                self.read_prompted(&prompt, TerminalEcho::Visible).await?
            else {
                return Ok(());
            };
            session.record_input(SystemTime::now());
            let trimmed = line.trim();

            // `??` requests the long help, any other `?`-prefixed input
            // the short help (legacy `:12071-12078`); the next loop turn
            // renders it.
            if trimmed == "??" {
                pending_help = Some(true);
                continue;
            }
            if trimmed.starts_with('?') {
                pending_help = Some(false);
                continue;
            }

            // Empty input (`<CR>`) reads the next message (legacy
            // `goNextMsg`, `:12082`). An out-of-range pointer returns
            // silently to the menu (legacy `noDirF = 1` makes
            // `noMorePlus` silent, `:12302`).
            if trimmed.is_empty() {
                if !self
                    .advance(session, &mut next, &mut last_displayed)
                    .await?
                {
                    return Ok(());
                }
                continue;
            }

            // `A`/`R`/`F`/`D`/`M`/`EH` act on the loaded message and are
            // inert until one has been read (legacy `IF(tempFlag)`,
            // `:12087`). Before the first read only `Q` and `L` act.
            let Some(current) = last_displayed else {
                match trimmed.chars().next().map(|c| c.to_ascii_lowercase()) {
                    Some('q') => return Ok(()),
                    Some('l') => self.handle_list_messages(session).await?,
                    _ => {}
                }
                continue;
            };

            // A non-empty command on the loaded message; `Ok(false)`
            // means the option left the loop (legacy `Q` or an
            // out-of-range advance).
            if !self
                .dispatch_read_option(session, trimmed, current, &mut next, &mut last_displayed)
                .await?
            {
                return Ok(());
            }
        }
    }

    /// Handles one sub-prompt command on the loaded message `current`,
    /// mirroring the legacy `LowerChar(str[0])` SELECT
    /// (`express.e:12092-12224`). Returns `Ok(false)` when the loop
    /// should exit (legacy `Q`, `:12226`, or an out-of-range advance),
    /// `Ok(true)` to keep looping.
    async fn dispatch_read_option(
        &mut self,
        session: &mut MenuSession,
        trimmed: &str,
        current: u32,
        next: &mut u32,
        last_displayed: &mut Option<u32>,
    ) -> Result<bool, T::Error> {
        match trimmed.chars().next().map(|c| c.to_ascii_lowercase()) {
            // `Q`uit returns to the menu (`express.e:12226`).
            Some('q') => return Ok(false),
            // `A`gain re-displays the current message and stays on it
            // (`express.e:12102-12105`).
            Some('a') => {
                self.read_and_render(session, current).await?;
            }
            // `R`eply posts a reply to the current message, then advances
            // to the next one (`express.e:12161-12168`).
            Some('r') => {
                self.handle_reply(session, current).await?;
                if !self.advance(session, next, last_displayed).await? {
                    return Ok(false);
                }
            }
            // `F`orward forwards the current message, then stays on it
            // (`express.e:12153-12160`).
            Some('f') => {
                self.handle_forward(session, current).await?;
            }
            // `D`elete: gated on the per-message delete permission (legacy
            // `ACS_DELETE_MESSAGE`, `express.e:12148`). When permitted the
            // confirm-and-delete runs and the loop advances
            // unconditionally — the legacy `goNextMsg` fires whatever the
            // confirm answer (`:12151`). A caller without delete
            // permission falls through (the option is not theirs).
            Some('d') if self.current_user_can_delete(session, current).await => {
                self.handle_kill(session, current).await?;
                if !self.advance(session, next, last_displayed).await? {
                    return Ok(false);
                }
            }
            // `M`ove: gated on the move permission (legacy `ACS_SYSOP_READ`,
            // `express.e:12170`). Advances only on a successful move
            // (`:12172`); an aborted or rejected move stays.
            Some('m') if can_move(session.user()) => {
                let moved = self.handle_move_mail(session, current).await?;
                if moved && !self.advance(session, next, last_displayed).await? {
                    return Ok(false);
                }
            }
            // `EH` edits the current header (the `E`-family option, gated
            // on edit-header access — legacy `ACS_MESSAGE_EDIT`,
            // `express.e:12179`), then re-displays the edited message and
            // stays (`displayMessage` -> `nextMenu`, `:12191-12193`). Bare
            // `E` (emacs) / `EM` (body edit) are deliberately not carried
            // (see slice B5 Out of Scope).
            Some('e') if trimmed.eq_ignore_ascii_case("eh") && can_edit_header(session.user()) => {
                self.handle_edit_header(session, current).await?;
                self.read_and_render(session, current).await?;
            }
            // `L`ist the base's messages (`express.e:12220`), then stay on
            // the current message (`nextMenu`, `:12223`).
            Some('l') => {
                self.handle_list_messages(session).await?;
            }
            // Any other key is an unimplemented B5 option; fall through
            // and re-render the prompt.
            _ => {}
        }
        Ok(true)
    }

    /// Reads `*next` and displays it, advancing the pointer past it and
    /// recording it as the loaded message. Re-reads the base bounds so a
    /// just-posted reply is reachable. Returns `Ok(false)` when `*next`
    /// is out of range — the legacy out-of-range -> `QUIT` clamp
    /// (`express.e:12012` / `:12328`) — so the caller leaves the loop.
    async fn advance(
        &mut self,
        session: &mut MenuSession,
        next: &mut u32,
        last_displayed: &mut Option<u32>,
    ) -> Result<bool, T::Error> {
        let Some((lowest, highest)) = (match self.current_base_bounds(session).await {
            Ok(bounds) => bounds,
            Err(error) => {
                eprintln!("R sub-prompt: failed to determine message bounds: {error}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await?;
                return Ok(false);
            }
        }) else {
            return Ok(false);
        };
        if *next > highest || *next < lowest {
            return Ok(false);
        }
        self.read_and_render(session, *next).await?;
        *last_displayed = Some(*next);
        *next += 1;
        Ok(true)
    }

    /// Returns the `(lowest, highest)` existing message bounds in the
    /// session's current message base — the legacy `mailStat.lowestKey`
    /// (approximated by the lowest undeleted message) and
    /// `mailStat.highMsgNum - 1` (`express.e:12010-12012`).
    /// `Ok(None)` when the session has no current base or no store is
    /// registered for it.
    async fn current_base_bounds(
        &self,
        session: &MenuSession,
    ) -> Result<Option<(u32, u32)>, MailStoreError> {
        let Some((_, guard)) =
            super::lock_current_base(session, self.services.mail_stores.as_ref()).await
        else {
            return Ok(None);
        };
        let lowest = guard.lowest_undeleted_message()?;
        Ok(Some((lowest, guard.highest_message())))
    }

    /// True when the session's user may delete the current message
    /// (`number`). Loads the message and applies the `can_delete`
    /// predicate, releasing the store lock before returning so the
    /// delete handler can re-lock without self-deadlock. A missing
    /// base, an absent message or a load error all read as "not
    /// permitted".
    async fn current_user_can_delete(&self, session: &MenuSession, number: u32) -> bool {
        let Some((_, guard)) =
            super::lock_current_base(session, self.services.mail_stores.as_ref()).await
        else {
            return false;
        };
        let permitted = match guard.load(number) {
            Ok(Some(mail)) => can_delete(session.user(), &mail),
            _ => false,
        };
        drop(guard);
        permitted
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
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

    #[derive(Default)]
    struct CaptureTerminal {
        output: Vec<u8>,
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
            Box::pin(async { Ok(TerminalRead::Eof) })
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

    /// A Menu-phase session bound to an awaiting-validation new user —
    /// the only tier where `can_move` / `can_edit_header` are denied.
    fn menu_session_for_new_user() -> MenuSession {
        let user = User::register_new(
            7,
            NewUserDraft {
                handle: "newbie".to_string(),
                location: Some("Townsville".to_string()),
                phone_number: Some("555-0123".to_string()),
                email: Some("newbie@example.com".to_string()),
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
            .record_identified_user("newbie", user)
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

    #[tokio::test]
    async fn move_option_is_inert_for_a_user_without_move_access() {
        // The `M` arm is guarded by `can_move` (legacy `ACS_SYSOP_READ`
        // gate, `express.e:12170`): a caller without it falls through to
        // the unknown-option arm and the loop re-renders silently — the
        // move-target prompts must never appear. Pins the match guard
        // itself; the smoke pins the prompt-rendering gate only.
        let services = test_services();
        let mut terminal = CaptureTerminal::default();
        let mut session = menu_session_for_new_user();
        let mut next = 2;
        let mut last_displayed = Some(1);
        let keep_looping = {
            let mut flow = super::super::MenuFlow {
                terminal: &mut terminal,
                services: &services,
            };
            flow.dispatch_read_option(&mut session, "m", 1, &mut next, &mut last_displayed)
                .await
                .expect("dispatch")
        };
        assert!(keep_looping, "an ignored option must keep the loop alive");
        assert!(
            terminal.output.is_empty(),
            "`m` must be inert without move access, got {:?}",
            String::from_utf8_lossy(&terminal.output)
        );
    }
}
