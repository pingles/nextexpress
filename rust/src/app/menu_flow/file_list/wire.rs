//! `NextScan` wire constants and render helpers — every byte pinned
//! against the live `AquaScan` captures, with the three `NextScan`
//! branding swaps (deliberate departure, user decision 2026-06-10:
//! plain `NextScan` centre label, dash runs stretched +25 to hold the
//! frame widths; see `designs/NEXTSCAN.md` §7).
//!
//! All output is valid UTF-8 — encoding policy in AGENTS.md; the
//! legacy single Latin-1 art/© bytes are re-encoded as their UTF-8
//! equivalents (same Unicode code points: U+00B8 ¸, U+00F8 ø,
//! U+00A4 ¤, U+00B0 °, U+00AC ¬, U+00AF ¯, U+00A9 ©), recorded
//! in `COMMAND_PARITY.md`.

use super::scan::{ListedRow, ScanLine};
use crate::domain::files::file::File;
use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};

/// The 4-column marker slot spliced between the name field and the
/// check byte on aligned rows (design 2026-06-12 §5; a deliberate
/// `NextExpress` departure recorded in `COMMAND_PARITY.md`).
const MARKER_FLAGGED: &[u8] = b"[X] ";
const MARKER_EMPTY: &[u8] = b"    ";

/// The trailing marker appended to an over-long (unframeable) row when
/// it is flagged — no column shift, the slot has nowhere to land
/// (design 2026-06-12 §5).
const MARKER_TRAILING: &[u8] = b" [X]";

/// Listing banner — `NextScan`-branded; dash run stretched 15→40 so the
/// frame keeps `AquaScan`'s 77 visible columns
/// (`ae_tierd_aquascan3.txt:163`, branding per `designs/NEXTSCAN.md` §7).
pub(super) const LISTING_BANNER: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m";

/// The `FR` (reverse) listing banner — the right label is `'fr ?'`, one
/// visible column wider than `'f ?'`, so the dash run flexes 40→39 to
/// hold the same 77-col frame (`ae_tierd_aquascan3.txt` S10/S11).
const LISTING_BANNER_REVERSE: &[u8] =
    b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]---------------------------------------[ \x1b[36m'fr ?' for options \x1b[34m]--\x1b[0m";

/// Selects the listing banner for a forward (`F`) or reverse (`FR`) scan.
pub(super) fn listing_banner(reverse: bool) -> &'static [u8] {
    if reverse {
        LISTING_BANNER_REVERSE
    } else {
        LISTING_BANNER
    }
}

/// Help banner — carries the rebranded copyright; dash run stretched
/// 9→34, 79 visible columns (`ae_tierd_aquascan3.txt:105`). `\u{a9}`
/// is the copyright sign © re-encoded as UTF-8.
pub(super) const HELP_BANNER: &str =
    "\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m";

/// The `More?` pager prompt (`ae_tierd_aquascan3.txt:158`), trailing
/// space included, no line terminator.
pub(super) const MORE_PROMPT: &[u8] =
    b"\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m ";

/// The non-stop confirmation prompt (`ae_tierd_aquascan3.txt:361`).
pub(super) const NS_CONFIRM_PROMPT: &[u8] =
    b"\x1b[36mNon-stop scrolling! Are you sure \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[36m? ";

/// `F` at `More?` — flag-by-name line prompt
/// (`ae_tierd_aquascan3.txt:212`).
pub(super) const FLAG_BY_NAME_PROMPT: &[u8] = b"\x1b[36mFile name(s) to flag:\x1b[0m ";

/// `R` at `More?` — the distinct flag-by-number prompt
/// (`ae_tierd_aquascan3.txt:252-257`).
pub(super) const FLAG_BY_NUMBER_PROMPT: &[u8] = b"\x1b[36mFile number(s) to flag:\x1b[0m ";

/// Listing footer (`ae_tierd_aquascan3.txt:157`).
pub(super) const END_OF_FILE_LIST: &[u8] = b"\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m";

/// Bad answer at the directories prompt (`ae_tierd_aquascan.txt:114`).
pub(super) const ERROR_IN_INPUT: &[u8] = b"Error in input!";

/// Unsupported argument forms at the menu
/// (`ae_tierd_aquascan4.txt` U4).
pub(super) const ARGUMENT_ERROR: &[u8] = b"Argument error! Type 'f ?' for help.";

/// Separator art line A motif (44-space indent) — the `AquaScan` wave,
/// `_¸,ø*¤°¬°¤*ø,¸_…`, re-encoded UTF-8.
const SEPARATOR_ART_A: &str =
    "_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_";

/// Separator art line B motif (6-space indent, date appended).
const SEPARATOR_ART_B: &str =
    "\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{af}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{b0}\u{a4}*\u{f8},";

const RESET: &[u8] = b"\x1b[0m";

/// The four-line separator block emitted before a framed file when a
/// new date group starts (`ae_tierd_aquascan3.txt:142-145`): blank,
/// art line A, art line B carrying the date, blank. Lines carry no
/// terminators.
pub(super) fn separator_block(date_mmddyy: &str) -> Vec<Vec<u8>> {
    let mut line_a = RESET.to_vec();
    line_a.extend_from_slice(&[b' '; 44]);
    line_a.extend_from_slice(SEPARATOR_ART_A.as_bytes());
    let mut line_b = RESET.to_vec();
    line_b.extend_from_slice(&[b' '; 6]);
    line_b.extend_from_slice(SEPARATOR_ART_B.as_bytes());
    line_b.push(b' ');
    line_b.extend_from_slice(date_mmddyy.as_bytes());
    vec![RESET.to_vec(), line_a, line_b, RESET.to_vec()]
}

/// The `[ File #N ]` header row: the pad between the label and the
/// dash run shrinks as `n` grows so the dashes stay at visible
/// column 31 and the row at 79 columns (`ae_tierd_aquascan3.txt:146`
/// vs the S7 repr's `File #10`).
pub(super) fn file_number_header(n: u32) -> Vec<u8> {
    let label_visible = format!("[ File #{n} ]").len();
    let pad = 31usize.saturating_sub(label_visible);
    let mut row = format!("\x1b[0m\x1b[34m[\x1b[0m File #{n} \x1b[34m]").into_bytes();
    row.extend(std::iter::repeat_n(b' ', pad));
    row.extend_from_slice(b"- ---- ---------------------------------- ---- -");
    row
}

/// A framed (colour-coded) DIR row (`ae_tierd_aquascan3.txt:147`):
/// cyan name padded to col 13, the 4-column flag-marker slot, blue
/// check byte, green size + two spaces, yellow date, reset before the
/// description. Only called for frameable rows (name < 13 chars, size
/// within the 7-column field), so the colour boundaries always land on
/// the fixed offsets.
///
/// The marker slot sits inside the existing blue run with the check
/// byte — `[X] ` when `is_flagged`, four spaces otherwise (no new SGR;
/// design 2026-06-12 §5).
pub(super) fn framed_row(file: &File, is_flagged: bool) -> Vec<u8> {
    let plain = super::dir_row::dir_row_lines(file)
        .into_iter()
        .next()
        .unwrap_or_default();
    let slot = if is_flagged {
        MARKER_FLAGGED
    } else {
        MARKER_EMPTY
    };
    let mut row = b"\x1b[0m\x1b[36m".to_vec();
    row.extend_from_slice(&plain[..13]);
    row.extend_from_slice(b"\x1b[34m");
    row.extend_from_slice(slot);
    row.push(plain[13]);
    row.extend_from_slice(b"\x1b[32m");
    row.extend_from_slice(&plain[14..23]);
    row.extend_from_slice(b"\x1b[33m");
    row.extend_from_slice(&plain[23..31]);
    row.extend_from_slice(RESET);
    row.extend_from_slice(&plain[31..]);
    row
}

/// A plain fall-through line (unframeable rows and description
/// continuations): the raw bytes behind a doubled reset.
pub(super) fn plain_line(content: &[u8]) -> Vec<u8> {
    let mut line = b"\x1b[0m\x1b[0m".to_vec();
    line.extend_from_slice(content);
    line
}

/// The in-pager pause help (`?` at `More?`) — byte-exact from
/// `ae_tierd_aquascan4.txt` U2, leading form-feed through the
/// trailing `~SP|`+form-feed redraw marker. Advertises the door's
/// full verb surface; the navigation and cross-tier verbs are
/// advertised-but-inert in D2 (unknown keys continue — the door's
/// own default) and owed to their owning slices.
pub(super) const PAUSE_HELP: &[u8] = b"\x0c\r\n\
     \x1b[36mThese are the commands that can be used at the pause prompt:\r\n\
\r\n\
      \x1b[32m(\x1b[33mEnter\x1b[32m),(\x1b[33mY\x1b[32m),(\x1b[33mSpace\x1b[32m)\x1b[34m .. \x1b[36mContinue file scanning\r\n\
      \x1b[32m(\x1b[33mC\x1b[32m)\x1b[34m .................. \x1b[36mClear screen and continue scanning\r\n\
      \x1b[32m(\x1b[33mDownArrow\x1b[32m),(\x1b[33m3\x1b[32m)\x1b[34m ...... \x1b[36mGo one page down\r\n\
      \x1b[32m(\x1b[33mUpArrow\x1b[32m),(\x1b[33m9\x1b[32m)\x1b[34m ........ \x1b[36mGo one page up (back)\r\n\
      \x1b[32m(\x1b[33m7\x1b[32m)\x1b[34m .................. \x1b[36mGo to start of listing\r\n\
      \x1b[32m(\x1b[33m5\x1b[32m)\x1b[34m .................. \x1b[36mRedraw page\r\n\
      \x1b[32m(\x1b[33mNS\x1b[32m)\x1b[34m ................. \x1b[36mTurn on non-stop scrolling\r\n\
      \x1b[32m(\x1b[33m?\x1b[32m)\x1b[34m .................. \x1b[36mView this help page\r\n\
      \x1b[32m(\x1b[33mF\x1b[32m)\x1b[34m .................. \x1b[36mFlag files by name\r\n\
      \x1b[32m(\x1b[33mR\x1b[32m),(\x1b[33m#\x1b[32m)\x1b[34m .............. \x1b[36mFlag files by number\r\n\
      \x1b[32m(\x1b[33mK\x1b[32m)\x1b[34m .................. \x1b[36mSkip dir\r\n\
      \x1b[32m(\x1b[33mL\x1b[32m)\x1b[34m .................. \x1b[36mReload dir\r\n\
      \x1b[32m(\x1b[33mN\x1b[32m),(\x1b[33mQ\x1b[32m)\x1b[34m .............. \x1b[36mQuit\r\n\
      \x1b[32m(\x1b[33mCtrl-C\x1b[32m)\x1b[34m ............. \x1b[36mQuit (Can be used at any time)\r\n\
      \x1b[32m(\x1b[33mD\x1b[32m)\x1b[34m .................. \x1b[36mQuit and download\r\n\
      \x1b[32m(\x1b[33mX\x1b[32m)\x1b[34m .................. \x1b[36mMark fake file\x1b[0m\r\n\
      \x1b[32m(\x1b[33mV\x1b[32m)\x1b[34m .................. \x1b[36mView a file\x1b[0m\r\n\
      \x1b[32m(\x1b[33mO\x1b[32m)\x1b[34m .................. \x1b[36mWho are online?\x1b[0m\r\n\
      \x1b[32m(\x1b[33mZ\x1b[32m)\x1b[34m .................. \x1b[36mZippy search\x1b[0m\r\n\
      \x1b[32m(\x1b[33mA\x1b[32m)\x1b[34m .................. \x1b[36mAlter file flags\x1b[0m\r\n\
\x1b[0m\r\n\
\x1b[0m~SP|\x0c\x1b[0m\r\n\
";

/// The `F ?` help screen — byte-exact from `ae_tierd_aquascan3.txt`
/// S1 (:100-129) with the three `NextScan` branding swaps: form feed,
/// the Copyright help banner, the verbatim syntax/diagram text, and
/// the captured epilogue (one reset blank, a doubled-reset blank, a
/// reset blank).
pub(super) const HELP_SCREEN: &str = "\x1b[0m\x0c\r\n\
\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m\r\n\
\r\n\
\r\n\
\x1b[0m  F                           \x1b[36m- Show the FileHelp and prompt for date and dir\r\n\
\x1b[0m  F R                         \x1b[36m- Same as above but use reverse scanning\r\n\
\x1b[0m  F ?                         \x1b[36m- Show this help text\r\n\
\x1b[0m  F W                         \x1b[36m- Configure NextScan\r\n\
\x1b[0m  F [R] dir [Q] [NS]          \x1b[36m- Start scanning immediately\r\n\
     ^  ^    ^   ^\r\n\
     |  |    |   |\r\n\
     |  |    |   `-- Non-stop scrolling\r\n\
     |  |    `--- Quick scan = Show only first line of every description\r\n\
     |  |\r\n\
     |  +-- U -- Upload dir\r\n\
     |  +-- A -- All dirs\r\n\
     |  +-- x -- Dir number x\r\n\
     |  `-- H -- Hold dir\r\n\
     |\r\n\
     `-- Scan in reverse chronological order\r\n\
\x1b[0m\r\n\
\x1b[0m\x1b[0m\r\n\
\x1b[0m\r\n\
";

/// Whether the `NextScan` framer can frame this row: the captured
/// fall-through cases are names that fill the check-byte column
/// (≥ 13 chars) and sizes past the 7-column field — both leave the
/// colour boundaries nowhere fixed to land.
fn frameable(file: &File) -> bool {
    file.name().len() < 13 && file.size().count() <= super::dir_row::MAX_ALIGNED_SIZE
}

/// Assembles one directory's listing body — everything between the
/// scan header and the pager: date-group separator blocks, `[ File #N ]`
/// headers, colour-framed rows with plain continuations, plain
/// fall-through rows, and the `[ End of File List ]` footer directly
/// after the last row (`ae_tierd_aquascan3.txt:142-157`).
///
/// The captured grouping rule: a separator block precedes a framed
/// file only when it is the dir's first framed file or its `MM-DD-YY`
/// date differs from the previous *framed* file's; same-date framed
/// neighbours butt-join, and plain rows attach directly, consume no
/// file number, and are invisible to the grouping
/// (`ae_tierd_aquascan5.txt` V1, `ae_tierd_aquascan3.txt` S7).
/// Empty input assembles nothing — `Nothing found!` dirs get neither
/// body nor footer.
///
/// Each file's identity (`conference`, `area`, name) keys the flag set:
/// an aligned (framed) row carries the 4-column marker slot, an
/// over-long (plain) row appends a trailing ` [X]` when flagged. The
/// `ScanLine` of a file's first row carries its [`ListedRow`]; every
/// other line (separators, headers, continuations, footer) is raw.
///
/// `quick` (`N`'s `Q` token, capture N7q) truncates every file to the
/// first line of its description — no continuation rows. `F` passes
/// `false` unconditionally (the door supports `F <dir> Q`, but landed
/// `F` swallows `Q` as Invalid — a recorded follow-up).
pub(super) fn assemble_dir_lines(
    files: &[File],
    conference: u32,
    flagged: &FlaggedFiles,
    quick: bool,
) -> Vec<ScanLine> {
    let mut lines: Vec<ScanLine> = Vec::new();
    let mut previous_framed_date: Option<String> = None;
    let mut file_number = 0u32;

    for file in files {
        let key = FlaggedKey::new(conference, file.name());
        let is_flagged = flagged.contains(&key);
        let mut rows = super::dir_row::dir_row_lines(file).into_iter();
        let Some(first_row) = rows.next() else {
            continue;
        };
        let indent = if frameable(file) {
            let date = String::from_utf8_lossy(&first_row[23..31]).into_owned();
            if previous_framed_date.as_deref() != Some(date.as_str()) {
                lines.extend(separator_block(&date).into_iter().map(ScanLine::raw));
                previous_framed_date = Some(date);
            }
            file_number += 1;
            lines.push(ScanLine::raw(file_number_header(file_number)));
            lines.push(ScanLine {
                bytes: framed_row(file, is_flagged),
                listed: Some(ListedRow {
                    key,
                    number: Some(file_number),
                    aligned: true,
                }),
            });
            // Framed continuations indent past the wider marker slot:
            // the legacy 33-space indent plus the 4-column slot.
            37
        } else {
            let mut bytes = plain_line(&first_row);
            if is_flagged {
                bytes.extend_from_slice(MARKER_TRAILING);
            }
            lines.push(ScanLine {
                bytes,
                listed: Some(ListedRow {
                    key,
                    number: None,
                    aligned: false,
                }),
            });
            // Plain continuations keep the legacy 33-space indent.
            33
        };
        if !quick {
            lines.extend(
                rows.map(|continuation| {
                    ScanLine::raw(plain_line(&reindent(&continuation, indent)))
                }),
            );
        }
    }

    if !lines.is_empty() {
        lines.push(ScanLine::raw(END_OF_FILE_LIST.to_vec()));
    }
    lines
}

/// The legacy `dir_row` continuation indent (`express.e:19499`); every
/// continuation arrives padded to exactly this width.
const LEGACY_CONTINUATION_INDENT: usize = 33;

/// Re-indents a `dir_row` continuation (authored at the legacy
/// 33-space indent) to `indent` leading spaces, preserving the
/// description text verbatim. Framed rows widen this to 37 for the
/// marker slot; plain rows pass the legacy indent straight through.
fn reindent(continuation: &[u8], indent: usize) -> Vec<u8> {
    let text = continuation
        .get(LEGACY_CONTINUATION_INDENT..)
        .unwrap_or_default();
    let mut line = vec![b' '; indent];
    line.extend_from_slice(text);
    line
}

/// Visible columns of a rendered row — bytes outside `ESC[..m` SGR
/// runs, one per byte. Repaint targets are file rows, which are ASCII
/// outside SGR; the high-bit separator art is never a repaint target.
pub(super) fn visible_columns(bytes: &[u8]) -> usize {
    let mut width = 0;
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        if byte == 0x1b {
            let end = rest
                .iter()
                .position(|&b| b == b'm')
                .expect("SGR sequence terminated");
            rest = &rest[end + 1..];
        } else {
            width += 1;
            rest = &rest[1..];
        }
    }
    width
}

/// Scan header for dir `n`. Forward (`F`):
/// `Scanning dir N from top... Ok! / Nothing found!`
/// (`ae_tierd_aquascan3.txt:140`, `ae_tierd_aquascan.txt:522`). Reverse
/// (`FR`): `Reverse-scanning dir N... Ok! / Nothing found!` — no
/// "from top" (`ae_tierd_aquascan3.txt:696`).
pub(super) fn scanning_dir_header(n: u32, found: bool, reverse: bool) -> Vec<u8> {
    let outcome = if found { "Ok!" } else { "Nothing found!" };
    if reverse {
        format!("Reverse-scanning dir {n}... {outcome}").into_bytes()
    } else {
        format!("Scanning dir {n} from top... {outcome}").into_bytes()
    }
}

/// The hold-directory variant (`ae_tierd_aquascan3.txt:675-687`).
pub(super) fn scanning_hold_header(found: bool) -> Vec<u8> {
    let outcome = if found { "Ok!" } else { "Nothing found!" };
    format!("Scanning HOLD dir from top... {outcome}").into_bytes()
}

/// Out-of-range directory argument (`ae_tierd_aquascan.txt:330-342`).
pub(super) fn highest_dir_error(max: u32) -> Vec<u8> {
    format!("The highest directory number is {max}!").into_bytes()
}

/// The door's own directories prompt for bare `F`
/// (`ae_tierd_aquascan3.txt:163`; capital `N`one, space before `?` —
/// deliberately distinct from the stock `getDirSpan` prompt).
pub(super) fn directories_prompt(max: u32) -> Vec<u8> {
    format!(
        "\x1b[36mDirectories: \x1b[32m(\x1b[33m1-{max}\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m "
    )
    .into_bytes()
}

// --- Slice D4: the internal `Z` (zippy text search) wire ---------------
//
// `Z` runs `internalCommandZ` (`amiexpress/express.e:26123`), *not* the
// AquaScan door, so its wire is the genuine internal command's — plain
// text rows (no NextScan frames), a distinct directory prompt, and the
// internal `getDirSpan` error. Pinned to
// `comparison/transcripts/ae_tierd_zippy.txt` /
// `ae_tierd_zippy2.txt`. Kept here beside the `F` constants because both
// browse the same file areas and share [`super::dir_row::dir_row_lines`].

/// The search-string prompt (`amiexpress/express.e:26150`,
/// `ae_tierd_zippy.txt` Z1) — plain text, trailing space, no ANSI, no
/// terminator. Shown only for bare `Z`; `Z <token>` reads the string
/// from the argument.
pub(super) const ZIPPY_SEARCH_PROMPT: &[u8] = b"Enter string to search for: ";

/// The internal `getDirSpan('')` directory prompt
/// (`amiexpress/express.e:26864`, `ae_tierd_zippy.txt` Z1). Distinct
/// from the `AquaScan` [`directories_prompt`]: lowercase `=none?`, the
/// `?` followed by a space, and a closing reset with **no** trailing
/// space.
pub(super) fn zippy_directories_prompt(max: u32) -> Vec<u8> {
    format!(
        "\x1b[36mDirectories: \x1b[32m(\x1b[33m1-{max}\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none? \x1b[0m"
    )
    .into_bytes()
}

/// The per-directory scan header for the internal zippy
/// (`amiexpress/express.e:26191`, `ae_tierd_zippy.txt` Z1) — plain text,
/// no terminator. The upload (`U`) answer renders the highest dir's
/// number here, not the word "UPLOAD" (`:26181-26186`,
/// `ae_tierd_zippy2.txt` ZU).
pub(super) fn zippy_scanning_dir_header(n: u32) -> Vec<u8> {
    format!("Scanning directory {n}").into_bytes()
}

/// The hold-directory scan header (`amiexpress/express.e:26198`,
/// `ae_tierd_zippy2.txt` ZH) — no terminator.
pub(super) const ZIPPY_SCANNING_HOLD: &[u8] = b"Scanning directory HOLD";

/// The internal `getDirSpan` out-of-range error
/// (`amiexpress/express.e:26905`, `ae_tierd_zippy2.txt` ZOOR) — no
/// terminator. Distinct from `AquaScan`'s [`highest_dir_error`]. The
/// handler frames it with the legacy leading and trailing blanks.
pub(super) const ZIPPY_NO_SUCH_DIRECTORY: &[u8] = b"No such directory.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible columns of `bytes` — delegates to the module-scope
    /// [`visible_columns`] (the repaint helper shares this logic).
    fn visible_width(bytes: &[u8]) -> usize {
        visible_columns(bytes)
    }

    /// The captured `AquaScan` originals the `NextScan` swaps must match
    /// in visible width (`ae_tierd_aquascan3.txt:163` / `:105`).
    const AQUASCAN_LISTING_BANNER: &[u8] =
        b"\x1b[0m\x1b[34m--[ \x1b[36mAquaScan v1.0 by Aquarius/Outlaws \x1b[34m]---------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m";
    const AQUASCAN_HELP_BANNER: &[u8] =
        b"\x1b[0m\x1b[34m--[ \x1b[36mAquaScan v1.0 by Aquarius/Outlaws \x1b[34m]---------[ \x1b[36mCopyright \xa9 1994 Aquarius \x1b[34m]--\x1b[0m";

    #[test]
    fn listing_banner_swaps_the_brand_and_holds_the_frame_width() {
        assert_eq!(
            LISTING_BANNER,
            &b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m"[..],
        );
        assert_eq!(visible_width(LISTING_BANNER), 77);
        assert_eq!(
            visible_width(LISTING_BANNER),
            visible_width(AQUASCAN_LISTING_BANNER),
        );
    }

    #[test]
    fn reverse_listing_banner_swaps_the_label_and_holds_the_frame_width() {
        // `FR`: the right label grows `'f ?'`→`'fr ?'` (one col wider),
        // so the dash run flexes 40→39 to keep the 77-col frame
        // (`ae_tierd_aquascan3.txt` S10/S11).
        assert_eq!(
            listing_banner(true),
            &b"\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]---------------------------------------[ \x1b[36m'fr ?' for options \x1b[34m]--\x1b[0m"[..],
        );
        assert_eq!(visible_width(listing_banner(true)), 77);
        // Forward selection is byte-identical to the const.
        assert_eq!(listing_banner(false), LISTING_BANNER);
    }

    #[test]
    fn reverse_scan_header_drops_from_top() {
        // `Reverse-scanning dir N... Ok! / Nothing found!`
        // (`ae_tierd_aquascan3.txt:696`) — no "from top".
        assert_eq!(
            scanning_dir_header(1, true, true),
            b"Reverse-scanning dir 1... Ok!".to_vec(),
        );
        assert_eq!(
            scanning_dir_header(2, false, true),
            b"Reverse-scanning dir 2... Nothing found!".to_vec(),
        );
        // Forward (reverse=false) keeps the captured "from top" text.
        assert_eq!(
            scanning_dir_header(1, true, false),
            b"Scanning dir 1 from top... Ok!".to_vec(),
        );
    }

    /// Visible columns of a UTF-8 string: every char outside `ESC[..m`
    /// SGR runs is one column (all `NextScan` glyphs are single-cell).
    fn visible_width_str(s: &str) -> usize {
        let mut width = 0;
        let mut rest = s;
        while let Some(c) = rest.chars().next() {
            if c == '\x1b' {
                let end = rest.find('m').expect("SGR sequence terminated");
                rest = &rest[end + 1..];
            } else {
                width += 1;
                rest = &rest[c.len_utf8()..];
            }
        }
        width
    }

    #[test]
    fn all_wire_output_is_valid_utf8() {
        // Encoding policy (AGENTS.md "Wire encoding"): the NextExpress
        // wire is valid UTF-8. The art constants are &str by type so
        // only the assembled byte path (which splices them alongside
        // raw &[u8] segments) needs checking here. The whole-session
        // gate lives in
        // tierd_file_list_smoke.rs::utf8_gate_every_session_byte_decodes.
        assert!(String::from_utf8(separator_block("01-15-26").concat()).is_ok());
    }

    #[test]
    fn help_banner_swaps_brand_and_copyright_and_holds_width() {
        assert_eq!(
            HELP_BANNER,
            "\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m",
        );
        assert_eq!(visible_width_str(HELP_BANNER), 79);
        assert_eq!(
            visible_width_str(HELP_BANNER),
            visible_width(AQUASCAN_HELP_BANNER)
        );
    }

    #[test]
    fn separator_block_carries_the_file_date_in_the_second_line() {
        // ae_tierd_aquascan3.txt:143-144: art lines at 44- and
        // 6-space indents, the second carrying the date.
        let block = separator_block("01-15-26");
        assert_eq!(block.len(), 4);
        assert_eq!(block[0], b"\x1b[0m".to_vec());
        let mut line_a = b"\x1b[0m".to_vec();
        line_a.extend_from_slice(&[b' '; 44]);
        line_a.extend_from_slice("_¸,ø*¤°¬°¤*ø,¸_¸,ø*¤°¬¬°¤*ø,¸_".as_bytes());
        assert_eq!(block[1], line_a);
        let mut line_b = b"\x1b[0m".to_vec();
        line_b.extend_from_slice(&[b' '; 6]);
        line_b.extend_from_slice("¸,ø*¤°¬¯¬°¤*ø,¸_¸,ø*¤°¬°¤*ø, 01-15-26".as_bytes());
        assert_eq!(block[2], line_b);
        assert_eq!(block[3], b"\x1b[0m".to_vec());
    }

    #[test]
    fn file_number_header_pads_to_a_stable_width() {
        // ae_tierd_aquascan3.txt:146 (1-digit) vs the S7 repr's
        // `File #10` (2-digit): the pad shrinks so the dash run stays
        // at visible column 31; total visible width 79.
        let one_digit = file_number_header(1);
        assert_eq!(
            one_digit,
            b"\x1b[0m\x1b[34m[\x1b[0m File #1 \x1b[34m]                    - ---- ---------------------------------- ---- -".to_vec(),
        );
        let two_digit = file_number_header(10);
        assert_eq!(
            two_digit,
            b"\x1b[0m\x1b[34m[\x1b[0m File #10 \x1b[34m]                   - ---- ---------------------------------- ---- -".to_vec(),
        );
        assert_eq!(visible_width(&one_digit), 79);
        assert_eq!(visible_width(&two_digit), 79);
    }

    #[test]
    fn framed_row_colours_the_dir_row_fields() {
        // ae_tierd_aquascan3.txt:147: cyan name to col 13, blue check
        // byte, green size + two spaces, yellow date, reset before
        // the two description spaces.
        use crate::domain::bytes::Bytes;
        use crate::domain::files::file::{File, FileStatus};
        let file = File::new(
            "FRESHUPL.LHA".to_string(),
            Bytes::new(43_210),
            FileStatus::Available,
            Some(b'P'),
            "Uploaded last night, awaiting sort".to_string(),
            std::time::SystemTime::from(time::macros::datetime!(2026-06-09 12:00 UTC)),
        );
        assert_eq!(
            framed_row(&file, false),
            b"\x1b[0m\x1b[36mFRESHUPL.LHA \x1b[34m    P\x1b[32m  43210  \x1b[33m06-09-26\x1b[0m  Uploaded last night, awaiting sort".to_vec(),
        );
    }

    #[test]
    fn plain_rows_get_the_double_reset_prefix() {
        // Unframeable rows stream the raw DIR row behind `ESC[0m`
        // twice (ae_tierd_aquascan3.txt S7 repr; same prefix as
        // continuation lines).
        assert_eq!(plain_line(b"THIRTEENCH.LZ   66666"), {
            let mut line = b"\x1b[0m\x1b[0m".to_vec();
            line.extend_from_slice(b"THIRTEENCH.LZ   66666");
            line
        });
    }

    #[test]
    fn scan_headers_match_the_captures() {
        assert_eq!(
            scanning_dir_header(1, true, false),
            b"Scanning dir 1 from top... Ok!".to_vec()
        );
        assert_eq!(
            scanning_dir_header(2, false, false),
            b"Scanning dir 2 from top... Nothing found!".to_vec(),
        );
        assert_eq!(
            scanning_hold_header(false),
            b"Scanning HOLD dir from top... Nothing found!".to_vec(),
        );
        assert_eq!(
            scanning_hold_header(true),
            b"Scanning HOLD dir from top... Ok!".to_vec(),
        );
        assert_eq!(
            highest_dir_error(2),
            b"The highest directory number is 2!".to_vec(),
        );
    }

    #[test]
    fn prompts_match_the_captures() {
        assert_eq!(
            MORE_PROMPT,
            &b"\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m "[..],
        );
        assert_eq!(
            NS_CONFIRM_PROMPT,
            &b"\x1b[36mNon-stop scrolling! Are you sure \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[36m? "[..],
        );
        assert_eq!(
            FLAG_BY_NAME_PROMPT,
            &b"\x1b[36mFile name(s) to flag:\x1b[0m "[..],
        );
        assert_eq!(
            FLAG_BY_NUMBER_PROMPT,
            &b"\x1b[36mFile number(s) to flag:\x1b[0m "[..],
        );
        assert_eq!(
            directories_prompt(2),
            b"\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m ".to_vec(),
        );
    }

    fn seeded(
        name: &str,
        size: u64,
        check: Option<u8>,
        at: time::OffsetDateTime,
        desc: &str,
    ) -> crate::domain::files::file::File {
        use crate::domain::bytes::Bytes;
        use crate::domain::files::file::{File, FileStatus};
        File::new(
            name.to_string(),
            Bytes::new(size),
            FileStatus::Available,
            check,
            desc.to_string(),
            std::time::SystemTime::from(at),
        )
    }

    #[test]
    fn assembles_the_captured_dir2_trio_with_the_date_group_rule() {
        // ae_tierd_aquascan3.txt:142-157 (S2) / ae_tierd_aquascan5.txt
        // V1: separator block before #1 and before #2 (date change),
        // NONE before same-date #3, which butt-joins straight after
        // #2's continuation line; footer directly after the last row.
        use time::macros::datetime;
        let files = vec![
            seeded(
                "FRESHUPL.LHA",
                43_210,
                Some(b'P'),
                datetime!(2026-06-09 12:00 UTC),
                "Uploaded last night, awaiting sort",
            ),
            seeded(
                "MYDEMO.DMS",
                567_890,
                Some(b'P'),
                datetime!(2026-06-10 12:00 UTC),
                "My first demo - feedback welcome!\nGreets to everyone on node 1.",
            ),
            seeded(
                "TOOLPACK.LHA",
                234_567,
                Some(b'P'),
                datetime!(2026-06-10 12:00 UTC),
                "Misc CLI tools collection",
            ),
        ];
        let mut expected: Vec<Vec<u8>> = Vec::new();
        expected.extend(separator_block("06-09-26"));
        expected.push(file_number_header(1));
        expected.push(framed_row(&files[0], false));
        expected.extend(separator_block("06-10-26"));
        expected.push(file_number_header(2));
        expected.push(framed_row(&files[1], false));
        // Framed continuation: the legacy 33-space indent widened to
        // 37 for the marker slot.
        expected.push(plain_line(
            b"                                     Greets to everyone on node 1.",
        ));
        expected.push(file_number_header(3));
        expected.push(framed_row(&files[2], false));
        expected.push(END_OF_FILE_LIST.to_vec());
        let actual: Vec<Vec<u8>> = assemble_dir_lines(&files, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn quick_scan_drops_description_continuations() {
        // Capture N7q (`ae_tierd_newfiles.txt` pass 2, `N 01-01-26 2 Q`):
        // with the quick flag each file shows only the first line of its
        // description — MYDEMO's continuation is absent and File #3's
        // header follows the framed row directly; everything else
        // (separators, frames, footer) is unchanged.
        use time::macros::datetime;
        let files = vec![
            seeded(
                "FRESHUPL.LHA",
                43_210,
                Some(b'P'),
                datetime!(2026-06-09 12:00 UTC),
                "Uploaded last night, awaiting sort",
            ),
            seeded(
                "MYDEMO.DMS",
                567_890,
                Some(b'P'),
                datetime!(2026-06-10 12:00 UTC),
                "My first demo - feedback welcome!\nGreets to everyone on node 1.",
            ),
            seeded(
                "TOOLPACK.LHA",
                234_567,
                Some(b'P'),
                datetime!(2026-06-10 12:00 UTC),
                "Misc CLI tools collection",
            ),
        ];
        let mut expected: Vec<Vec<u8>> = Vec::new();
        expected.extend(separator_block("06-09-26"));
        expected.push(file_number_header(1));
        expected.push(framed_row(&files[0], false));
        expected.extend(separator_block("06-10-26"));
        expected.push(file_number_header(2));
        expected.push(framed_row(&files[1], false));
        // Quick: NO continuation line — File #3 butt-joins directly.
        expected.push(file_number_header(3));
        expected.push(framed_row(&files[2], false));
        expected.push(END_OF_FILE_LIST.to_vec());
        let actual: Vec<Vec<u8>> = assemble_dir_lines(&files, 1, &FlaggedFiles::default(), true)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn plain_rows_attach_without_separator_or_number_and_are_invisible_to_grouping() {
        // ae_tierd_aquascan3.txt S7 repr: DISKMAST (framed) is
        // followed directly by the unframeable MEGADEMO row (8-digit
        // size, different date, still no separator), and the next
        // framed file gets its separator from the date change against
        // DISKMAST — numbering skips the plain row.
        use time::macros::datetime;
        let files = vec![
            seeded(
                "DISKMAST.LHA",
                234_511,
                Some(b'P'),
                datetime!(2026-04-22 12:00 UTC),
                "DiskMaster 2.1 - directory utility",
            ),
            seeded(
                "MEGADEMO.DMS",
                12_345_678,
                Some(b'P'),
                datetime!(2026-05-01 12:00 UTC),
                "Spaceballs mega demo disk 1 of 2",
            ),
            seeded(
                "XPRZMODM.LHA",
                56_789,
                Some(b'P'),
                datetime!(2026-05-06 12:00 UTC),
                "XPR Zmodem library 2.10",
            ),
        ];
        let mut expected: Vec<Vec<u8>> = Vec::new();
        expected.extend(separator_block("04-22-26"));
        expected.push(file_number_header(1));
        expected.push(framed_row(&files[0], false));
        expected.push(plain_line(
            b"MEGADEMO.DMS P12345678  05-01-26  Spaceballs mega demo disk 1 of 2",
        ));
        expected.extend(separator_block("05-06-26"));
        expected.push(file_number_header(2));
        expected.push(framed_row(&files[2], false));
        expected.push(END_OF_FILE_LIST.to_vec());
        let actual: Vec<Vec<u8>> = assemble_dir_lines(&files, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn thirteen_char_names_fall_through_plain() {
        use time::macros::datetime;
        let files = vec![seeded(
            "THIRTEENCH.LZ",
            66_666,
            None,
            datetime!(2026-05-20 12:00 UTC),
            "Exactly thirteen character filename",
        )];
        let expected = vec![
            plain_line(b"THIRTEENCH.LZ   66666  05-20-26  Exactly thirteen character filename"),
            END_OF_FILE_LIST.to_vec(),
        ];
        let actual: Vec<Vec<u8>> = assemble_dir_lines(&files, 1, &FlaggedFiles::default(), false)
            .into_iter()
            .map(|line| line.bytes)
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn empty_dir_assembles_no_lines() {
        assert!(assemble_dir_lines(&[], 1, &FlaggedFiles::default(), false).is_empty());
    }

    #[test]
    fn aligned_rows_carry_the_marker_slot() {
        // Design 2026-06-12 §5: an aligned (framed, name < 13) row
        // splices the 4-column marker slot inside the blue run — four
        // spaces unflagged, `[X] ` flagged — and the file's first
        // `ScanLine` records its aligned identity for the F/R verbs.
        use time::macros::datetime;
        let file = seeded(
            "ANSIPACK.LHA",
            234_567,
            Some(b'P'),
            datetime!(2026-01-15 12:00 UTC),
            "Collection of 40 ANSI screens from the",
        );

        let unflagged = assemble_dir_lines(
            std::slice::from_ref(&file),
            1,
            &FlaggedFiles::default(),
            false,
        );
        let row = &unflagged[unflagged.len() - 2];
        assert_eq!(
            row.bytes,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m    P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the".to_vec(),
        );

        let key = FlaggedKey::new(1, "ANSIPACK.LHA");
        let mut flags = FlaggedFiles::default();
        flags.flag(key.clone());
        let flagged = assemble_dir_lines(&[file], 1, &flags, false);
        let row = &flagged[flagged.len() - 2];
        assert_eq!(
            row.bytes,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m[X] P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the".to_vec(),
        );
        assert_eq!(
            row.listed,
            Some(ListedRow {
                key: key.clone(),
                number: Some(1),
                aligned: true,
            }),
        );
        assert!(flags.contains(&key), "the flagged key drives the slot");
    }

    #[test]
    fn prompt_flagged_files_paint_the_marker_in_listings() {
        // A file flagged at the A prompt (or restored on logon) carries
        // no catalogue area, yet it is the same (conference, name) file
        // the F listing shows — the marker must paint. Before the
        // July 2026 identity fix the area-0 key never matched the
        // listing's real-area key and the marker stayed blank.
        use time::macros::datetime;
        let file = seeded(
            "ANSIPACK.LHA",
            234_567,
            Some(b'P'),
            datetime!(2026-01-15 12:00 UTC),
            "Collection of 40 ANSI screens from the",
        );

        let mut flags = FlaggedFiles::default();
        flags.flag(FlaggedKey::new(1, "ansipack.lha"));
        let lines = assemble_dir_lines(&[file], 1, &flags, false);
        let row = &lines[lines.len() - 2];
        assert_eq!(
            row.bytes,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m[X] P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the".to_vec(),
            "a prompt-flagged file paints [X] in the listing"
        );
    }

    #[test]
    fn overlong_names_append_the_marker_when_flagged() {
        // Design 2026-06-12 §5: an over-long (unframeable, name >= 13)
        // row has no slot to splice — when flagged it appends a
        // trailing ` [X]`; its identity records `aligned == false`,
        // `number == None` (plain rows consume no file number).
        use time::macros::datetime;
        let file = seeded(
            "THIRTEENCH.LZ",
            66_666,
            None,
            datetime!(2026-05-20 12:00 UTC),
            "Exactly thirteen character filename",
        );

        let key = FlaggedKey::new(1, "THIRTEENCH.LZ");
        let mut flags = FlaggedFiles::default();
        flags.flag(key.clone());
        let flagged = assemble_dir_lines(&[file], 1, &flags, false);
        let row = &flagged[0];
        assert_eq!(
            row.bytes,
            plain_line(b"THIRTEENCH.LZ   66666  05-20-26  Exactly thirteen character filename [X]"),
        );
        assert_eq!(
            row.listed,
            Some(ListedRow {
                key,
                number: None,
                aligned: false,
            }),
        );
    }

    #[test]
    fn footer_and_errors_match_the_captures() {
        assert_eq!(
            END_OF_FILE_LIST,
            &b"\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m"[..],
        );
        assert_eq!(ERROR_IN_INPUT, &b"Error in input!"[..]);
        assert_eq!(ARGUMENT_ERROR, &b"Argument error! Type 'f ?' for help."[..],);
    }
}
