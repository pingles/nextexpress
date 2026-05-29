//! Application-layer input limits and bounded editor helpers.

/// Maximum bytes accepted for a single terminal input line.
pub(crate) const MAX_TERMINAL_LINE_BYTES: usize = 4096;

/// Appends `line` plus a trailing newline when doing so keeps
/// `buffer` within `max_bytes`.
#[must_use]
pub(crate) fn append_line_with_newline(buffer: &mut String, line: &str, max_bytes: usize) -> bool {
    let Some(with_line) = buffer.len().checked_add(line.len()) else {
        return false;
    };
    let Some(with_newline) = with_line.checked_add(1) else {
        return false;
    };
    if with_newline > max_bytes {
        return false;
    }
    buffer.push_str(line);
    buffer.push('\n');
    true
}

#[cfg(test)]
mod tests {
    use super::append_line_with_newline;

    #[test]
    fn appends_line_when_it_fits_exactly() {
        let mut buffer = String::from("abc");
        assert!(append_line_with_newline(&mut buffer, "de", 6));
        assert_eq!(buffer, "abcde\n");
    }

    #[test]
    fn rejects_line_when_newline_would_exceed_limit() {
        let mut buffer = String::from("abc");
        assert!(!append_line_with_newline(&mut buffer, "de", 5));
        assert_eq!(buffer, "abc");
    }
}
