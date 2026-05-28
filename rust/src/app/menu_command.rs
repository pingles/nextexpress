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
    /// `RP <num>`: reply to message `<num>` (Slice 49a).
    Reply(NumberArg),
    /// `FW <num>`: forward message `<num>` (Slice 49a).
    Forward(NumberArg),
    /// `K <num>`: kill (soft-delete) message `<num>` (Slice 49b).
    Kill(NumberArg),
    /// `MV <num>`: move message `<num>` to a different msgbase (Slice 49b).
    Move(NumberArg),
    /// `EH <num>`: edit the header of message `<num>` (Slice 49b).
    EditHeader(NumberArg),
    /// `T`: print the current date and time (Tier A quickwin A1).
    /// Mirrors `internalCommandT()` at
    /// `amiexpress/express.e:25622-25644`.
    ShowTime,
    /// `VER`: print the version banner (Tier A quickwin A2).
    /// Mirrors `internalCommandVER()` at
    /// `amiexpress/express.e:25688-25698`.
    ShowVersion,
    /// `H`: print the BBS help screen (Tier A quickwin A5).
    /// Mirrors `internalCommandH()` at
    /// `amiexpress/express.e:25071-25087`.
    ShowHelp,
    /// `Q`: toggle the session's quiet mode (Tier A quickwin A9).
    /// Mirrors `internalCommandQ()` at
    /// `amiexpress/express.e:25504-25516`.
    QuietToggle,
    /// `S`: print the user statistics screen (Tier A quickwin A3).
    /// Mirrors `internalCommandS()` at
    /// `amiexpress/express.e:25540-25608`.
    ShowStats,
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
    if trimmed.eq_ignore_ascii_case("T") {
        return MenuCommand::ShowTime;
    }
    if trimmed.eq_ignore_ascii_case("VER") {
        return MenuCommand::ShowVersion;
    }
    if trimmed.eq_ignore_ascii_case("H") {
        return MenuCommand::ShowHelp;
    }
    if trimmed.eq_ignore_ascii_case("Q") {
        return MenuCommand::QuietToggle;
    }
    if trimmed.eq_ignore_ascii_case("S") {
        return MenuCommand::ShowStats;
    }
    if let Some(arg) = parse_number_command(trimmed, "J") {
        return MenuCommand::Join(arg);
    }
    // Two-letter commands resolved before the single-letter `R`
    // so `RP 1` doesn't get swallowed by the read parser as a
    // bogus read argument.
    if let Some(arg) = parse_number_command(trimmed, "RP") {
        return MenuCommand::Reply(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "FW") {
        return MenuCommand::Forward(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "MV") {
        return MenuCommand::Move(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "EH") {
        return MenuCommand::EditHeader(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "K") {
        return MenuCommand::Kill(arg);
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
    fn parses_reply_command() {
        assert_eq!(
            parse_menu_command("RP 7"),
            MenuCommand::Reply(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("rp 7"),
            MenuCommand::Reply(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("RP"),
            MenuCommand::Reply(NumberArg::Missing)
        );
        assert_eq!(
            parse_menu_command("RP nope"),
            MenuCommand::Reply(NumberArg::Invalid)
        );
    }

    #[test]
    fn parses_forward_command() {
        assert_eq!(
            parse_menu_command("FW 7"),
            MenuCommand::Forward(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("fw 7"),
            MenuCommand::Forward(NumberArg::Number(7))
        );
        assert_eq!(
            parse_menu_command("FW"),
            MenuCommand::Forward(NumberArg::Missing)
        );
    }

    #[test]
    fn parses_kill_command() {
        assert_eq!(
            parse_menu_command("K 2"),
            MenuCommand::Kill(NumberArg::Number(2))
        );
        assert_eq!(
            parse_menu_command("k 2"),
            MenuCommand::Kill(NumberArg::Number(2))
        );
    }

    #[test]
    fn parses_move_command() {
        assert_eq!(
            parse_menu_command("MV 3"),
            MenuCommand::Move(NumberArg::Number(3))
        );
        assert_eq!(
            parse_menu_command("mv 3"),
            MenuCommand::Move(NumberArg::Number(3))
        );
    }

    #[test]
    fn parses_edit_header_command() {
        assert_eq!(
            parse_menu_command("EH 5"),
            MenuCommand::EditHeader(NumberArg::Number(5))
        );
        assert_eq!(
            parse_menu_command("eh 5"),
            MenuCommand::EditHeader(NumberArg::Number(5))
        );
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

    #[test]
    fn parses_show_time_command() {
        // Tier A quickwin A1: a bare `T` (case-insensitive) routes to
        // `internalCommandT()` at `amiexpress/express.e:25622-25644`.
        assert_eq!(parse_menu_command("T"), MenuCommand::ShowTime);
        assert_eq!(parse_menu_command("t"), MenuCommand::ShowTime);
    }

    #[test]
    fn show_time_rejects_extra_tokens() {
        // `T anything` is not the show-time command — the legacy
        // command takes no arguments.
        assert_eq!(parse_menu_command("T 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("T now"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_show_version_command() {
        // Tier A quickwin A2: `VER` (case-insensitive) routes to
        // `internalCommandVER()` at `amiexpress/express.e:25688-25698`.
        assert_eq!(parse_menu_command("VER"), MenuCommand::ShowVersion);
        assert_eq!(parse_menu_command("ver"), MenuCommand::ShowVersion);
        assert_eq!(parse_menu_command("Ver"), MenuCommand::ShowVersion);
    }

    #[test]
    fn show_version_rejects_extra_tokens() {
        // `VER` takes no arguments in the legacy.
        assert_eq!(parse_menu_command("VER 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("VER full"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_show_help_command() {
        // Tier A quickwin A5: `H` (case-insensitive) routes to
        // `internalCommandH()` at `amiexpress/express.e:25071-25087`.
        assert_eq!(parse_menu_command("H"), MenuCommand::ShowHelp);
        assert_eq!(parse_menu_command("h"), MenuCommand::ShowHelp);
    }

    #[test]
    fn show_help_rejects_extra_tokens() {
        // The slice ships the no-arg form only. The legacy supported
        // an `NS` (non-stop) token that gets reintroduced by A12; for
        // now `H NS` falls through to Unknown so the future binding
        // is unambiguous.
        assert_eq!(parse_menu_command("H NS"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("H help"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_quiet_toggle_command() {
        // Tier A quickwin A9: `Q` (case-insensitive) routes to
        // `internalCommandQ()` at `amiexpress/express.e:25504-25516`.
        assert_eq!(parse_menu_command("Q"), MenuCommand::QuietToggle);
        assert_eq!(parse_menu_command("q"), MenuCommand::QuietToggle);
    }

    #[test]
    fn quiet_toggle_rejects_extra_tokens() {
        // The legacy command takes no arguments; trailing tokens fall
        // through to Unknown so the parser doesn't accidentally bind
        // future `Q*` two-letter commands.
        assert_eq!(parse_menu_command("Q 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("Q on"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_show_stats_command() {
        // Tier A quickwin A3: a bare `S` (case-insensitive) routes to
        // `internalCommandS()` at `amiexpress/express.e:25540-25608`.
        assert_eq!(parse_menu_command("S"), MenuCommand::ShowStats);
        assert_eq!(parse_menu_command("s"), MenuCommand::ShowStats);
    }

    #[test]
    fn show_stats_rejects_extra_tokens() {
        // The baseline `S` takes no arguments; trailing tokens fall
        // through to Unknown so the future `S` extended-report form
        // (A11) and any `S*` two-letter command stay unambiguous.
        assert_eq!(parse_menu_command("S 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("S full"), MenuCommand::Unknown);
    }

    #[test]
    fn show_version_does_not_collide_with_single_letter_v_commands() {
        // The future `V` (view file, Slice D-T6) and `VS` / `VO`
        // commands share a `V` prefix with `VER`. The current parser
        // only knows `VER`; bare `V` and `VO` must fall through to
        // Unknown so future slices can bind them without ambiguity.
        assert_eq!(parse_menu_command("V"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("VO"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("VS"), MenuCommand::Unknown);
    }

    #[test]
    fn checked_in_main_menu_advertises_only_implemented_commands() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate lives under repository root")
            .join("Conf02")
            .join("Menu5.txt");
        let Ok(menu) = std::fs::read_to_string(path) else {
            // cargo-mutants copies the Rust crate without the repo-root
            // screen assets. The ordinary test suite runs from the
            // checked-out repository and validates the asset there.
            return;
        };
        let advertised = advertised_commands(&menu);
        let expected = [
            "C", "E", "EH", "FW", "G", "H", "J", "K", "M", "MV", "N", "Q", "R", "RP", "S", "T",
            "VER",
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(advertised, expected);
        for command in advertised {
            assert_ne!(parse_menu_command(command), MenuCommand::Unknown);
        }
    }

    fn advertised_commands(menu: &str) -> std::collections::BTreeSet<&str> {
        menu.lines()
            .filter_map(|line| {
                if !line.starts_with("    ") {
                    return None;
                }
                let command = line.split_whitespace().next()?;
                command
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase())
                    .then_some(command)
            })
            .collect()
    }
}
