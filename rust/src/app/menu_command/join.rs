use super::{val_prefix, JoinArg, MenuCommand, MsgBaseArg};

/// Parses the `J` command line into a [`JoinArg`]. Mirrors the legacy
/// `internalCommandJ` parameter handling
/// (`amiexpress/express.e:25125-25136`): whitespace-split tokens, the
/// first token's `Val` prefix selects the conference, and a `.`
/// inside the first token or the presence of a second token signals
/// the message-base forms.
pub(super) fn parse_join_command(line: &str) -> Option<JoinArg> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("J") {
        return None;
    }
    let Some(first) = tokens.next() else {
        return Some(JoinArg::Missing);
    };
    Some(parse_join_params(first, tokens))
}

/// Parses `J`'s parameter tokens (`amiexpress/express.e:25130-25136`)
/// into a [`JoinArg`]. Shared with `JM`'s dotted-argument delegation.
fn parse_join_params<'a>(first: &str, mut rest: impl Iterator<Item = &'a str>) -> JoinArg {
    let conference = val_prefix(first);
    if let Some(dot) = first.find('.') {
        return JoinArg::WithMsgBase {
            conference,
            msgbase: val_prefix(&first[dot + 1..]),
        };
    }
    if let Some(second) = rest.next() {
        return JoinArg::WithMsgBase {
            conference,
            msgbase: val_prefix(second),
        };
    }
    JoinArg::Conference(conference)
}

/// Parses the `JM` command line (`internalCommandJM` parameter
/// handling, `amiexpress/express.e:25197-25208`). A `.` anywhere in
/// the first token delegates the raw params to the `J` logic.
pub(super) fn parse_join_msgbase_command(line: &str) -> Option<MenuCommand> {
    let mut tokens = line.split_ascii_whitespace();
    let head = tokens.next()?;
    if !head.eq_ignore_ascii_case("JM") {
        return None;
    }
    let Some(first) = tokens.next() else {
        return Some(MenuCommand::JoinMsgBase(MsgBaseArg::Missing));
    };
    if first.contains('.') {
        return Some(MenuCommand::Join(parse_join_params(first, tokens)));
    }
    Some(MenuCommand::JoinMsgBase(MsgBaseArg::Base(val_prefix(
        first,
    ))))
}
