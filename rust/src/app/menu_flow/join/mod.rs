//! `J` (Join Conference) menu command (Slice 32 / Tier C C2) and the
//! `<` / `>` previous/next-accessible-conference commands (Tier C C3).
//!
//! Mirrors the legacy `internalCommandJ`
//! (`amiexpress/express.e:25113-25183`): a direct in-range argument
//! joins immediately; everything else opens the single-shot
//! `Conference Number (1-N): ` prompt. A granted join writes the
//! `Joining Conference: <name>` announcement, any name-type promotion
//! screen (Slice 34) and Slice 41's `ScanMailOnJoin`; a denied
//! request writes the legacy no-access notice and stays in the
//! current conference.
//!
//! `<` / `>` (`internalCommandLT`/`GT`,
//! `amiexpress/express.e:24529-24564`) walk to the nearest accessible
//! conference below/above the current one and join it through the
//! same machinery as a direct `J <n>`; with no accessible neighbour
//! in that direction they fall into the interactive prompt — no
//! wraparound.
//!
//! `JM` (`internalCommandJM`, `amiexpress/express.e:25185-25237`,
//! Tier C C4a) joins a message base of the current conference, and
//! the dotted / two-token `J` forms (`J 1.2` / `J 1 2`,
//! `:25130-25136`) target a base of another conference — both run the
//! same full join sequence, whose announcement appends ` [<base>]`
//! on multi-base conferences (`:5077-5084`).

use std::time::SystemTime;

use crate::app::menu_command::{val_prefix, JoinArg, MsgBaseArg};
use crate::app::session_presenter::{format_explicit_join_line, render_name_type_promotion};
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::{
    render_conference_number_prompt, render_msgbase_number_prompt, render_scan_summary,
    MAIL_STORE_ERROR_LINE, NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE, SINGLE_MSGBASE_CONFERENCE_LINE,
};
use crate::domain::conference::{find_msgbase_in, Conference, MessageBase};
use crate::domain::conference_visit::{
    next_accessible_conference_after, prev_accessible_conference_before, resolve_explicit_join,
    ExplicitJoinResolution,
};
use crate::domain::messaging::scan_mail::scan_mail;
use crate::domain::session::typed::{ExplicitJoinTransition, MenuSession};

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the `J` menu command (`internalCommandJ`,
    /// `amiexpress/express.e:25113-25183`).
    ///
    /// A direct conference argument already inside `1..=numConf`
    /// joins immediately (`amiexpress/express.e:25142` gate).
    /// Everything else — bare `J`, a `Val` of zero/negative, an
    /// out-of-range number (direct arguments are *not* clamped;
    /// clamping is prompt-input-only, `:25153-25154`) — opens the
    /// interactive conference prompt.
    ///
    /// The dotted / two-token forms (Tier C C4a,
    /// `amiexpress/express.e:25130-25136`) carry a message-base
    /// request, which survives the conference prompt exactly as the
    /// legacy `newMsgBase` local does. The access check on the
    /// resolved conference precedes the base range check (`:25156`
    /// vs `:25168`); a granted conference with an out-of-range base
    /// is deferred to C4b (the legacy `Message Base Number (1-N): `
    /// prompt, `:25169-25180`) — interim it returns to the menu
    /// silently.
    ///
    /// Returns the session in both outcomes: explicit join never
    /// logs the caller off (they already hold a conference).
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_join_command(
        &mut self,
        mut session: MenuSession,
        arg: JoinArg,
    ) -> Result<MenuSession, T::Error> {
        let conferences = self.services.conferences.as_ref();
        let highest = highest_conference_number(conferences);
        let (requested_conference, requested_msgbase) = match arg {
            JoinArg::Conference(n) => (Some(n), None),
            JoinArg::WithMsgBase {
                conference,
                msgbase,
            } => (Some(conference), Some(msgbase)),
            JoinArg::Missing => (None, None),
        };
        let direct_target = requested_conference
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| (1..=highest).contains(&n));
        let target = match direct_target {
            Some(n) => n,
            None => match self
                .prompt_for_conference_number(&mut session, highest)
                .await?
            {
                Some(n) => n,
                None => return Ok(session),
            },
        };
        let Some(msgbase) = requested_msgbase else {
            return self.handle_explicit_join(session, target, None).await;
        };
        // Message-base forms: the legacy access-checks the resolved
        // conference (`amiexpress/express.e:25156`) before
        // range-checking the base (`:25168`), so a denied conference
        // always wins over an out-of-range base.
        let base_info = match resolve_explicit_join(target, None, session.user(), conferences) {
            ExplicitJoinResolution::Denied => None,
            ExplicitJoinResolution::Granted { conference, .. } => Some((
                u32::try_from(msgbase)
                    .ok()
                    .filter(|&n| conference.find_msgbase(n).is_some()),
                msgbase_count_of(conference),
            )),
        };
        let Some((granted_base, base_count)) = base_info else {
            self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                .await?;
            return Ok(session);
        };
        match granted_base {
            Some(base) => self.handle_explicit_join(session, target, Some(base)).await,
            None => {
                // An explicit out-of-range message base opens J's
                // `Message Base Number (1-N): ` prompt
                // (`amiexpress/express.e:25169-25180`) — observed live
                // even on a single-base conference (`J 1 2` yields the
                // `(1-1)` prompt; the single-base notice is JM-only).
                // N is the TARGET conference's base count (`:25167`).
                match self
                    .prompt_for_msgbase_number(&mut session, base_count)
                    .await?
                {
                    None => Ok(session),
                    Some(value) => {
                        // J's prompt answer is NOT clamped
                        // (`amiexpress/express.e:25179`): `joinConf`
                        // resets a base the conference does not hold
                        // to the primary (`:4995`); a negative `Val`
                        // collapses to the same reset path.
                        let base = u32::try_from(value).unwrap_or(0);
                        self.handle_explicit_join(session, target, Some(base)).await
                    }
                }
            }
        }
    }

    /// Drives the `JM` menu command (`internalCommandJM`,
    /// `amiexpress/express.e:25185-25237`): joins a message base of
    /// the *current* conference. The dotted form never reaches here —
    /// the parser routes it to [`Self::handle_join_command`], exactly
    /// as the legacy delegates the raw params to `internalCommandJ`
    /// (`:25203-25205`).
    ///
    /// On a conference holding a single message base every form fails
    /// with the legacy notice before any range logic
    /// (`amiexpress/express.e:25211-25215`: the `NMSGBASES` tooltype
    /// probe returns -1 when absent — the normal single-base
    /// configuration). `NextExpress` equates "tooltype absent" with
    /// `bases.len() == 1`; the legacy nuance of an explicitly-set
    /// `NMSGBASES=1` (which prompts `(1-1)` instead) is deliberately
    /// not modelled — recorded in `slices/cmds-conf-nav.md`.
    ///
    /// An in-range argument runs the full join sequence — there is no
    /// "already there" check, so `JM <current>` re-joins in full. A
    /// missing or out-of-range argument opens the `JoinMsgBase`
    /// screen and the single-shot `Message Base Number (1-N): `
    /// prompt (`amiexpress/express.e:25220-25230`), whose answer is
    /// clamped into `1..=N` (`:25233-25234`) — unlike `J`'s
    /// message-base prompt, which passes its answer to the join
    /// unclamped.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_join_msgbase_command(
        &mut self,
        mut session: MenuSession,
        arg: MsgBaseArg,
    ) -> Result<MenuSession, T::Error> {
        let conferences = self.services.conferences.as_ref();
        // Defensive: a menu session without an open visit (or one
        // pointing outside the catalogue) has no current conference
        // to count bases on; stay at the menu silently.
        let Some(conference) = session
            .current_conference_number()
            .and_then(|n| conferences.iter().find(|c| c.number() == n))
        else {
            return Ok(session);
        };
        if conference.msgbases().len() == 1 {
            self.write_and_flush(SINGLE_MSGBASE_CONFERENCE_LINE).await?;
            return Ok(session);
        }
        let current = conference.number();
        let base_count = msgbase_count_of(conference);
        let granted_base = match arg {
            MsgBaseArg::Base(n) => u32::try_from(n)
                .ok()
                .filter(|&n| conference.find_msgbase(n).is_some()),
            MsgBaseArg::Missing => None,
        };
        match granted_base {
            Some(base) => {
                self.handle_explicit_join(session, current, Some(base))
                    .await
            }
            None => {
                // Missing / out-of-range arguments open the JoinMsgBase
                // screen and the `Message Base Number (1-N): ` prompt
                // (`amiexpress/express.e:25220-25230`).
                match self
                    .prompt_for_msgbase_number(&mut session, base_count)
                    .await?
                {
                    None => Ok(session),
                    Some(value) => {
                        // JM's prompt answer IS clamped into `1..=N`
                        // (`amiexpress/express.e:25233-25234`) — the
                        // documented asymmetry with `J`'s prompt.
                        let base = value.clamp(1, i64::from(base_count));
                        let base = u32::try_from(base).expect("clamped into 1..=base_count");
                        self.handle_explicit_join(session, current, Some(base))
                            .await
                    }
                }
            }
        }
    }

    /// Drives the `<` menu command (`internalCommandLT`,
    /// `amiexpress/express.e:24529-24546`): joins the nearest
    /// lower-numbered conference the caller holds a grant for, at its
    /// primary message base, through the same machinery as a direct
    /// `J <n>` (legacy `joinConf(newConf,1,FALSE,FALSE)`, `:24543`) —
    /// the join output is byte-identical. Inaccessible conferences
    /// are skipped silently (`:24536-24538`). With no accessible
    /// conference below (or, defensively, no open visit) the legacy
    /// runs `internalCommandJ('')` (`:24541`) — the interactive
    /// conference-number prompt; there is no wraparound.
    ///
    /// The legacy `ACS_JOIN_CONFERENCE` gate (`:24531`) is
    /// deliberately not ported: the port has no join right yet and
    /// `J` does not gate today, so `<` / `>` stay consistent with it.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_prev_conference(
        &mut self,
        session: MenuSession,
    ) -> Result<MenuSession, T::Error> {
        let target = session.current_conference_number().and_then(|current| {
            prev_accessible_conference_before(
                session.user(),
                self.services.conferences.as_ref(),
                current,
            )
            .map(Conference::number)
        });
        match target {
            Some(number) => self.handle_explicit_join(session, number, None).await,
            None => self.handle_join_command(session, JoinArg::Missing).await,
        }
    }

    /// Drives the `>` menu command (`internalCommandGT`,
    /// `amiexpress/express.e:24548-24564`): the upward mirror of
    /// [`Self::handle_prev_conference`] — nearest higher-numbered
    /// granted conference at its primary message base
    /// (`joinConf(newConf,1,FALSE,FALSE)`, `:24562`), skipping
    /// inaccessible conferences silently, falling into the
    /// interactive prompt past the top (`:24559-24560`). No
    /// wraparound, and the same deliberate `ACS_JOIN_CONFERENCE`
    /// omission.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_next_conference(
        &mut self,
        session: MenuSession,
    ) -> Result<MenuSession, T::Error> {
        let target = session.current_conference_number().and_then(|current| {
            next_accessible_conference_after(
                session.user(),
                self.services.conferences.as_ref(),
                current,
            )
            .map(Conference::number)
        });
        match target {
            Some(number) => self.handle_explicit_join(session, number, None).await,
            None => self.handle_join_command(session, JoinArg::Missing).await,
        }
    }

    /// Drives the `<<` menu command (`internalCommandLT2`,
    /// `amiexpress/express.e:24566-24578`): steps to the previous
    /// message base of the current conference. In bounds the step is
    /// a full message-base join
    /// (`joinConf(currentConf,newMsgBase,FALSE,FALSE)`, `:24576`);
    /// past the bottom (`newMsgBase<1`, `:24573`) it falls into the
    /// `JM` no-arg flow (`:24574`) — the single-base notice or the
    /// interactive prompt. Unlike `<` / `>` the legacy runs NO
    /// security check on the direct path, and there is no
    /// per-message-base access concept. A session without an open
    /// visit defensively takes the `JM` no-arg flow too.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_prev_msgbase(
        &mut self,
        session: MenuSession,
    ) -> Result<MenuSession, T::Error> {
        self.step_msgbase(session, -1).await
    }

    /// Drives the `>>` menu command (`internalCommandGT2`,
    /// `amiexpress/express.e:24580-24592`): the upward mirror of
    /// [`Self::handle_prev_msgbase`] — past the top
    /// (`newMsgBase>getConfMsgBaseCount(currentConf)`, `:24587`) it
    /// falls into the `JM` no-arg flow (`:24588`). No wraparound.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_next_msgbase(
        &mut self,
        session: MenuSession,
    ) -> Result<MenuSession, T::Error> {
        self.step_msgbase(session, 1).await
    }

    /// Shared `<<` / `>>` walk: `currentMsgBase ± 1`, joined in full
    /// when the current conference holds that base, otherwise the
    /// `JM` no-arg flow (`amiexpress/express.e:24571-24577` /
    /// `:24585-24591` — `LT2` only checks the lower bound and `GT2`
    /// only the upper, but `currentMsgBase` is always in range so a
    /// full range check is equivalent).
    async fn step_msgbase(
        &mut self,
        session: MenuSession,
        delta: i64,
    ) -> Result<MenuSession, T::Error> {
        let conferences = self.services.conferences.as_ref();
        let target = session.current_msgbase().and_then(|(conf, base)| {
            let stepped = i64::from(base) + delta;
            let count = conferences
                .iter()
                .find(|c| c.number() == conf)
                .map(msgbase_count_of)?;
            if (1..=i64::from(count)).contains(&stepped) {
                Some((conf, u32::try_from(stepped).expect("in 1..=count")))
            } else {
                None
            }
        });
        match target {
            Some((conf, base)) => self.handle_explicit_join(session, conf, Some(base)).await,
            None => {
                self.handle_join_msgbase_command(session, MsgBaseArg::Missing)
                    .await
            }
        }
    }

    /// Runs the single-shot interactive message-base prompt
    /// (`amiexpress/express.e:25169-25176` in `internalCommandJ`,
    /// `:25220-25230` in `internalCommandJM`): the optional
    /// `JoinMsgBase` screen — resolved for the conference the caller
    /// is *currently* in, because the legacy `confScreenDir` is
    /// repointed only inside `joinConf` (`:5052`) — then the
    /// `Message Base Number (1-N): ` prompt and exactly one line of
    /// input; the legacy never re-prompts.
    ///
    /// Returns `Some(Val)` of a non-empty line, UNCLAMPED — the two
    /// callers disagree about clamping (`internalCommandJM` clamps
    /// into `1..=N`, `:25233-25234`; `internalCommandJ` passes the
    /// raw value to `joinConf`, which resets a base the conference
    /// does not hold to the primary, `:25179` + `:4995`), so the
    /// decision stays with them. Returns `None` when the caller
    /// aborted: a blank line (exact emptiness, no trimming) writes
    /// the lone CRLF the legacy `lineInput` emits (`:2378`); Eof /
    /// idle timeout return silently and the menu loop's next read
    /// applies the carrier-loss / idle transitions.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    async fn prompt_for_msgbase_number(
        &mut self,
        session: &mut MenuSession,
        msgbase_count: u32,
    ) -> Result<Option<i64>, T::Error> {
        let screen = match session.current_conference_number() {
            Some(current) => {
                self.services
                    .screens
                    .as_ref()
                    .joinmsgbase_screen(current)
                    .await
            }
            None => Vec::new(),
        };
        if !screen.is_empty() {
            self.terminal.write(&screen).await?;
        }
        let prompt = render_msgbase_number_prompt(msgbase_count);
        let TerminalRead::Line(line) = self.read_prompted(&prompt, TerminalEcho::Visible).await?
        else {
            return Ok(None);
        };
        session.record_input(SystemTime::now());
        if line.is_empty() {
            // Blank aborts silently (`amiexpress/express.e:25228`);
            // the only wire output is `lineInput`'s trailing `\b\n`
            // (`:2378`) — one CRLF after the echoed empty line.
            self.write_newline().await?;
            return Ok(None);
        }
        Ok(Some(val_prefix(&line)))
    }

    /// Runs the single-shot interactive conference prompt
    /// (`amiexpress/express.e:25143-25154`): the optional `JoinConf`
    /// screen, the `Conference Number (1-N): ` prompt, then exactly
    /// one line of input — the legacy never re-prompts.
    ///
    /// Returns `Some(number)` (the `Val` of the line clamped into
    /// `1..=N`, `:25153-25154`) for a non-empty line. Returns `None`
    /// when the caller aborted: a blank line (exact emptiness, no
    /// trimming — a whitespace-only line `Val`s to 0 and clamps to
    /// conference 1 instead) writes the lone CRLF the legacy
    /// `lineInput` always emits (`:2378`); Eof / idle timeout return
    /// silently and the menu loop's next read applies the
    /// carrier-loss / idle transitions.
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    async fn prompt_for_conference_number(
        &mut self,
        session: &mut MenuSession,
        highest_conference_number: u32,
    ) -> Result<Option<u32>, T::Error> {
        // SCREEN_JOINCONF (`amiexpress/express.e:25143`): renders
        // only when the sysop installed `Screens/JoinConf.txt` — the
        // adapter returns empty bytes when the asset is absent,
        // matching the reference where nothing precedes the prompt.
        let screen = self.services.screens.as_ref().joinconf_screen().await;
        if !screen.is_empty() {
            self.terminal.write(&screen).await?;
        }
        let prompt = render_conference_number_prompt(highest_conference_number);
        let TerminalRead::Line(line) = self.read_prompted(&prompt, TerminalEcho::Visible).await?
        else {
            return Ok(None);
        };
        session.record_input(SystemTime::now());
        if line.is_empty() {
            // Blank aborts silently (`amiexpress/express.e:25148`);
            // the only wire output is `lineInput`'s trailing `\b\n`
            // (`:2378`) — one CRLF after the echoed empty line.
            self.write_newline().await?;
            return Ok(None);
        }
        let mut value = val_prefix(&line);
        // Legacy clamp order (`amiexpress/express.e:25153-25154`).
        // On an empty catalogue (highest = 0, unreachable from a
        // joined menu session) this yields 0, which resolution
        // denies.
        if value < 1 {
            value = 1;
        }
        if value > i64::from(highest_conference_number) {
            value = i64::from(highest_conference_number);
        }
        Ok(Some(
            u32::try_from(value).expect("clamped into 0..=u32::MAX above"),
        ))
    }

    /// Joins `target_conference_number` exactly (legacy `joinConf`
    /// via `internalCommandJ`, `amiexpress/express.e:25156-25182`):
    /// the granted path renders the join announcement, any name-type
    /// promotion screen and the scan-on-join; a denied request
    /// writes the legacy no-access notice
    /// (`amiexpress/express.e:25157`) and leaves the session in its
    /// current conference.
    ///
    /// `requested_msgbase_number` targets a specific message base
    /// (Tier C C4a; `None` = the conference's primary base). Callers
    /// range-check the base first — a base the conference does not
    /// hold defensively resets to the primary base in the domain
    /// (legacy `joinConf`, `amiexpress/express.e:4995`).
    ///
    /// # Errors
    /// Propagates terminal I/O errors.
    pub(super) async fn handle_explicit_join(
        &mut self,
        session: MenuSession,
        target_conference_number: u32,
        requested_msgbase_number: Option<u32>,
    ) -> Result<MenuSession, T::Error> {
        let conferences = self.services.conferences.as_ref();
        match session.explicit_join_conference(
            target_conference_number,
            requested_msgbase_number,
            conferences,
            SystemTime::now(),
        ) {
            ExplicitJoinTransition::Joined {
                mut session,
                conference_number,
                msgbase_number,
                name_type_promoted_to,
                ..
            } => {
                // Compute the announcement bytes up-front so the
                // immutable borrow on `self.services.conferences.as_ref()`
                // doesn't overlap the mutable borrows below.
                let line =
                    format_explicit_join_line(conferences, conference_number, msgbase_number);
                self.write_and_flush(&line).await?;
                render_name_type_promotion(
                    self.terminal,
                    self.services.screens.as_ref(),
                    name_type_promoted_to,
                )
                .await?;
                self.scan_mail_on_join(&mut session).await?;
                Ok(session)
            }
            ExplicitJoinTransition::Denied(session) => {
                self.write_and_flush(NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE)
                    .await?;
                Ok(session)
            }
        }
    }

    /// Fires `conferences.allium:ScanMailOnJoin` against the new visit
    /// (Slice 41): locks the visit's mail store, runs
    /// `messaging.allium:ScanMail` from the caller's read pointer
    /// (`from_message = 0` is the rule's "`last_scanned + 1`" sentinel —
    /// the legacy `forceMailScan = NOFORCE` path), renders the
    /// `SCREEN_MAILSCAN` asset when the scan surfaced unread mail, then
    /// the textual summary line. A missing visit or unregistered store
    /// is silent; a store error is logged to stderr and degraded to the
    /// generic mail-store-error notice — the session continues either
    /// way.
    async fn scan_mail_on_join(&mut self, session: &mut MenuSession) -> Result<(), T::Error> {
        let Some((visit_msgbase, guard)) =
            super::lock_current_base(session, self.services.mail_stores.as_ref()).await
        else {
            return Ok(());
        };
        let scope = find_msgbase_in(self.services.conferences.as_ref(), visit_msgbase)
            .map(MessageBase::all_scan_scope)
            .unwrap_or_default();
        let result = scan_mail(
            session.user_mut(),
            &*guard,
            visit_msgbase,
            scope,
            0,
            SystemTime::now(),
        );
        drop(guard);
        match result {
            Ok(result) => {
                if result.unread_count > 0 {
                    let screen = self.services.screens.as_ref().mailscan_screen().await;
                    self.terminal.write(&screen).await?;
                }
                let summary = render_scan_summary(result.unread_count, result.first_unread_number);
                self.write_and_flush(&summary).await
            }
            Err(err) => {
                eprintln!("scan_mail_on_join failed: {err}");
                self.write_and_flush(MAIL_STORE_ERROR_LINE).await
            }
        }
    }
}

/// The highest conference number in the catalogue — the legacy
/// `cmds.numConf` bound the prompt advertises and clamps into
/// (`amiexpress/express.e:25144`, `:25154`). The catalogue is in
/// ascending number order per the `ConferenceRepository::load_all`
/// contract, so the last entry carries the highest number. Zero on
/// an empty catalogue, where every join request is denied.
fn highest_conference_number(conferences: &[Conference]) -> u32 {
    conferences.last().map_or(0, Conference::number)
}

/// The conference's message-base count — the legacy
/// `getConfMsgBaseCount` (`amiexpress/express.e:2048-2052`), which
/// the `Message Base Number (1-N): ` prompt advertises (`:25167`,
/// `:25218`) and `JM`'s clamp bounds (`:25233-25234`).
/// `Conference::new` enforces a non-empty base list, so the count is
/// at least 1.
fn msgbase_count_of(conference: &Conference) -> u32 {
    u32::try_from(conference.msgbases().len()).expect("message-base count fits in u32")
}

#[cfg(test)]
mod tests;
