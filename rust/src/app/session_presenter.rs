//! Rendering helpers shared by the workflow sub-flows.
//!
//! The login, registration and menu flows each own their own state
//! machine, but several wire-format decisions show up in more than
//! one place — the conference-join announcement is produced by the
//! auto-rejoin path (after sign-in) and the explicit-join path (from
//! the menu); the name-type promotion screen is shown after both.
//! Keeping the rendering in one module means changes to the wire
//! shape land in a single file and the sub-flows stay focused on
//! their own state transitions.

use crate::app::screens::ScreenRepository;
use crate::app::terminal::Terminal;
use crate::app::wire_text;
use crate::domain::conference::{Conference, MessageBase, NameType};

/// Resolves `(conference_name, msgbase_name)` for the wire-format
/// helpers. The `msgbase_name` is `Some(_)` only when the
/// conference holds more than one message base, mirroring the
/// `getConfMsgBaseCount(conf)>1` branch in legacy `joinConf`.
#[must_use]
pub(crate) fn resolve_conference_strings(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> (&str, Option<&str>) {
    let Some(conference) = conferences.iter().find(|c| c.number() == conference_number) else {
        return ("?", None);
    };
    let msgbase_name = if conference.msgbases().len() > 1 {
        conference
            .msgbases()
            .iter()
            .find(|m| m.number() == msgbase_number)
            .map(MessageBase::name)
    } else {
        None
    };
    (conference.name(), msgbase_name)
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline auto-rejoin announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5071-5073`). Returns just the
/// conference-name segment when the lookup fails, which is
/// defensive — `auto_rejoin_conference` only reports
/// `conference_number`s that came from the catalogue.
#[must_use]
pub(crate) fn format_auto_rejoin_line(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    wire_text::auto_rejoin_line(conference_number, conference_name, msgbase_name)
}

/// Looks up `conference_number` in `conferences` and renders the
/// inline explicit-join announcement matching the legacy `joinConf`
/// output (`amiexpress/express.e:5079-5083`).
#[must_use]
pub(crate) fn format_explicit_join_line(
    conferences: &[Conference],
    conference_number: u32,
    msgbase_number: u32,
) -> Vec<u8> {
    let (conference_name, msgbase_name) =
        resolve_conference_strings(conferences, conference_number, msgbase_number);
    wire_text::explicit_join_line(conference_name, msgbase_name)
}

/// Renders `SCREEN_REALNAMES` / `SCREEN_INTERNETNAMES` when a join
/// promoted the session's `display_name_type` (Slice 34).
pub(crate) async fn render_name_type_promotion<T, S>(
    terminal: &mut T,
    screens: &S,
    promoted: Option<NameType>,
) -> Result<(), T::Error>
where
    T: Terminal + ?Sized,
    S: ScreenRepository + ?Sized,
{
    let bytes = match promoted {
        Some(NameType::RealName) => screens.realnames_screen().await,
        Some(NameType::InternetName) => screens.internetnames_screen().await,
        Some(NameType::Handle) | None => return Ok(()),
    };
    terminal.write(&bytes).await?;
    terminal.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::conference::{Conference, MessageBase};

    #[test]
    fn resolve_conference_strings_returns_name_only_for_single_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = false` branch in
        // legacy `joinConf` (`amiexpress/express.e:5072`): the
        // announcement omits the `[<msgbase>]` segment.
        let confs = vec![Conference::new(
            7,
            "Solo".to_string(),
            vec![MessageBase::new(7, 1, "main".to_string())],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 7, 1);
        assert_eq!(name, "Solo");
        assert!(
            mb.is_none(),
            "single-msgbase conferences should not include a msgbase name"
        );
    }

    #[test]
    fn resolve_conference_strings_emits_msgbase_for_multi_msgbase_conferences() {
        // Mirrors `getConfMsgBaseCount(conf)>1 = true` branch in
        // legacy `joinConf` (`amiexpress/express.e:5070`): the
        // announcement carries `[<msgbase>]`.
        let confs = vec![Conference::new(
            3,
            "Tech-and-misc".to_string(),
            vec![
                MessageBase::new(3, 1, "main".to_string()),
                MessageBase::new(3, 2, "tech".to_string()),
            ],
        )
        .expect("valid")];
        let (name, mb) = resolve_conference_strings(&confs, 3, 2);
        assert_eq!(name, "Tech-and-misc");
        assert_eq!(mb, Some("tech"));
    }

    #[test]
    fn resolve_conference_strings_returns_question_mark_for_unknown_conference() {
        // Defensive fallback: a conference number that's not in the
        // catalogue produces "?". Today this is unreachable (the
        // resolver only reports numbers that came from the
        // catalogue) but the helper has to be total.
        let (name, mb) = resolve_conference_strings(&[], 99, 1);
        assert_eq!(name, "?");
        assert!(mb.is_none());
    }
}
