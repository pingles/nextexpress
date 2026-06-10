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
    /// `J` / `J <number>`: explicit conference join (Tier C C2).
    /// Mirrors `internalCommandJ()` at
    /// `amiexpress/express.e:25113-25183`.
    Join(JoinArg),
    /// `R` / `R <number>`: read one message.
    Read(NumberArg),
    /// `MS`: scan every conference the caller can access for mail
    /// (Tier B B1). Mirrors `internalCommandMS()` at
    /// `amiexpress/express.e:25250`.
    ScanAllMail,
    /// `E` / `E <to>`: enter a message.
    Post(PostArg),
    /// `C`: comment to sysop (Slice 44).
    CommentToSysop,
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
    /// `X`: toggle the user's expert mode (Tier A quickwin A6).
    /// Mirrors `internalCommandX()` at
    /// `amiexpress/express.e:26113-26121`.
    ExpertToggle,
    /// `?`: re-display the conference menu (Tier A quickwin A7).
    /// Mirrors `internalCommandQuestionMark()` at
    /// `amiexpress/express.e:24594-24599` — a no-op outside expert
    /// mode, where the menu loop has just displayed the menu anyway.
    ShowMenu,
    /// `^<topic>`: display the topic help screen (Tier A quickwin A10).
    /// Mirrors `internalCommandUpHat()` at
    /// `amiexpress/express.e:25089-25110`. The string is the topic name
    /// after the caret; an empty topic is a no-op.
    TopicHelp(String),
    /// `M`: toggle the session's ANSI colour output (Tier A quickwin
    /// A8). Mirrors `internalCommandM()` at
    /// `amiexpress/express.e:25239-25247`.
    AnsiToggle,
    /// `CF`: edit the caller's per-conference scan flags (Tier C C5).
    /// Mirrors `internalCommandCF()` at `amiexpress/express.e:24672`.
    ConferenceFlags,
    /// `<`: join the nearest lower-numbered accessible conference
    /// (Tier C C3). Mirrors `internalCommandLT()` at
    /// `amiexpress/express.e:24529-24546`. The legacy tokenizer keeps
    /// only the text before the first space as the command
    /// (`processCommand`, `amiexpress/express.e:28236-28244`) and the
    /// handler reads no parameters, so anything after the token is
    /// discarded.
    PrevConference,
    /// `>`: join the nearest higher-numbered accessible conference
    /// (Tier C C3). Mirrors `internalCommandGT()` at
    /// `amiexpress/express.e:24548-24564`; parameters are discarded
    /// exactly as for [`MenuCommand::PrevConference`].
    NextConference,
    /// Any command not recognised by this slice.
    Unknown,
}

/// Parsed numeric argument of the `R` command.
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

/// Parsed argument shape of the `J` command, using legacy `Val`
/// semantics (`amiexpress/express.e:25125-25136`): `parseParams`
/// splits the parameters on whitespace, the first token's numeric
/// prefix is the conference, and a `.`-suffix on that token or a
/// second token names a message base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinArg {
    /// `J <token>` where the token carries no `.` and no second token
    /// follows: the legacy `Val(param)` of the token
    /// (`amiexpress/express.e:25131`). May be zero (non-numeric
    /// token) or negative — range checks happen at dispatch, where
    /// anything outside `1..=numConf` opens the interactive prompt
    /// (`amiexpress/express.e:25142`).
    Conference(i64),
    /// Bare `J` — the legacy `newConf := -1` default
    /// (`amiexpress/express.e:25127`) always lands in the interactive
    /// prompt branch.
    Missing,
    /// `J <a>.<b>` or `J <a> <b>` — the conference + message-base
    /// argument forms (`amiexpress/express.e:25132-25135`).
    ///
    /// TODO(C4a): slice C4a completes these by parsing the
    /// conference and message-base values and joining the requested
    /// base. Interim for slice C2 they are routed into the
    /// conference-number prompt rather than joining silently.
    WithMsgBase,
}

/// Parses the numeric prefix of `token` with the semantics of the
/// Amiga E `Val()` built-in as used by `internalCommandJ`
/// (`amiexpress/express.e:25131`): an optional leading `-` sign
/// followed by decimal digits, stopping at the first non-digit
/// character. Returns 0 when the token carries no leading number
/// (e.g. `"abc"`, `""`, or a bare `"-"`). Saturates at the `i64`
/// bounds rather than reproducing the legacy 32-bit wraparound — the
/// caller clamps the value into `1..=numConf` anyway.
#[must_use]
pub(crate) fn val_prefix(token: &str) -> i64 {
    let (negative, rest) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit);
    if negative {
        digits.fold(0_i64, |acc, b| {
            acc.saturating_mul(10).saturating_sub(i64::from(b - b'0'))
        })
    } else {
        digits.fold(0_i64, |acc, b| {
            acc.saturating_mul(10).saturating_add(i64::from(b - b'0'))
        })
    }
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
    if trimmed.eq_ignore_ascii_case("CF") {
        return MenuCommand::ConferenceFlags;
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
    if trimmed.eq_ignore_ascii_case("X") {
        return MenuCommand::ExpertToggle;
    }
    if trimmed == "?" {
        return MenuCommand::ShowMenu;
    }
    if let Some(topic) = trimmed.strip_prefix('^') {
        return MenuCommand::TopicHelp(topic.trim().to_string());
    }
    if trimmed.eq_ignore_ascii_case("M") {
        return MenuCommand::AnsiToggle;
    }
    // `<` / `>` dispatch on the head token alone — the legacy
    // tokenizer keeps everything before the first space as the
    // command (`processCommand`, `amiexpress/express.e:28236-28244`)
    // and `internalCommandLT`/`GT` take no parameters. The match is
    // exact-token (`StrCmp`, `amiexpress/express.e:28322-28329`), so
    // `<<` / `>>` / `<2` never bind here.
    match trimmed.split_ascii_whitespace().next() {
        Some("<") => return MenuCommand::PrevConference,
        Some(">") => return MenuCommand::NextConference,
        _ => {}
    }
    if let Some(arg) = parse_join_command(trimmed) {
        return MenuCommand::Join(arg);
    }
    if let Some(arg) = parse_number_command(trimmed, "R") {
        return MenuCommand::Read(arg);
    }
    // `MS` (bare) is the multi-conference scan; an `eq_ignore_ascii_case`
    // on the whole line rejects `MS <n>` (which falls through to
    // Unknown) the same way the other no-argument commands do.
    if trimmed.eq_ignore_ascii_case("MS") {
        return MenuCommand::ScanAllMail;
    }
    if let Some(post) = parse_post_command(trimmed) {
        return MenuCommand::Post(post);
    }
    MenuCommand::Unknown
}

/// Parses the `J` command line into a [`JoinArg`]. Mirrors the legacy
/// `internalCommandJ` parameter handling
/// (`amiexpress/express.e:25125-25136`): whitespace-split tokens, the
/// first token's `Val` prefix selects the conference, and a `.`
/// inside the first token or the presence of a second token signals
/// the message-base forms (tokens past the second are ignored, as
/// `parseParams` only ever reads items 0 and 1 here).
fn parse_join_command(line: &str) -> Option<JoinArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("J") {
        return None;
    }
    let Some(first) = tokens.next() else {
        return Some(JoinArg::Missing);
    };
    if first.contains('.') || tokens.next().is_some() {
        return Some(JoinArg::WithMsgBase);
    }
    Some(JoinArg::Conference(val_prefix(first)))
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
        // Tier C C2: `J` uses legacy `Val` semantics
        // (`amiexpress/express.e:25127-25136`) — the conference is the
        // numeric prefix of the first token, and non-numeric tokens
        // `Val` to 0 rather than being rejected.
        assert_eq!(
            parse_menu_command("J 7"),
            MenuCommand::Join(JoinArg::Conference(7))
        );
        assert_eq!(
            parse_menu_command("j 7"),
            MenuCommand::Join(JoinArg::Conference(7))
        );
        assert_eq!(parse_menu_command("J"), MenuCommand::Join(JoinArg::Missing));
        assert_eq!(
            parse_menu_command("J nope"),
            MenuCommand::Join(JoinArg::Conference(0))
        );
        assert_eq!(
            parse_menu_command("J 2abc"),
            MenuCommand::Join(JoinArg::Conference(2))
        );
        assert_eq!(
            parse_menu_command("J -1"),
            MenuCommand::Join(JoinArg::Conference(-1))
        );
    }

    #[test]
    fn join_msgbase_argument_forms_parse_to_the_msgbase_shape() {
        // Dotted (`J 1.1`) and two-token (`J 1 2`) forms carry a
        // message-base request (`amiexpress/express.e:25132-25135`).
        // TODO(C4a): completed by slice C4a; interim they route into
        // the conference prompt.
        assert_eq!(
            parse_menu_command("J 1.1"),
            MenuCommand::Join(JoinArg::WithMsgBase)
        );
        assert_eq!(
            parse_menu_command("J 1 2"),
            MenuCommand::Join(JoinArg::WithMsgBase)
        );
        // Legacy parseParams ignores tokens past the second.
        assert_eq!(
            parse_menu_command("J 1 2 3"),
            MenuCommand::Join(JoinArg::WithMsgBase)
        );
    }

    #[test]
    fn val_prefix_parses_leading_digits_with_optional_minus_sign() {
        assert_eq!(val_prefix("2"), 2);
        assert_eq!(val_prefix("2abc"), 2);
        assert_eq!(val_prefix("2.1"), 2);
        assert_eq!(val_prefix("007"), 7);
        assert_eq!(val_prefix("-1"), -1);
        assert_eq!(val_prefix("-12x"), -12);
    }

    #[test]
    fn val_prefix_returns_zero_when_no_leading_number() {
        assert_eq!(val_prefix(""), 0);
        assert_eq!(val_prefix("abc"), 0);
        assert_eq!(val_prefix("-"), 0);
        // E's `Val` accepts a leading `-` only; `+` is not a sign.
        assert_eq!(val_prefix("+2"), 0);
        // No whitespace skipping — a leading space stops the parse,
        // so whitespace-only prompt input `Val`s to 0.
        assert_eq!(val_prefix(" 2"), 0);
        assert_eq!(val_prefix(".5"), 0);
    }

    #[test]
    fn val_prefix_saturates_instead_of_overflowing() {
        assert_eq!(val_prefix("99999999999999999999999"), i64::MAX);
        assert_eq!(val_prefix("-99999999999999999999999"), i64::MIN);
    }

    #[test]
    fn parses_prev_and_next_conference_commands() {
        // Tier C C3: `<` / `>` are the prev/next-accessible-conference
        // commands (`internalCommandLT`/`GT`,
        // `amiexpress/express.e:24529-24564`).
        assert_eq!(parse_menu_command("<"), MenuCommand::PrevConference);
        assert_eq!(parse_menu_command(">"), MenuCommand::NextConference);
    }

    #[test]
    fn prev_and_next_conference_discard_parameters() {
        // The legacy tokenizer keeps only the text before the first
        // space (`processCommand`, `amiexpress/express.e:28236-28244`)
        // and neither handler reads `cmdparams` — so any trailing
        // tokens are silently discarded.
        assert_eq!(parse_menu_command("< 2"), MenuCommand::PrevConference);
        assert_eq!(
            parse_menu_command("< anything at all"),
            MenuCommand::PrevConference
        );
        assert_eq!(parse_menu_command("> 2"), MenuCommand::NextConference);
        assert_eq!(parse_menu_command("> next"), MenuCommand::NextConference);
    }

    #[test]
    fn doubled_angle_brackets_stay_unknown_until_c4b() {
        // `<<` / `>>` are distinct whole tokens dispatched to
        // `internalCommandLT2`/`GT2` (`amiexpress/express.e:28324-28329`)
        // — slice C4b. Until it lands they must fall through to
        // Unknown, never prefix-match `<` / `>`.
        assert_eq!(parse_menu_command("<<"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command(">>"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("<< 2"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command(">> 2"), MenuCommand::Unknown);
    }

    #[test]
    fn angle_bracket_tokens_with_attached_text_are_unknown() {
        // The command token is everything before the first space, so
        // `<2` is a single unknown token (`StrCmp` dispatch is exact,
        // `amiexpress/express.e:28322-28329`), not `<` with a
        // parameter.
        assert_eq!(parse_menu_command("<2"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command(">2"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("<>"), MenuCommand::Unknown);
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
    fn parses_ms_as_the_multi_conference_scan() {
        // Tier B B1: `MS` is the multi-conference mail scan
        // (`MenuCommand::ScanAllMail`).
        assert_eq!(parse_menu_command("MS"), MenuCommand::ScanAllMail);
        assert_eq!(parse_menu_command("ms"), MenuCommand::ScanAllMail);
    }

    #[test]
    fn n_is_not_recognized_pending_the_tier_d_new_files_scan() {
        // Tier B B2: `N`'s mail-scan binding (a NextExpress drift —
        // legacy `N` is the new-files scan) is removed. Until Tier D
        // ships the new-files scan, `N` is an unknown command.
        assert_eq!(parse_menu_command("N"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("n"), MenuCommand::Unknown);
    }

    #[test]
    fn scan_commands_reject_extra_tokens() {
        assert_eq!(parse_menu_command("MS 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("N 7"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_ansi_toggle_command() {
        // Tier A quickwin A8: bare `M` (case-insensitive) is the ANSI
        // toggle, mirroring `internalCommandM()` at
        // `amiexpress/express.e:25239-25247`.
        assert_eq!(parse_menu_command("M"), MenuCommand::AnsiToggle);
        assert_eq!(parse_menu_command("m"), MenuCommand::AnsiToggle);
    }

    #[test]
    fn ansi_toggle_rejects_extra_tokens() {
        assert_eq!(parse_menu_command("M 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("M on"), MenuCommand::Unknown);
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
        assert_eq!(parse_menu_command("EM"), MenuCommand::Unknown);
    }

    #[test]
    fn retired_top_level_mail_shortcuts_are_unknown() {
        // Tier B B8: `RP` / `FW` / `K` / `MV` / `EH` were never legacy
        // menu commands — they are options inside the `R` sub-prompt and
        // were retired once it shipped. They now fall through to the
        // unknown-command notice.
        for token in ["RP 7", "rp 7", "FW 2", "K 2", "k 2", "MV 3", "EH 5", "eh 5"] {
            assert_eq!(
                parse_menu_command(token),
                MenuCommand::Unknown,
                "`{token}` must no longer parse to a top-level command",
            );
        }
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
    fn parses_cf_as_the_conference_flags_command() {
        // Tier C C5: `CF` (case-insensitive) is the conference-flags
        // editor (`internalCommandCF`, `amiexpress/express.e:24672`).
        assert_eq!(parse_menu_command("CF"), MenuCommand::ConferenceFlags);
        assert_eq!(parse_menu_command("cf"), MenuCommand::ConferenceFlags);
    }

    #[test]
    fn conference_flags_rejects_extra_tokens() {
        // The mask key and conference list are entered at the editor's
        // own prompts, not as command-line arguments.
        assert_eq!(parse_menu_command("CF 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("CF M"), MenuCommand::Unknown);
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
    fn parses_expert_toggle_command() {
        // Tier A quickwin A6: a bare `X` (case-insensitive) routes to
        // `internalCommandX()` at `amiexpress/express.e:26113-26121`.
        assert_eq!(parse_menu_command("X"), MenuCommand::ExpertToggle);
        assert_eq!(parse_menu_command("x"), MenuCommand::ExpertToggle);
    }

    #[test]
    fn expert_toggle_rejects_extra_tokens() {
        // The legacy command takes no arguments; trailing tokens fall
        // through to Unknown.
        assert_eq!(parse_menu_command("X 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("X on"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_show_menu_command() {
        // Tier A quickwin A7: a bare `?` routes to
        // `internalCommandQuestionMark()` at
        // `amiexpress/express.e:24594-24599`.
        assert_eq!(parse_menu_command("?"), MenuCommand::ShowMenu);
    }

    #[test]
    fn show_menu_rejects_extra_tokens() {
        // `?` takes no arguments; trailing tokens fall through to
        // Unknown.
        assert_eq!(parse_menu_command("? 1"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("? help"), MenuCommand::Unknown);
    }

    #[test]
    fn parses_topic_help_command() {
        // Tier A quickwin A10: `^<topic>` or `^ <topic>` routes to
        // `internalCommandUpHat()` at `amiexpress/express.e:25089`.
        // The topic is the text after the caret, trimmed.
        assert_eq!(
            parse_menu_command("^FILES"),
            MenuCommand::TopicHelp("FILES".to_string())
        );
        assert_eq!(
            parse_menu_command("^ files"),
            MenuCommand::TopicHelp("files".to_string())
        );
    }

    #[test]
    fn parses_bare_caret_as_empty_topic() {
        // A bare `^` carries no topic; the legacy returns immediately
        // without displaying anything.
        assert_eq!(
            parse_menu_command("^"),
            MenuCommand::TopicHelp(String::new())
        );
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

    /// The checked-in main menu (`Conf02/Menu5.txt`) must advertise
    /// **exactly** the set of menu commands the parser implements. The
    /// expected set is derived from [`advertised_token`] applied to
    /// every `MenuCommand` variant ([`every_menu_command`]), so adding
    /// a command fails this test — first to *compile*, because
    /// `advertised_token`'s match is exhaustive and demands a token for
    /// the new variant, and then on the assertion below until the menu
    /// asset lists it. Removing or renaming a command likewise fails
    /// until the menu drops or renames the entry.
    ///
    /// The advertised set is read back from the menu by taking the
    /// first whitespace token of each indented line and keeping the
    /// ones the parser recognises, so a stale entry (e.g. a command
    /// that no longer parses) is caught too.
    #[test]
    fn main_menu_advertises_exactly_the_implemented_commands() {
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

        let advertised: std::collections::BTreeSet<String> = menu
            .lines()
            .filter(|line| line.starts_with(' '))
            .filter_map(|line| line.split_whitespace().next())
            .filter(|token| parse_menu_command(token) != MenuCommand::Unknown)
            .map(str::to_string)
            .collect();

        let expected: std::collections::BTreeSet<String> = every_menu_command()
            .iter()
            .filter_map(advertised_token)
            .map(str::to_string)
            .collect();

        assert_eq!(
            advertised, expected,
            "Conf02/Menu5.txt must advertise exactly the implemented menu \
             commands — update the menu (and `advertised_token`) when adding \
             or removing a command",
        );
    }

    /// The menu token each `MenuCommand` is advertised under in the main
    /// menu, or `None` for commands that are not advertised
    /// (`Unknown`). The match is **exhaustive on purpose**: a new
    /// `MenuCommand` variant will not compile until it is given a token
    /// here, which makes
    /// [`main_menu_advertises_exactly_the_implemented_commands`] fail
    /// until `Conf02/Menu5.txt` lists it. Add the new variant to
    /// [`every_menu_command`] too so it is counted.
    fn advertised_token(command: &MenuCommand) -> Option<&'static str> {
        match command {
            MenuCommand::Logoff => Some("G"),
            MenuCommand::Join(_) => Some("J"),
            MenuCommand::Read(_) => Some("R"),
            MenuCommand::ScanAllMail => Some("MS"),
            MenuCommand::Post(_) => Some("E"),
            MenuCommand::CommentToSysop => Some("C"),
            MenuCommand::ShowTime => Some("T"),
            MenuCommand::ShowVersion => Some("VER"),
            MenuCommand::ShowHelp => Some("H"),
            MenuCommand::QuietToggle => Some("Q"),
            MenuCommand::ShowStats => Some("S"),
            MenuCommand::ExpertToggle => Some("X"),
            MenuCommand::ShowMenu => Some("?"),
            MenuCommand::TopicHelp(_) => Some("^"),
            MenuCommand::AnsiToggle => Some("M"),
            MenuCommand::ConferenceFlags => Some("CF"),
            MenuCommand::PrevConference => Some("<"),
            MenuCommand::NextConference => Some(">"),
            MenuCommand::Unknown => None,
        }
    }

    /// One sample of every `MenuCommand` variant, used to enumerate the
    /// implemented command set. Keep in sync with [`advertised_token`]
    /// (the compiler enforces that match; add the matching sample here).
    fn every_menu_command() -> Vec<MenuCommand> {
        vec![
            MenuCommand::Logoff,
            MenuCommand::Join(JoinArg::Missing),
            MenuCommand::Read(NumberArg::Missing),
            MenuCommand::ScanAllMail,
            MenuCommand::Post(PostArg::Missing),
            MenuCommand::CommentToSysop,
            MenuCommand::ShowTime,
            MenuCommand::ShowVersion,
            MenuCommand::ShowHelp,
            MenuCommand::QuietToggle,
            MenuCommand::ShowStats,
            MenuCommand::ExpertToggle,
            MenuCommand::ShowMenu,
            MenuCommand::TopicHelp(String::new()),
            MenuCommand::AnsiToggle,
            MenuCommand::ConferenceFlags,
            MenuCommand::PrevConference,
            MenuCommand::NextConference,
            MenuCommand::Unknown,
        ]
    }
}
