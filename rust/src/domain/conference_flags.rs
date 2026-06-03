//! `CF` (conference flags) edit semantics (spec:
//! `conferences.allium:EditConferenceScanFlags`).
//!
//! Pure parsing and mutation: the terminal loop (app layer) reads the
//! mask key and the expression line, then calls
//! [`parse_scan_flag_selection`] and [`apply_scan_flag_edit`] here.
//! Mirrors `internalCommandCF`'s expression handling
//! (`amiexpress/express.e:24769-24838`).

use crate::domain::conference::{ConferenceMembership, ScanFlag};

/// A parsed `CF` edit expression. The legacy prompt is
/// `Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on`
/// (`amiexpress/express.e:24769`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanFlagSelection {
    /// `+` — set the flag on every accessible conference
    /// (`express.e:24775`).
    AllOn,
    /// `-` — clear the flag on every accessible conference
    /// (`express.e:24784`).
    AllOff,
    /// `*` — toggle the flag on every accessible conference.
    ///
    /// Design D1: the legacy advertises `*` in the prompt but its code
    /// has no `*` branch (validated live against the FS-UAE reference),
    /// so the legacy `*` is a no-op. NextExpress honours the advertised
    /// toggle-all.
    ToggleAll,
    /// A comma-separated list of conference numbers — toggle each named
    /// conference (`express.e:24793-24837`, which EORs the mask per
    /// match, so a number listed twice cancels).
    Toggle(Vec<u32>),
}

/// Parses a raw `CF` expression line into a [`ScanFlagSelection`], or
/// `None` when the line is empty — the legacy returns to the menu on an
/// empty expression (`express.e:24773`).
///
/// Tokens in the conference-number list that do not parse as a number are
/// dropped: the legacy simply matches no conference for them.
#[must_use]
pub fn parse_scan_flag_selection(input: &str) -> Option<ScanFlagSelection> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed {
        "+" => ScanFlagSelection::AllOn,
        "-" => ScanFlagSelection::AllOff,
        "*" => ScanFlagSelection::ToggleAll,
        _ => ScanFlagSelection::Toggle(
            trimmed
                .split(',')
                .filter_map(|tok| tok.trim().parse::<u32>().ok())
                .collect(),
        ),
    })
}

/// Applies a parsed `CF` edit to the caller's memberships
/// (`conferences.allium:EditConferenceScanFlags`).
///
/// Only **granted** memberships are touched — the legacy gates every edit
/// on `checkConfAccess` (`express.e:24777`/`:24786`/`:24801`).
/// `+`/`-`/`*` act on every granted membership; a conference list toggles
/// each named, granted conference once per occurrence (so a number listed
/// twice cancels, mirroring the legacy EOR).
pub fn apply_scan_flag_edit(
    memberships: &mut [ConferenceMembership],
    flag: ScanFlag,
    selection: &ScanFlagSelection,
) {
    match selection {
        ScanFlagSelection::AllOn => {
            for m in memberships.iter_mut().filter(|m| m.is_granted()) {
                m.set_scan_flag(flag, true);
            }
        }
        ScanFlagSelection::AllOff => {
            for m in memberships.iter_mut().filter(|m| m.is_granted()) {
                m.set_scan_flag(flag, false);
            }
        }
        ScanFlagSelection::ToggleAll => {
            for m in memberships.iter_mut().filter(|m| m.is_granted()) {
                m.toggle_scan_flag(flag);
            }
        }
        ScanFlagSelection::Toggle(numbers) => {
            for &number in numbers {
                if let Some(m) = memberships
                    .iter_mut()
                    .find(|m| m.is_granted() && m.conference_number() == number)
                {
                    m.toggle_scan_flag(flag);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::conference::{ConferenceMembership, ScanFlag};

    fn memberships() -> Vec<ConferenceMembership> {
        // Conferences 1, 2, 3 granted; conference 4 revoked. All scan
        // flags start at their defaults (mail/file on, all/zoom off).
        vec![
            ConferenceMembership::new(1, true),
            ConferenceMembership::new(2, true),
            ConferenceMembership::new(3, true),
            ConferenceMembership::new(4, false),
        ]
    }

    fn flag_of(ms: &[ConferenceMembership], conf: u32, flag: ScanFlag) -> bool {
        ms.iter()
            .find(|m| m.conference_number() == conf)
            .expect("membership present")
            .scan_flag(flag)
    }

    #[test]
    fn empty_expression_parses_to_none_so_the_command_exits() {
        assert_eq!(parse_scan_flag_selection(""), None);
        assert_eq!(parse_scan_flag_selection("   "), None);
    }

    #[test]
    fn plus_minus_star_parse_to_their_variants() {
        assert_eq!(
            parse_scan_flag_selection("+"),
            Some(ScanFlagSelection::AllOn)
        );
        assert_eq!(
            parse_scan_flag_selection("-"),
            Some(ScanFlagSelection::AllOff)
        );
        assert_eq!(
            parse_scan_flag_selection("*"),
            Some(ScanFlagSelection::ToggleAll)
        );
    }

    #[test]
    fn conference_list_parses_numbers_and_drops_garbage_tokens() {
        assert_eq!(
            parse_scan_flag_selection("1, 3 , x, 2"),
            Some(ScanFlagSelection::Toggle(vec![1, 3, 2]))
        );
    }

    #[test]
    fn plus_sets_the_flag_on_every_granted_conference_only() {
        let mut ms = memberships();
        apply_scan_flag_edit(&mut ms, ScanFlag::MailScanAll, &ScanFlagSelection::AllOn);
        assert!(flag_of(&ms, 1, ScanFlag::MailScanAll));
        assert!(flag_of(&ms, 2, ScanFlag::MailScanAll));
        assert!(flag_of(&ms, 3, ScanFlag::MailScanAll));
        assert!(
            !flag_of(&ms, 4, ScanFlag::MailScanAll),
            "revoked conference is not accessible and must be left untouched"
        );
    }

    #[test]
    fn minus_clears_the_flag_on_every_granted_conference() {
        let mut ms = memberships();
        apply_scan_flag_edit(&mut ms, ScanFlag::MailScan, &ScanFlagSelection::AllOff);
        assert!(!flag_of(&ms, 1, ScanFlag::MailScan));
        assert!(!flag_of(&ms, 2, ScanFlag::MailScan));
        assert!(!flag_of(&ms, 3, ScanFlag::MailScan));
    }

    #[test]
    fn star_toggles_every_granted_conference() {
        // Design D1: '*' is the advertised toggle-all the legacy no-ops.
        let mut ms = memberships();
        apply_scan_flag_edit(&mut ms, ScanFlag::MailScan, &ScanFlagSelection::ToggleAll);
        assert!(
            !flag_of(&ms, 1, ScanFlag::MailScan),
            "default on -> toggled off"
        );
        assert!(!flag_of(&ms, 2, ScanFlag::MailScan));
        assert!(!flag_of(&ms, 3, ScanFlag::MailScan));
        apply_scan_flag_edit(&mut ms, ScanFlag::MailScan, &ScanFlagSelection::ToggleAll);
        assert!(flag_of(&ms, 1, ScanFlag::MailScan), "toggled back on");
    }

    #[test]
    fn conference_list_toggles_only_the_named_conferences() {
        let mut ms = memberships();
        apply_scan_flag_edit(
            &mut ms,
            ScanFlag::MailScan,
            &ScanFlagSelection::Toggle(vec![1, 3]),
        );
        assert!(!flag_of(&ms, 1, ScanFlag::MailScan));
        assert!(
            flag_of(&ms, 2, ScanFlag::MailScan),
            "conference 2 was not named and stays at its default"
        );
        assert!(!flag_of(&ms, 3, ScanFlag::MailScan));
    }

    #[test]
    fn a_conference_listed_twice_cancels_like_the_legacy_eor() {
        let mut ms = memberships();
        apply_scan_flag_edit(
            &mut ms,
            ScanFlag::MailScan,
            &ScanFlagSelection::Toggle(vec![1, 1]),
        );
        assert!(
            flag_of(&ms, 1, ScanFlag::MailScan),
            "toggled twice returns to the original value"
        );
    }

    #[test]
    fn a_list_targeting_a_revoked_conference_is_a_noop() {
        let mut ms = memberships();
        apply_scan_flag_edit(
            &mut ms,
            ScanFlag::MailScan,
            &ScanFlagSelection::Toggle(vec![4]),
        );
        assert!(
            flag_of(&ms, 4, ScanFlag::MailScan),
            "revoked conference is not toggled (stays at its untouched default)"
        );
    }
}
