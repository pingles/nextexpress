use super::{parse_span_token, FileListArg, ZippyArg};

/// Parses the `Z` command line into a [`ZippyArg`]. Exact-token dispatch
/// (`StrCmp(cmdcode,'Z')`, `amiexpress/express.e:28388`): the command
/// binds only when the first whitespace token is exactly `Z`, so `ZOOM`
/// (`internalCommandZOOM`, `:26215`) stays a separate token reserved for
/// a later slice. The search string is the first parameter token
/// (`item(0)`, `:26146`); a bare `Z` is the prompt form (`:26150`).
pub(super) fn parse_zippy_command(line: &str) -> Option<ZippyArg> {
    let mut tokens = super::command_tokens(line, "Z")?;
    let Some(query) = tokens.next() else {
        return Some(ZippyArg::Prompt);
    };
    Some(match tokens.next() {
        Some(span) => ZippyArg::QueryInDir {
            query: query.to_string(),
            span: span.to_string(),
        },
        None => ZippyArg::Query(query.to_string()),
    })
}

/// Parses the `F` / `FR` command line into a [`FileListArg`] following
/// the captured `AquaScan` grammar (dir = `U` | `A` | number | `H`,
/// optional trailing `NS`). Numeric directory tokens carry their
/// [`val_prefix`] result raw; range checks happen at dispatch.
pub(super) fn parse_file_list_command(line: &str) -> Option<FileListArg> {
    let mut tokens = line.split_ascii_whitespace();
    let command = tokens.next()?;
    let reverse = if command.eq_ignore_ascii_case("F") {
        false
    } else if command.eq_ignore_ascii_case("FR") {
        true
    } else {
        return None;
    };
    let Some(first) = tokens.next() else {
        return Some(FileListArg::Prompt { reverse });
    };
    if first == "?" {
        return Some(if tokens.next().is_none() {
            FileListArg::Help
        } else {
            FileListArg::Invalid
        });
    }

    let Some(span) = parse_span_token(first) else {
        return Some(FileListArg::Invalid);
    };

    let non_stop = match tokens.next() {
        None => false,
        Some(token) if token.eq_ignore_ascii_case("NS") && tokens.next().is_none() => true,
        Some(_) => return Some(FileListArg::Invalid),
    };
    Some(FileListArg::Span {
        span,
        non_stop,
        reverse,
    })
}
