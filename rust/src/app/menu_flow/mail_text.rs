//! Cross-mail-family wire text shared by the menu's mail commands.
//!
//! Lifted out of `wire_text` so the mail renderers (read, post, reply,
//! forward, scan, sysop-admin) own the user-facing strings they emit
//! rather than reaching across into the transport-shaped `wire_text`
//! module.

/// Sent when the current conference has no mail store configured.
/// In a correctly-configured BBS every conference's `MsgBase/`
/// directory backs a store; this notice surfaces a sysop
/// misconfiguration.
pub(crate) const NO_MAIL_BASE_LINE: &[u8] = b"\r\nNo message base for this conference.\r\n";

/// Sent when the underlying mail store rejects the request (I/O
/// failure, corrupted payload, etc.). The detailed error is logged
/// to stderr; the wire surface is intentionally generic so a bad
/// disk doesn't leak file paths to the user.
pub(crate) const MAIL_STORE_ERROR_LINE: &[u8] = b"\r\nMessage base error. Notify the sysop.\r\n";

/// Sent when the user aborts message composition (empty subject,
/// `/A` body command, or `~` shortcut). The session returns to the
/// menu prompt.
pub(crate) const POST_ABORTED_LINE: &[u8] = b"\r\nMessage aborted.\r\n";

/// Sent when the resolved recipient has no granted membership for
/// the current conference. Mirrors `amiexpress/express.e:10838`.
pub(crate) const POST_RECIPIENT_NO_ACCESS_LINE: &[u8] =
    b"\r\nUser does not have access to this conference.\r\n";

/// Sent when the user lacks `has_access(EnterMessage)`. The
/// pending-validation tier denies this right (Slice 21), so this
/// notice fires for not-yet-validated accounts.
pub(crate) const POST_ACCESS_DENIED_LINE: &[u8] =
    b"\r\nYou do not have permission to post messages.\r\n";

/// Sent when the user addresses ALL or EALL but the current message
/// base's [`AllowedAddressing`] policy refuses that broadcast kind
/// (Slice 43).
///
/// Mirrors the spirit of the legacy `enterMSG` "Echo To All" gate at
/// `amiexpress/express.e:10802` which checks the tooltype-driven
/// `ALLOW_ALL` flag.
pub(crate) const POST_ADDRESSING_NOT_ALLOWED_LINE: &[u8] =
    b"\r\nThis message base does not accept that addressee.\r\n";

/// Sent when a reply / forward / kill / move / edit-header command
/// references a message number that does not exist in the current
/// message base (Slice 49a / 49b).
pub(crate) const SOURCE_NOT_FOUND_LINE: &[u8] = b"\r\nNo such message in this base.\r\n";

/// Sent when a `FW` command's typed addressee cannot be resolved
/// (Slice 49a). Mirrors the `E` command's unknown-user surface so
/// the user can re-type without further explanation.
pub(crate) const FORWARD_UNKNOWN_USER_LINE: &[u8] = b"\r\nUnknown forward recipient.\r\n";

/// Formats the post-success line shown after `messaging.allium:PostMail`
/// (Slice 42). Mirrors the legacy `enterMSG` "Saving..." sequence at
/// `amiexpress/express.e:10972-10976`, simplified to a single line so
/// the menu loop can resume cleanly.
///
/// ```text
///   Message #<n> saved.
/// ```
pub(crate) fn render_post_success(number: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(b"\r\nMessage #");
    out.extend_from_slice(number.to_string().as_bytes());
    out.extend_from_slice(b" saved.\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_post_success_emits_message_number_and_terminator() {
        // Pin the legacy-aligned save confirmation.
        assert_eq!(render_post_success(7), b"\r\nMessage #7 saved.\r\n");
    }
}
