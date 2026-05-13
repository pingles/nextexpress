//! Parser for menu command lines.
//!
//! The parser is deliberately effect-free: it turns a raw terminal
//! line into a typed command shape and leaves all session, repository,
//! and terminal effects to [`crate::app::menu_flow::MenuFlow`].

/// Parsed menu command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuCommand {
    /// `G`: user requested logoff.
    Logoff,
    /// `J` / `J <number>`: explicit conference join.
    Join(NumberArg),
    /// `R` / `R <number>`: read one message.
    Read(NumberArg),
    /// `M` / `N`: scan all/new mail.
    Scan(ScanArg),
    /// `E` / `E <to>`: enter a message.
    Post(PostArg),
    /// `C`: comment to sysop (Slice 44).
    CommentToSysop,
    /// Any command not recognised by this slice.
    Unknown,
}

/// Parsed numeric argument shared by `J` and `R` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberArg {
    /// `<command> <n>` where `<n>` parsed as a `u32`.
    Number(u32),
    /// `<command>` with no number.
    Missing,
    /// `<command> <token>` where `<token>` could not be parsed as a
    /// `u32`, or where extra trailing tokens were supplied.
    Invalid,
}

/// Parsed shape of an `M` / `N` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanArg {
    /// `N` - scan from `last_scanned + 1`. Surfaces unread mail the
    /// user has not yet been alerted to.
    New,
    /// `M` - scan from message 1. Lists every message visible to the
    /// user in the current msgbase as the unread set.
    All,
}

/// Parsed shape of an `E` / `E <to>` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostArg {
    /// `E <to>` where `<to>` is one-or-more tokens after the command
    /// and is kept verbatim.
    To(String),
    /// `E` with no inline recipient. The handler prompts for it.
    Missing,
}

/// Parses a raw menu line into a typed [`MenuCommand`].
#[must_use]
pub(crate) fn parse_menu_command(line: &str) -> MenuCommand {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("G") {
        return MenuCommand::Logoff;
    }
    if trimmed.eq_ignore_ascii_case("C") {
        return MenuCommand::CommentToSysop;
    }
    if let Some(arg) = parse_number_command(trimmed, "J") {
        return MenuCommand::Join(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "R") {
        return MenuCommand::Read(arg);
    }
    if let Some(scan) = parse_scan_command(trimmed) {
        return MenuCommand::Scan(scan);
    }
    if let Some(post) = parse_post_command(trimmed) {
        return MenuCommand::Post(post);
    }
    MenuCommand::Unknown
}

fn parse_number_command(line: &str, command: &str) -> Option<NumberArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case(command) {
        return None;
    }
    let Some(arg) = tokens.next() else {
        return Some(NumberArg::Missing);
    };
    if tokens.next().is_some() {
        return Some(NumberArg::Invalid);
    }
    match arg.parse::<u32>() {
        Ok(n) => Some(NumberArg::Number(n)),
        Err(_) => Some(NumberArg::Invalid),
    }
}

fn parse_scan_command(line: &str) -> Option<ScanArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    if head.eq_ignore_ascii_case("M") {
        Some(ScanArg::All)
    } else if head.eq_ignore_ascii_case("N") {
        Some(ScanArg::New)
    } else {
        None
    }
}

fn parse_post_command(line: &str) -> Option<PostArg> {
    let mut chars = line.chars();
    let head = chars.next()?;
    if !matches!(head, 'E' | 'e') {
        return None;
    }
    let rest: String = chars.collect();
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some(PostArg::Missing);
        }
        return None;
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(PostArg::To(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_logoff_command() {
        assert_eq!(parse_menu_command("G"), MenuCommand::Logoff);
        assert_eq!(parse_menu_command("g"), MenuCommand::Logoff);
    }

    #[test]
    fn parses_join_command_arguments() {
        assert_eq!(
            parse_menu_command("J 7"),
            MenuCommand::Join(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("j 7"),
            MenuCommand::Join(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("J"),
            MenuCommand::Join(NumberArg::Missing)
        );
        assert_eq!(
            parse_menu_command("J nope"),
            MenuCommand::Join(NumberArg::Invalid)
        );
        assert_eq!(
            parse_menu_command("J 1 2"),
            MenuCommand::Join(NumberArg::Invalid)
        );
    }

    #[test]
    fn parses_read_command_arguments() {
        assert_eq!(
            parse_menu_command("R 7"),
            MenuCommand::Read(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("r 7"),
            MenuCommand::Read(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("R"),
            MenuCommand::Read(NumberArg::Missing)
        );
        assert_eq!(
            parse_menu_command("R foo"),
            MenuCommand::Read(NumberArg::Invalid)
        );
        assert_eq!(
            parse_menu_command("R 1 2"),
            MenuCommand::Read(NumberArg::Invalid)
        );
    }

    #[test]
    fn read_command_zero_is_valid_parse_but_will_404_at_load_time() {
        assert_eq!(
            parse_menu_command("R 0"),
            MenuCommand::Read(NumberArg::Number(0))
        );
    }

    #[test]
    fn parses_scan_commands() {
        assert_eq!(parse_menu_command("M"), MenuCommand::Scan(ScanArg::All));
        assert_eq!(parse_menu_command("m"), MenuCommand::Scan(ScanArg::All));
        assert_eq!(parse_menu_command("N"), MenuCommand::Scan(ScanArg::New));
        assert_eq!(parse_menu_command("n"), MenuCommand::Scan(ScanArg::New));
    }

    #[test]
    fn scan_commands_reject_extra_tokens() {
        assert_eq!(parse_menu_command("M 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("N 7"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_post_command() {
        assert_eq!(parse_menu_command("E"), MenuCommand::Post(PostArg::Missing));
        assert_eq!(parse_menu_command("e"), MenuCommand::Post(PostArg::Missing));
        assert_eq!(
            parse_menu_command("E bob"),
            MenuCommand::Post(PostArg::To("bob".to_string()))
        );
        assert_eq!(
            parse_menu_command("E John Smith"),
            MenuCommand::Post(PostArg::To("John Smith".to_string()))
        );
    }

    #[test]
    fn unrelated_commands_are_unknown() {
        assert_eq!(parse_menu_command(""), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("Read 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("MS"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("EM"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_comment_to_sysop_command() {
        // Slice 44: bare `C` (case-insensitive) routes to
        // `messaging.allium:PostCommentToSysop`.
        assert_eq!(parse_menu_command("C"), MenuCommand::CommentToSysop);
        assert_eq!(parse_menu_command("c"), MenuCommand::CommentToSysop);
    }

    #[test]
    fn comment_command_rejects_extra_tokens() {
        // `C anything` is not a comment-to-sysop — the legacy command
        // takes no arguments and lands the user straight in the
        // editor.
        assert_eq!(parse_menu_command("C foo"), MenuCommand::Unknown);
    }
}
