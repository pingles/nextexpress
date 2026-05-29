//! Size limits for persisted mail content.
//!
//! These limits are deliberately byte-oriented because the terminal
//! and on-disk JSON paths are byte streams. They are high enough for
//! normal BBS mail while bounding memory and disk growth per message.

/// Maximum UTF-8 bytes accepted for a mail subject.
pub const MAX_MAIL_SUBJECT_BYTES: usize = 256;

/// Maximum UTF-8 bytes accepted for a mail body.
pub const MAX_MAIL_BODY_BYTES: usize = 64 * 1024;

/// Maximum bytes accepted for one persisted mail JSON file.
///
/// This is larger than [`MAX_MAIL_BODY_BYTES`] to allow headers,
/// timestamps, escaping overhead and small attachment manifests without
/// permitting unbounded startup/load reads.
pub const MAX_PERSISTED_MAIL_BYTES: u64 = 72 * 1024;

#[cfg(test)]
mod tests {
    use super::{MAX_MAIL_BODY_BYTES, MAX_MAIL_SUBJECT_BYTES, MAX_PERSISTED_MAIL_BYTES};

    #[test]
    fn mail_size_limits_are_explicit_values() {
        assert_eq!(MAX_MAIL_SUBJECT_BYTES, 256);
        assert_eq!(MAX_MAIL_BODY_BYTES, 65_536);
        assert_eq!(MAX_PERSISTED_MAIL_BYTES, 73_728);
    }
}
