//! The `yesNo` single-key confirm primitive, interpreted in the core.
//!
//! The legacy `yesNo(flag)` (`amiexpress/express.e:2129`) reads one key
//! and maps it to a yes/no answer: `y`/`Y` → yes, `n`/`N` → no, CR →
//! the `flag` default, anything else loops. That interpretation is a
//! core interaction concern, deliberately kept out of the transport
//! [`KeyEvent`](crate::app::terminal::KeyEvent) so the same keystrokes
//! can carry different meanings elsewhere — the file-list pager reads
//! `y`/`n` as continue/stop, not yes/no.

use crate::app::terminal::KeyEvent;

/// A yes/no answer to a single-key confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YesNo {
    Yes,
    No,
}

/// Interprets one keystroke as a [`YesNo`] answer, mirroring the legacy
/// `yesNo` mapping (`amiexpress/express.e:2143-2154`): `y`/`Y` →
/// [`YesNo::Yes`], `n`/`N` → [`YesNo::No`], [`KeyEvent::Enter`] →
/// `default`, any other key → `None` (the caller loops, as `yesNo`'s
/// `LOOP` does).
pub(crate) fn yes_no(key: KeyEvent, default: YesNo) -> Option<YesNo> {
    match key {
        KeyEvent::Char(b'y' | b'Y') => Some(YesNo::Yes),
        KeyEvent::Char(b'n' | b'N') => Some(YesNo::No),
        KeyEvent::Enter => Some(default),
        // Ctrl-C loops like any other non-answer key — the legacy
        // `yesNo` has no break-out (`express.e:2143-2154`).
        KeyEvent::Char(_) | KeyEvent::Backspace | KeyEvent::CtrlC | KeyEvent::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_keys_answer_yes_regardless_of_case_or_default() {
        assert_eq!(yes_no(KeyEvent::Char(b'y'), YesNo::No), Some(YesNo::Yes));
        assert_eq!(yes_no(KeyEvent::Char(b'Y'), YesNo::No), Some(YesNo::Yes));
        assert_eq!(yes_no(KeyEvent::Char(b'y'), YesNo::Yes), Some(YesNo::Yes));
    }

    #[test]
    fn n_keys_answer_no_regardless_of_case_or_default() {
        assert_eq!(yes_no(KeyEvent::Char(b'n'), YesNo::Yes), Some(YesNo::No));
        assert_eq!(yes_no(KeyEvent::Char(b'N'), YesNo::Yes), Some(YesNo::No));
        assert_eq!(yes_no(KeyEvent::Char(b'n'), YesNo::No), Some(YesNo::No));
    }

    #[test]
    fn enter_takes_the_default_either_way() {
        assert_eq!(yes_no(KeyEvent::Enter, YesNo::No), Some(YesNo::No));
        assert_eq!(yes_no(KeyEvent::Enter, YesNo::Yes), Some(YesNo::Yes));
    }

    #[test]
    fn any_other_key_does_not_answer() {
        assert_eq!(yes_no(KeyEvent::Char(b'x'), YesNo::No), None);
        assert_eq!(yes_no(KeyEvent::Char(b' '), YesNo::No), None);
        assert_eq!(yes_no(KeyEvent::Backspace, YesNo::No), None);
        assert_eq!(yes_no(KeyEvent::Other, YesNo::No), None);
    }
}
