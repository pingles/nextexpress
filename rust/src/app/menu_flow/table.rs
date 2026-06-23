//! Shared column-formatting helpers for the mail / list / scan
//! listing tables.
//!
//! Lifted out of `wire_text` so the mail, list and scan renderers can
//! share the one truncate-and-pad implementation without depending on
//! the transport-shaped `wire_text` module.

/// The 7-character status column for a listing row
/// (`amiexpress/express.e:11719`): only `Public` mail renders as
/// `"Public "`; every other (non-deleted) visibility is `"Private"`.
pub(crate) fn scan_row_status(
    visibility: crate::domain::messaging::mail::MailVisibility,
) -> &'static str {
    use crate::domain::messaging::mail::MailVisibility;
    match visibility {
        MailVisibility::Public => "Public ",
        _ => "Private",
    }
}

/// Left-justifies `value` within `width` columns, truncating it to
/// `width` characters first so the listing columns stay aligned even
/// for over-long handles or subjects (`AmigaE` `StringF` `\l\s[n]`).
pub(crate) fn left_field(value: &str, width: usize) -> String {
    let truncated: String = value.chars().take(width).collect();
    format!("{truncated:<width$}")
}
