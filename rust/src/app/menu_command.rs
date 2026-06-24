//! Parser for menu command lines.
//!
//! The parser is deliberately effect-free: it turns a raw terminal
//! line into a typed command shape and leaves all session, repository,
//! and terminal effects to [`crate::app::menu_flow::MenuFlow`].

mod files;
mod join;
mod mail;

/// Parsed menu command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MenuCommand {
    /// `G` / `G Y`: user requested logoff. `auto` is set by the `Y`
    /// param (`amiexpress/express.e:25049`, `paramsContains('Y')`),
    /// which forces logoff straight past the flagged-file confirm.
    Logoff { auto: bool },
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
    /// `JM` / `JM <number>`: join a message base of the current
    /// conference (Tier C C4a). Mirrors `internalCommandJM()` at
    /// `amiexpress/express.e:25185-25237`. A `.`-dotted first token
    /// never reaches this variant — the legacy hands the raw params
    /// to `internalCommandJ` (`:25203-25205`), which the parser
    /// mirrors by producing [`MenuCommand::Join`] directly.
    JoinMsgBase(MsgBaseArg),
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
    /// `<<`: join the previous message base of the current conference
    /// (Tier C C4b). Mirrors `internalCommandLT2()` at
    /// `amiexpress/express.e:24566-24578`. A distinct dispatch-table
    /// token from `<` (`StrCmp`, `amiexpress/express.e:28324`); the
    /// handler reads no parameters, so anything after the token is
    /// discarded.
    PrevMsgBase,
    /// `>>`: join the next message base of the current conference
    /// (Tier C C4b). Mirrors `internalCommandGT2()` at
    /// `amiexpress/express.e:24580-24592`; parameters are discarded
    /// exactly as for [`MenuCommand::PrevMsgBase`].
    NextMsgBase,
    /// `F …`: file listings via the `NextScan` lister (slice D2). The
    /// parity target is the `AquaScan` v1.0 door the stock deployment
    /// installs over `F` (`comparison/evidence-tierD/live-observations.md`);
    /// the shadowed internal is `internalCommandF`
    /// (`amiexpress/express.e:24877`), kept for the stock diff record.
    FileList(FileListArg),
    /// `Z` / `Z <token>`: zippy text search of file descriptions (slice
    /// D4). Mirrors `internalCommandZ` at `amiexpress/express.e:26123`.
    /// Unlike `F`/`FR`/`N`, `Z` is *not* shadowed by the `AquaScan`
    /// door (it is absent from the door's icon set), so the parity
    /// target is the genuine internal command — captured live in
    /// `comparison/transcripts/ae_tierd_zippy.txt`.
    ZippySearch(ZippyArg),
    /// Any command not recognised by this slice.
    Unknown,
}

/// Parsed argument shape of the `Z` command. Legacy `parseParams` reads
/// the search string from `item(0)` — a single whitespace token
/// (`amiexpress/express.e:26146`); the directory span in `item(1)` is
/// deferred to slice D7, so D4 carries only the query (or the prompt
/// marker for bare `Z`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ZippyArg {
    /// `Z <token>`: the `item(0)` search string, kept verbatim — the
    /// handler upper-cases it for matching
    /// (`amiexpress/express.e:26160`). No `item(1)`, so the handler
    /// opens the `getDirSpan('')` directory prompt.
    Query(String),
    /// `Z <token> <span>`: the `item(0)` query plus the `item(1)`
    /// directory span, supplied inline. The handler resolves the span
    /// directly — `getDirSpan(item(1))` (`amiexpress/express.e:26162-26163`,
    /// `:26875-26877`) — and scans **without** the directory prompt
    /// (slice D7, captured in `comparison/transcripts/ae_tierd_zippy3.txt`).
    QueryInDir {
        /// The `item(0)` search string, kept verbatim.
        query: String,
        /// The raw `item(1)` directory-span token (`number` / `A` / `U`
        /// / `H`), resolved by the handler with the same `getDirSpan`
        /// logic the prompt answer uses.
        span: String,
    },
    /// Bare `Z`: the prompt form — the handler emits `Enter string to
    /// search for:` and line-reads the query
    /// (`amiexpress/express.e:26150`).
    Prompt,
}

/// Parsed argument shape of the `F` command, mirroring the captured
/// `AquaScan` grammar (`F ?` help, `ae_tierd_aquascan3.txt` S1):
/// `F [R] dir [Q] [NS]` with dir = `U` | `A` | number | `H`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileListArg {
    /// Bare `F` / bare `FR`: open the door's own `Directories: …`
    /// prompt (`express.e:27645-27648` → `getDirSpan('')`). `reverse`
    /// (set by `FR`) reverse-walks whichever span the caller picks.
    Prompt { reverse: bool },
    /// `F ?`: show the `NextScan` help screen.
    Help,
    /// `F <dir> [NS]` / `FR <dir> [NS]`: scan immediately, optionally
    /// without pausing, optionally in reverse chronological order.
    Span {
        /// Which directories to scan.
        span: FileSpan,
        /// `NS` token present — non-stop scrolling, no pager.
        non_stop: bool,
        /// `FR` (the reverse token) — list newest-first and, for
        /// multi-dir spans, walk the directories highest→lowest
        /// (`amiexpress/express.e:27654`).
        reverse: bool,
    },
    /// Any other argument form — the captured
    /// `Argument error! Type 'f ?' for help.` path
    /// (`ae_tierd_aquascan4.txt` U4). Includes the unported tokens:
    /// `Q` (quick scan — capture first) and `W` (door
    /// self-configuration — `NextExpress` config is TOML, a permanent
    /// departure). `F R` with a space also lands here — the original
    /// dispatch matches the whole `FR` token (`express.e:28310`), so a
    /// space-separated `R` is not a reverse form.
    Invalid,
}

/// The directory selection of an `F` scan (captured grammar: dir =
/// `U`pload | `A`ll | number | `H`old).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileSpan {
    /// `F <n>` — one directory by number, carrying the raw `Val`
    /// result; the dispatch range-checks into `1..=areas` and answers
    /// out-of-range with the highest-dir error
    /// (`ae_tierd_aquascan.txt` A7).
    Dir(i64),
    /// `F A` — all directories, first to last.
    All,
    /// `F U` — the upload directory (the highest-numbered area;
    /// confirmed both as argument and prompt answer,
    /// `ae_tierd_aquascan.txt` A8 / `ae_tierd_aquascan4.txt` U6).
    Upload,
    /// `F H` — the hold directory (held-for-review files).
    Hold,
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
    /// argument forms (`amiexpress/express.e:25130-25136`): `Val` of
    /// the first token is the conference (it stops at the `.`), and
    /// the text after the first `.` — or, failing that, the second
    /// whitespace token — `Val`s to the message base. Both values
    /// carry raw `Val` results; range checks happen at dispatch.
    WithMsgBase {
        /// Requested conference number (`amiexpress/express.e:25131`).
        conference: i64,
        /// Requested message-base number within that conference
        /// (`amiexpress/express.e:25133` / `:25135`).
        msgbase: i64,
    },
}

/// Parsed argument shape of the `JM` command, using legacy `Val`
/// semantics (`amiexpress/express.e:25199-25208`): only the first
/// whitespace token is read; further tokens are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MsgBaseArg {
    /// `JM <token>` — the legacy `Val(param)` of the first token
    /// (`amiexpress/express.e:25208`). May be zero (non-numeric
    /// token) or negative; anything outside the current conference's
    /// base range is handled at dispatch
    /// (`amiexpress/express.e:25220`).
    Base(i64),
    /// Bare `JM` — the legacy `newMsgBase := -1` default
    /// (`amiexpress/express.e:25199`).
    Missing,
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
    // `G` logs off; `G Y` forces logoff straight past the flagged-file
    // confirm (`amiexpress/express.e:25049`, `paramsContains('Y')`).
    // Any other `G <args>` behaves like plain `G`.
    let mut g_parts = trimmed.splitn(2, char::is_whitespace);
    if g_parts
        .next()
        .is_some_and(|head| head.eq_ignore_ascii_case("G"))
    {
        let auto = g_parts
            .next()
            .is_some_and(|rest| rest.split_whitespace().any(|t| t.eq_ignore_ascii_case("Y")));
        return MenuCommand::Logoff { auto };
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
    // `<` / `>` / `<<` / `>>` dispatch on the head token alone — the
    // legacy tokenizer keeps everything before the first space as the
    // command (`processCommand`, `amiexpress/express.e:28236-28244`)
    // and none of `internalCommandLT`/`GT`/`LT2`/`GT2` take
    // parameters. The match is exact-token (`StrCmp`,
    // `amiexpress/express.e:28322-28329`), so `<<<` / `<2` / `<>`
    // never bind here.
    match trimmed.split_ascii_whitespace().next() {
        Some("<") => return MenuCommand::PrevConference,
        Some(">") => return MenuCommand::NextConference,
        Some("<<") => return MenuCommand::PrevMsgBase,
        Some(">>") => return MenuCommand::NextMsgBase,
        _ => {}
    }
    if let Some(arg) = join::parse_join_command(trimmed) {
        return MenuCommand::Join(arg);
    }
    if let Some(command) = join::parse_join_msgbase_command(trimmed) {
        return command;
    }
    if let Some(arg) = mail::parse_number_command(trimmed, "R") {
        return MenuCommand::Read(arg);
    }
    // `MS` (bare) is the multi-conference scan; an `eq_ignore_ascii_case`
    // on the whole line rejects `MS <n>` (which falls through to
    // Unknown) the same way the other no-argument commands do.
    if trimmed.eq_ignore_ascii_case("MS") {
        return MenuCommand::ScanAllMail;
    }
    if let Some(post) = mail::parse_post_command(trimmed) {
        return MenuCommand::Post(post);
    }
    if let Some(arg) = files::parse_file_list_command(trimmed) {
        return MenuCommand::FileList(arg);
    }
    if let Some(arg) = files::parse_zippy_command(trimmed) {
        return MenuCommand::ZippySearch(arg);
    }
    MenuCommand::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_logoff_command() {
        assert_eq!(parse_menu_command("G"), MenuCommand::Logoff { auto: false });
        assert_eq!(parse_menu_command("g"), MenuCommand::Logoff { auto: false });
    }

    #[test]
    fn g_with_y_param_is_a_forced_logoff() {
        // `G Y` skips the flagged-file confirm (express.e:25049); the
        // `Y` is matched case-insensitively, like the rest of the menu.
        assert_eq!(
            parse_menu_command("G Y"),
            MenuCommand::Logoff { auto: true }
        );
        assert_eq!(
            parse_menu_command("g y"),
            MenuCommand::Logoff { auto: true }
        );
        // A non-`Y` argument is still a plain (confirming) logoff.
        assert_eq!(
            parse_menu_command("G N"),
            MenuCommand::Logoff { auto: false }
        );
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
    fn join_msgbase_argument_forms_carry_conference_and_msgbase() {
        // Tier C C4a: the dotted (`J 1.1`) and two-token (`J 1 2`)
        // forms carry a message-base request
        // (`amiexpress/express.e:25130-25136`) — `Val` of the first
        // token is the conference (it stops at the `.`), the text
        // after the first `.` (or a second token) `Val`s to the
        // message base.
        assert_eq!(
            parse_menu_command("J 1.1"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 1,
            })
        );
        assert_eq!(
            parse_menu_command("J 2.1"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 2,
                msgbase: 1,
            })
        );
        assert_eq!(
            parse_menu_command("J 1 2"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 2,
            })
        );
        // Legacy parseParams reads only items 0 and 1 here: `J 1 2 3`
        // is conference 1, message base 2; the `3` is discarded.
        assert_eq!(
            parse_menu_command("J 1 2 3"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 2,
            })
        );
        // The dot is found first (`InStr`, `amiexpress/express.e:25132`),
        // so the text after it wins over a second token, and only the
        // text up to the *next* non-digit feeds `Val`: `J 1.2.3` is
        // conference 1, message base 2.
        assert_eq!(
            parse_menu_command("J 1.2.3 9"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 2,
            })
        );
        // `J 2.` → `Val('') = 0`: an explicit but out-of-range base.
        assert_eq!(
            parse_menu_command("J 2."),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 2,
                msgbase: 0,
            })
        );
    }

    #[test]
    fn parses_jm_command_argument_forms() {
        // Tier C C4a: `JM` takes the `Val` of its first token as the
        // message-base number (`amiexpress/express.e:25199-25208`);
        // extra tokens are ignored (only item 0 is read), and a bare
        // `JM` is the legacy `newMsgBase := -1` missing marker.
        assert_eq!(
            parse_menu_command("JM 2"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Base(2))
        );
        assert_eq!(
            parse_menu_command("jm 2"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Base(2))
        );
        assert_eq!(
            parse_menu_command("JM"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Missing)
        );
        // Non-numeric `Val`s to 0 (out of range, never rejected).
        assert_eq!(
            parse_menu_command("JM abc"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Base(0))
        );
        // `Val` prefix semantics, as for `J`.
        assert_eq!(
            parse_menu_command("JM 2abc"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Base(2))
        );
        // Tokens past the first are ignored: `JM 1 2` is base 1.
        assert_eq!(
            parse_menu_command("JM 1 2"),
            MenuCommand::JoinMsgBase(MsgBaseArg::Base(1))
        );
    }

    #[test]
    fn jm_dotted_argument_delegates_to_the_j_command() {
        // A `.` anywhere in JM's first token hands the *raw params*
        // to `internalCommandJ` (`amiexpress/express.e:25203-25205`)
        // — observed live: `JM 1.1` joins conference 1, identical to
        // `J 1.1`. The parser routes the dotted form straight to the
        // `Join` command shape.
        assert_eq!(
            parse_menu_command("JM 1.1"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 1,
            })
        );
        assert_eq!(
            parse_menu_command("jm 1.1"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 1,
            })
        );
        // J re-parses the raw params, so its two-token rule applies
        // after delegation too: `JM 1.1 5` is conference 1, base 1
        // (the dot wins, `amiexpress/express.e:25132-25135`).
        assert_eq!(
            parse_menu_command("JM 1.1 5"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 1,
                msgbase: 1,
            })
        );
        // `JM .5` → J parses `.5`: conference `Val('.5') = 0`, base 5.
        assert_eq!(
            parse_menu_command("JM .5"),
            MenuCommand::Join(JoinArg::WithMsgBase {
                conference: 0,
                msgbase: 5,
            })
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
    fn parses_prev_and_next_msgbase_commands() {
        // Tier C C4b: `<<` / `>>` are the prev/next-message-base
        // commands (`internalCommandLT2`/`GT2`,
        // `amiexpress/express.e:24566-24592`), dispatched as distinct
        // whole tokens (`StrCmp`, `amiexpress/express.e:28324-28329`)
        // — they never prefix-match `<` / `>`.
        assert_eq!(parse_menu_command("<<"), MenuCommand::PrevMsgBase);
        assert_eq!(parse_menu_command(">>"), MenuCommand::NextMsgBase);
    }

    #[test]
    fn prev_and_next_msgbase_discard_parameters() {
        // The legacy tokenizer keeps only the text before the first
        // space (`processCommand`, `amiexpress/express.e:28236-28244`)
        // and neither `internalCommandLT2` nor `GT2` reads
        // `cmdparams` — trailing tokens are silently discarded.
        assert_eq!(parse_menu_command("<< 2"), MenuCommand::PrevMsgBase);
        assert_eq!(
            parse_menu_command("<< anything at all"),
            MenuCommand::PrevMsgBase
        );
        assert_eq!(parse_menu_command(">> 2"), MenuCommand::NextMsgBase);
        assert_eq!(parse_menu_command(">> next"), MenuCommand::NextMsgBase);
    }

    #[test]
    fn longer_angle_bracket_runs_are_unknown() {
        // Exact-token dispatch: `<<<` is a single token matching
        // neither `<` nor `<<` (`amiexpress/express.e:28322-28329`).
        assert_eq!(parse_menu_command("<<<"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command(">>>"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("<<2"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command(">>2"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("<<>"), MenuCommand::Unknown);
    }

    #[test]
    fn single_and_double_angle_brackets_stay_distinct() {
        // `<` hops conferences, `<<` hops message bases — the two
        // dispatch-table entries are independent
        // (`amiexpress/express.e:28322-28325`).
        assert_eq!(parse_menu_command("<"), MenuCommand::PrevConference);
        assert_eq!(parse_menu_command("<<"), MenuCommand::PrevMsgBase);
        assert_eq!(parse_menu_command(">"), MenuCommand::NextConference);
        assert_eq!(parse_menu_command(">>"), MenuCommand::NextMsgBase);
        // `< <` is token `<` with a discarded parameter, not `<<`.
        assert_eq!(parse_menu_command("< <"), MenuCommand::PrevConference);
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
    #[test]
    fn f_command_parses_the_nextscan_grammar() {
        // The captured AquaScan syntax (`F ?` help, aquascan3.txt S1):
        // `F [R] dir [Q] [NS]` with dir = U | A | x | H. D2 ships the
        // un-R/Q forms; bare F prompts and `F ?` shows the help.
        assert_eq!(
            parse_menu_command("F"),
            MenuCommand::FileList(FileListArg::Prompt { reverse: false })
        );
        assert_eq!(
            parse_menu_command("F ?"),
            MenuCommand::FileList(FileListArg::Help)
        );
        assert_eq!(
            parse_menu_command("f 1"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: false,
            })
        );
        assert_eq!(
            parse_menu_command("F A"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::All,
                non_stop: false,
                reverse: false,
            })
        );
        assert_eq!(
            parse_menu_command("F u"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Upload,
                non_stop: false,
                reverse: false,
            })
        );
        assert_eq!(
            parse_menu_command("F H"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Hold,
                non_stop: false,
                reverse: false,
            })
        );
        assert_eq!(
            parse_menu_command("F 1 NS"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: true,
                reverse: false,
            })
        );
        assert_eq!(
            parse_menu_command("F A ns"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::All,
                non_stop: true,
                reverse: false,
            })
        );
        // Raw Val carry: range checks happen at dispatch, where 0 (and
        // anything past the area count) takes the highest-dir error.
        assert_eq!(
            parse_menu_command("F 0"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Dir(0),
                non_stop: false,
                reverse: false,
            })
        );
    }

    #[test]
    fn f_command_rejects_unsupported_argument_forms() {
        // Each takes the captured `Argument error! Type 'f ?' for
        // help.` path (aquascan4.txt U4). `F R` (with a space) stays
        // here — `FR` is the reverse token (slice D3); the original
        // dispatch matches the whole code (`express.e:28310`). `F W` is
        // permanent (config is TOML); `Q` waits for a quick-scan capture.
        for line in ["F R 1", "F W", "F XYZ", "F ? extra", "F 1 XYZ", "F 1 NS x"] {
            assert_eq!(
                parse_menu_command(line),
                MenuCommand::FileList(FileListArg::Invalid),
                "line {line:?} must parse as Invalid",
            );
        }
    }

    #[test]
    fn fr_command_parses_the_reverse_grammar() {
        // `FR` is the concatenated reverse token (`express.e:28310`
        // dispatches the whole code `FR`). It mirrors `F`'s span
        // grammar with `reverse: true`. Bare `FR`, like bare `F`,
        // opens the `Directories:` prompt (`express.e:27645-27648` →
        // `getDirSpan('')`) — we follow the original here over the
        // AquaScan capture, which skips the prompt for `FR`.
        assert_eq!(
            parse_menu_command("FR"),
            MenuCommand::FileList(FileListArg::Prompt { reverse: true })
        );
        assert_eq!(
            parse_menu_command("fr 1"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: false,
                reverse: true,
            })
        );
        assert_eq!(
            parse_menu_command("FR A"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::All,
                non_stop: false,
                reverse: true,
            })
        );
        assert_eq!(
            parse_menu_command("FR u"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Upload,
                non_stop: false,
                reverse: true,
            })
        );
        assert_eq!(
            parse_menu_command("FR H"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Hold,
                non_stop: false,
                reverse: true,
            })
        );
        assert_eq!(
            parse_menu_command("FR 1 NS"),
            MenuCommand::FileList(FileListArg::Span {
                span: FileSpan::Dir(1),
                non_stop: true,
                reverse: true,
            })
        );
        assert_eq!(
            parse_menu_command("FR ?"),
            MenuCommand::FileList(FileListArg::Help)
        );
    }

    #[test]
    fn parses_zippy_search_command() {
        // Slice D4: `Z` is the zippy text search (`internalCommandZ`,
        // `amiexpress/express.e:26123`). Legacy `parseParams` reads the
        // search string from `item(0)` — a single whitespace token
        // (`:26146`) — so `Z <token>` carries that one token as the
        // query, and bare `Z` is the prompt form (`:26150`, the
        // `Enter string to search for:` line input).
        assert_eq!(
            parse_menu_command("Z STARVIEW"),
            MenuCommand::ZippySearch(ZippyArg::Query("STARVIEW".to_string()))
        );
        assert_eq!(
            parse_menu_command("z starview"),
            MenuCommand::ZippySearch(ZippyArg::Query("starview".to_string()))
        );
        assert_eq!(
            parse_menu_command("Z"),
            MenuCommand::ZippySearch(ZippyArg::Prompt)
        );
        assert_eq!(
            parse_menu_command("z"),
            MenuCommand::ZippySearch(ZippyArg::Prompt)
        );
    }

    #[test]
    fn zippy_search_query_then_optional_inline_directory_span() {
        // `parseParams` puts the search string in `item(0)` and the
        // directory span in `item(1)` (`amiexpress/express.e:26146`,
        // `:26162-26163`). A bare `Z <token>` carries the query and
        // leaves the directory to the prompt; a second token is the
        // inline area-spec, which the handler resolves without prompting
        // (slice D7, `getDirSpan(item(1))`). Tokens past `item(1)` are
        // dropped (`parseParams` reads only items 0 and 1 here).
        assert_eq!(
            parse_menu_command("Z STARVIEW 1"),
            MenuCommand::ZippySearch(ZippyArg::QueryInDir {
                query: "STARVIEW".to_string(),
                span: "1".to_string(),
            })
        );
        assert_eq!(
            parse_menu_command("Z foo A NS"),
            MenuCommand::ZippySearch(ZippyArg::QueryInDir {
                query: "foo".to_string(),
                span: "A".to_string(),
            })
        );
        // `Z <token>` with no second token stays the prompt-for-dir form.
        assert_eq!(
            parse_menu_command("Z foo"),
            MenuCommand::ZippySearch(ZippyArg::Query("foo".to_string()))
        );
    }

    #[test]
    fn zippy_search_does_not_swallow_the_zoom_command() {
        // Exact-token dispatch (`StrCmp(cmdcode,'Z')`,
        // `amiexpress/express.e:28388`): `Z` binds only when the command
        // token is exactly `Z`. `ZOOM` (`internalCommandZOOM`,
        // `:26215`) is a distinct token reserved for a later slice, so
        // it must stay Unknown rather than parse as a zippy search.
        assert_eq!(parse_menu_command("ZOOM"), MenuCommand::Unknown);
        assert_eq!(parse_menu_command("zoom"), MenuCommand::Unknown);
    }

    #[test]
    fn fr_command_rejects_unsupported_argument_forms() {
        // Same `Argument error!` path as `F`. `FR W` (config is TOML)
        // and junk tokens are unsupported; `F R` (with a space) is
        // *not* an original reverse form (`express.e` matches the whole
        // `FR` token), so it stays the `F`-with-junk Invalid path.
        for line in ["FR W", "FR XYZ", "FR ? extra", "FR 1 XYZ"] {
            assert_eq!(
                parse_menu_command(line),
                MenuCommand::FileList(FileListArg::Invalid),
                "line {line:?} must parse as Invalid",
            );
        }
    }

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
            // Command rows are indented exactly four spaces; section
            // headers sit at two and the banner art at six, so the
            // four-space indent reads back *every* advertised command
            // token — including a stale one that no longer parses. The
            // previous `parse_menu_command != Unknown` filter silently
            // dropped unparseable tokens, which is exactly how the
            // retired `RP`/`FW`/`K`/`MV`/`EH` rows lingered in the menu
            // long after Tier B B8 removed them from the dispatcher.
            .filter(|line| line.bytes().take_while(|&b| b == b' ').count() == 4)
            .filter_map(|line| line.split_whitespace().next())
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
            MenuCommand::Logoff { .. } => Some("G"),
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
            MenuCommand::JoinMsgBase(_) => Some("JM"),
            MenuCommand::PrevConference => Some("<"),
            MenuCommand::NextConference => Some(">"),
            MenuCommand::PrevMsgBase => Some("<<"),
            MenuCommand::NextMsgBase => Some(">>"),
            MenuCommand::FileList(_) => Some("F"),
            MenuCommand::ZippySearch(_) => Some("Z"),
            MenuCommand::Unknown => None,
        }
    }

    /// One sample of every `MenuCommand` variant, used to enumerate the
    /// implemented command set. Keep in sync with [`advertised_token`]
    /// (the compiler enforces that match; add the matching sample here).
    fn every_menu_command() -> Vec<MenuCommand> {
        vec![
            MenuCommand::Logoff { auto: false },
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
            MenuCommand::JoinMsgBase(MsgBaseArg::Missing),
            MenuCommand::PrevConference,
            MenuCommand::NextConference,
            MenuCommand::PrevMsgBase,
            MenuCommand::NextMsgBase,
            MenuCommand::FileList(FileListArg::Prompt { reverse: false }),
            MenuCommand::ZippySearch(ZippyArg::Prompt),
            MenuCommand::Unknown,
        ]
    }
}
