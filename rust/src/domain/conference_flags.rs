//! `CF` (conference flags) edit semantics (spec:
//! `conferences.allium:EditConferenceScanFlags`).
//!
//! Pure parsing and mutation: the terminal loop (app layer) reads the
//! mask key and the expression line, then calls
//! [`parse_scan_flag_selection`] and [`apply_scan_flag_edit`] here.
//! Mirrors `internalCommandCF`'s expression handling
//! (`amiexpress/express.e:24769-24838`).

use crate::domain::conference::{Conference, ConferenceMembership, ScanFlag};

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
    /// so the legacy `*` is a no-op. `NextExpress` honours the advertised
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

/// Maps a `CF` mask-selection keystroke to the [`ScanFlag`] it edits, or
/// `None` for any other key — the legacy exits the editor on a
/// non-M/A/F/Z key (`express.e:24751-24762`). Case-insensitive; only the
/// first non-blank character is consulted (the legacy `readChar`).
#[must_use]
pub fn parse_scan_flag_mask(input: &str) -> Option<ScanFlag> {
    match input.trim().chars().next()?.to_ascii_uppercase() {
        'M' => Some(ScanFlag::MailScan),
        'A' => Some(ScanFlag::MailScanAll),
        'F' => Some(ScanFlag::FileScan),
        'Z' => Some(ScanFlag::Zoom),
        _ => None,
    }
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

/// One render-ready row of the `CF` listing: a conference the caller can
/// access, with its four scan-flag values. Built by [`conf_flag_rows`]
/// and rendered by `wire_text::render_conf_flags_listing`.
// The four glyph cells are independent M/A/F/Z flags, not a state enum.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfFlagRow {
    /// The conference number shown in the `[ n]` column.
    pub conference_number: u32,
    /// The conference name (left-justified to 23 columns when rendered).
    pub conference_name: String,
    /// `M` column — the new-mail scan flag.
    pub mail_scan: bool,
    /// `A` column — the all-messages scan flag.
    pub mailscan_all: bool,
    /// `F` column — the new-files scan flag.
    pub file_scan: bool,
    /// `Z` column — the ZOOM/QWK gather flag.
    pub zoom_scan: bool,
}

/// Builds the `CF` listing rows: one per conference the caller has a
/// granted membership for, in ascending conference-number order, joined
/// with the conference catalogue for names (legacy
/// `FOR i:=1 TO numConf IF checkConfAccess(i)`, `express.e:24695-24696`).
/// A membership whose conference is absent from the catalogue (a stale
/// row) is skipped.
#[must_use]
pub fn conf_flag_rows(
    memberships: &[ConferenceMembership],
    conferences: &[Conference],
) -> Vec<ConfFlagRow> {
    let mut rows: Vec<ConfFlagRow> = memberships
        .iter()
        .filter(|m| m.is_granted())
        .filter_map(|m| {
            let conference = conferences
                .iter()
                .find(|c| c.number() == m.conference_number())?;
            Some(ConfFlagRow {
                conference_number: m.conference_number(),
                conference_name: conference.name().to_string(),
                mail_scan: m.scan_flag(ScanFlag::MailScan),
                mailscan_all: m.scan_flag(ScanFlag::MailScanAll),
                file_scan: m.scan_flag(ScanFlag::FileScan),
                zoom_scan: m.scan_flag(ScanFlag::Zoom),
            })
        })
        .collect();
    rows.sort_by_key(|row| row.conference_number);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::conference::{Conference, ConferenceMembership, MessageBase, ScanFlag};

    fn catalogue() -> Vec<Conference> {
        vec![
            Conference::new(
                1,
                "New Users".to_string(),
                vec![MessageBase::new(1, 1, "main".to_string())],
            )
            .expect("conference"),
            Conference::new(
                2,
                "Amiga".to_string(),
                vec![MessageBase::new(2, 1, "main".to_string())],
            )
            .expect("conference"),
            Conference::new(
                3,
                "Three".to_string(),
                vec![MessageBase::new(3, 1, "main".to_string())],
            )
            .expect("conference"),
        ]
    }

    #[test]
    fn rows_cover_only_granted_conferences_in_ascending_order() {
        let confs = catalogue();
        let ms = vec![
            ConferenceMembership::new(3, true),
            ConferenceMembership::new(1, true),
            ConferenceMembership::new(2, false),
        ];
        let rows = conf_flag_rows(&ms, &confs);
        assert_eq!(
            rows.iter().map(|r| r.conference_number).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(rows[0].conference_name, "New Users");
        assert_eq!(rows[1].conference_name, "Three");
    }

    #[test]
    fn rows_reflect_each_flag_value_and_the_joined_name() {
        let confs = catalogue();
        let mut m = ConferenceMembership::new(2, true);
        m.set_scan_flag(ScanFlag::MailScan, false);
        m.set_scan_flag(ScanFlag::Zoom, true);
        let rows = conf_flag_rows(&[m], &confs);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].conference_name, "Amiga");
        assert!(!rows[0].mail_scan);
        assert!(!rows[0].mailscan_all);
        assert!(rows[0].file_scan, "file_scan defaults on");
        assert!(rows[0].zoom_scan);
    }

    #[test]
    fn a_membership_absent_from_the_catalogue_is_skipped() {
        let confs = catalogue();
        let ms = vec![ConferenceMembership::new(9, true)];
        assert!(conf_flag_rows(&ms, &confs).is_empty());
    }

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
    fn mask_key_maps_letters_to_flags_case_insensitively() {
        assert_eq!(parse_scan_flag_mask("M"), Some(ScanFlag::MailScan));
        assert_eq!(parse_scan_flag_mask("m"), Some(ScanFlag::MailScan));
        assert_eq!(parse_scan_flag_mask("A"), Some(ScanFlag::MailScanAll));
        assert_eq!(parse_scan_flag_mask("f"), Some(ScanFlag::FileScan));
        assert_eq!(parse_scan_flag_mask("Z"), Some(ScanFlag::Zoom));
    }

    #[test]
    fn mask_key_returns_none_for_a_non_mafz_key_or_empty() {
        assert_eq!(parse_scan_flag_mask("Q"), None);
        assert_eq!(parse_scan_flag_mask(""), None);
        assert_eq!(parse_scan_flag_mask("   "), None);
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
