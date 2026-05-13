//! `E` (Enter Mail) and `C` (Comment to Sysop) menu commands
//! (Slices 42 / 43 / 44).
//!
//! Drives the minimal line-mode editor (To: / Subject: / Private (y/N) /
//! body lines terminated by `.` on its own line) and dispatches to
//! `messaging.allium:PostMail` or `messaging.allium:PostCommentToSysop`
//! via the typed [`MenuSession`]. The two handlers share the `Subject:`
//! and body editor prompts plus the wire-rendering of post outcomes —
//! co-locating them in one file keeps the editor primitives in one
//! place.

use std::time::SystemTime;

use crate::app::menu_command::PostArg;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalRead};
use crate::app::wire_text::render_post_success;
use crate::app::wire_text::{
    MAIL_STORE_ERROR_LINE, NO_MAIL_BASE_LINE, NO_SYSOP_LINE, POST_ABORTED_LINE,
    POST_ACCESS_DENIED_LINE, POST_ADDRESSING_NOT_ALLOWED_LINE, POST_BODY_PROMPT,
    POST_PRIVATE_PROMPT, POST_RECIPIENT_NO_ACCESS_LINE, POST_SUBJECT_PROMPT, POST_TO_PROMPT,
    POST_UNKNOWN_USER_LINE,
};
use crate::domain::conference::{find_msgbase_in, MessageBaseRef};
use crate::domain::messaging::mail::{BroadcastTo, Mail};
use crate::domain::messaging::post_comment_to_sysop::CommentToSysopDraft;
use crate::domain::messaging::post_mail::{PostMailDraft, PostMailError};
use crate::domain::session::typed::MenuSession;
use crate::domain::user_repository::NameLookupResult;

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
        let Some(allowed_addressing) = find_msgbase_in(self.services.conferences(), visit_msgbase)
            .map(crate::domain::conference::MessageBase::allowed_addressing)
        else {
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

    /// Handles a `C` command from the menu (Slice 44). Resolves the
    /// sysop through the user repository, walks subject/body prompts
    /// (no recipient prompt, no private toggle — the rule fixes both),
    /// and invokes `messaging.allium:PostCommentToSysop` via the typed
    /// session.
    pub(super) async fn handle_comment_to_sysop(
        &mut self,
        session: &mut MenuSession,
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

        let sysop = match self.services.user_repo().find_sysop() {
            NameLookupResult::Found(user) => *user,
            NameLookupResult::NotFound => {
                self.write_and_flush(NO_SYSOP_LINE).await?;
                return Ok(());
            }
        };

        let Some(allowed_addressing) = find_msgbase_in(self.services.conferences(), visit_msgbase)
            .map(crate::domain::conference::MessageBase::allowed_addressing)
        else {
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
            CommentToSysopDraft {
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

    /// Renders the outcome of a
    /// [`PostMail`](crate::domain::messaging::post_mail::post_mail)
    /// or `PostCommentToSysop` invocation to the terminal. Shared
    /// between the `E` and `C` handlers so a single edit moves both
    /// wire surfaces in lockstep.
    async fn render_post_result(
        &mut self,
        result: Result<Mail, PostMailError>,
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
