//! Legacy DIR row rendering — layer 1 of the `NextScan` listing.
//!
//! Reproduces, at runtime from [`File`] fields, the row format the
//! legacy upload writer authored into `DIR<n>` files
//! (`amiexpress/express.e:19450-19509`; size via
//! `formatFileSizeForDirList` `:18918-18942`; date `formatLongDate`,
//! `MiscFuncs.e:278`, `FORMAT_USA` → `MM-DD-YY`; offsets independently
//! confirmed by the FTP parser, `ftpd.e:1093-1132`). `NextScan` emits
//! these bytes verbatim for rows its framer cannot frame, and reads
//! the same field layout when colouring framed rows.

use crate::domain::files::file::File;

/// `MM-DD-YY` — `formatLongDate`'s `FORMAT_USA` shape
/// (`MiscFuncs.e:278-297`), rendered in UTC.
const DIR_DATE: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[month]-[day]-[year repr:last_two]");

/// Renders `file` as its legacy DIR lines: the row, then one line per
/// description continuation, each indented the upload writer's 33
/// spaces (`express.e:19499`). No line terminators — the caller owns
/// wrapping and `\r\n`.
pub(super) fn dir_row_lines(file: &File) -> Vec<Vec<u8>> {
    let mut description = file.description_lines();
    let first = description.next().unwrap_or_default();

    let mut row = Vec::new();
    // `\l\s[13]`: left-justified minimum-width 13, never truncated.
    row.extend_from_slice(file.name().as_bytes());
    while row.len() < 13 {
        row.push(b' ');
    }
    // The format's literal space at col 13, overwritten by the check
    // byte only when the name leaves the column free
    // (`express.e:19458-19470`).
    row.push(match file.check_char() {
        Some(check) if file.name().len() < 13 => check,
        _ => b' ',
    });
    row.extend_from_slice(size_column(file.size().count()).as_bytes());
    row.extend_from_slice(b"  ");
    row.extend_from_slice(date_column(file).as_bytes());
    row.extend_from_slice(b"  ");
    row.extend_from_slice(first.as_bytes());

    std::iter::once(row)
        .chain(description.map(|line| {
            let mut continuation = vec![b' '; 33];
            continuation.extend_from_slice(line.as_bytes());
            continuation
        }))
        .collect()
}

/// The largest size that fits the 7-column DIR field. Shared with the
/// framer's frameability rule (`wire::frameable`): a size past this
/// drifts the columns, so the row falls through plain in both places
/// — one const keeps the two rules from disagreeing.
pub(super) const MAX_ALIGNED_SIZE: u64 = 9_999_999;

/// `formatFileSizeForDirList` (`express.e:18918-18942`), untoggled
/// branch: right-justified width 7 up to [`MAX_ALIGNED_SIZE`] octets,
/// unpadded past it (the authentic column drift; `CREDITBYKB` /
/// `CONVERT_TO_MB` variants are unconfigured on the reference board
/// and unported).
fn size_column(count: u64) -> String {
    if count <= MAX_ALIGNED_SIZE {
        format!("{count:>7}")
    } else {
        count.to_string()
    }
}

fn date_column(file: &File) -> String {
    time::OffsetDateTime::from(file.uploaded_at())
        .format(DIR_DATE)
        .expect("MM-DD-YY format cannot fail for in-range timestamps")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bytes::Bytes;
    use crate::domain::files::file::{File, FileStatus};
    use std::time::SystemTime;
    use time::macros::datetime;
    use time::OffsetDateTime;

    fn file(name: &str, size: u64, check: Option<u8>, at: OffsetDateTime, desc: &str) -> File {
        File::new(
            name.to_string(),
            Bytes::new(size),
            FileStatus::Available,
            check,
            desc.to_string(),
            SystemTime::from(at),
        )
    }

    fn lines(file: &File) -> Vec<Vec<u8>> {
        dir_row_lines(file)
    }

    #[test]
    fn renders_the_canonical_passed_row() {
        // comparison/evidence-tierD/fixtures/Dir1 line 1: name padded
        // to col 13, check byte at col 13, size right-justified 7 at
        // col 14, date at col 23, description at col 33.
        let f = file(
            "ANSIPACK.LHA",
            234_567,
            Some(b'P'),
            datetime!(2026-01-15 12:00 UTC),
            "Collection of 40 ANSI screens from the\nMirage art crew, January release.\nIncludes viewer.",
        );
        assert_eq!(
            lines(&f),
            vec![
                b"ANSIPACK.LHA P 234567  01-15-26  Collection of 40 ANSI screens from the".to_vec(),
                b"                                 Mirage art crew, January release.".to_vec(),
                b"                                 Includes viewer.".to_vec(),
            ]
        );
    }

    #[test]
    fn right_justifies_short_sizes_in_seven_columns() {
        let f = file(
            "MODRIPPR.LZH",
            98_304,
            Some(b'P'),
            datetime!(2026-02-03 12:00 UTC),
            "Rip protracker mods from memory",
        );
        assert_eq!(
            lines(&f),
            vec![b"MODRIPPR.LZH P  98304  02-03-26  Rip protracker mods from memory".to_vec()]
        );
    }

    #[test]
    fn eight_digit_sizes_overflow_the_column_unpadded() {
        // formatFileSizeForDirList's `\d` branch (express.e:18936):
        // sizes past 9,999,999 emit unpadded — authentic column drift.
        let f = file(
            "MEGADEMO.DMS",
            12_345_678,
            Some(b'P'),
            datetime!(2026-05-01 12:00 UTC),
            "Spaceballs mega demo disk 1 of 2",
        );
        assert_eq!(
            lines(&f),
            vec![b"MEGADEMO.DMS P12345678  05-01-26  Spaceballs mega demo disk 1 of 2".to_vec()]
        );
    }

    #[test]
    fn thirteen_char_names_leave_no_room_for_the_check_byte() {
        // express.e:19458 only pokes the check byte when the name is
        // shorter than 13 chars; the literal format space survives.
        let f = file(
            "THIRTEENCH.LZ",
            66_666,
            None,
            datetime!(2026-05-20 12:00 UTC),
            "Exactly thirteen character filename",
        );
        assert_eq!(
            lines(&f),
            vec![b"THIRTEENCH.LZ   66666  05-20-26  Exactly thirteen character filename".to_vec()]
        );
    }

    #[test]
    fn long_names_are_never_truncated_and_shift_the_columns() {
        // Amiga `\l\s[13]` pads but never cuts; the row simply drifts.
        let f = file(
            "ALONGFILENAME.LHA",
            77_777,
            None,
            datetime!(2026-05-22 12:00 UTC),
            "Long filename breaks the columns",
        );
        assert_eq!(
            lines(&f),
            vec![b"ALONGFILENAME.LHA   77777  05-22-26  Long filename breaks the columns".to_vec()]
        );
    }

    #[test]
    fn check_byte_is_suppressed_when_a_13_char_name_fills_the_column() {
        // The poke guard is strictly `< 13` (express.e:19458): even a
        // record carrying a check byte renders the literal format
        // space when the name occupies the full field.
        let f = file(
            "THIRTEENCH.LZ",
            66_666,
            Some(b'P'),
            datetime!(2026-05-20 12:00 UTC),
            "Exactly thirteen character filename",
        );
        assert_eq!(
            lines(&f),
            vec![b"THIRTEENCH.LZ   66666  05-20-26  Exactly thirteen character filename".to_vec()]
        );
    }

    #[test]
    fn twelve_char_name_with_check_byte_uses_the_final_pad_column() {
        // Boundary partner: at 12 chars the name leaves exactly the
        // col-13 slot free and the check byte lands in it.
        let f = file(
            "TWELVECHA.LH",
            1_000,
            Some(b'P'),
            datetime!(2026-05-20 12:00 UTC),
            "Twelve character filename",
        );
        assert_eq!(
            lines(&f),
            vec![b"TWELVECHA.LH P   1000  05-20-26  Twelve character filename".to_vec()]
        );
    }

    #[test]
    fn failed_check_byte_renders_at_column_13() {
        let f = file(
            "BADUPLD.LHA",
            11_111,
            Some(b'F'),
            datetime!(2026-05-15 12:00 UTC),
            "Upload aborted at 80 percent",
        );
        assert_eq!(
            lines(&f),
            vec![b"BADUPLD.LHA  F  11111  05-15-26  Upload aborted at 80 percent".to_vec()]
        );
    }
}
