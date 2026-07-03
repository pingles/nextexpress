use super::{
    parse_span_token, FileListArg, FileSpan, NewFilesArg, NewFilesSpec, ScanRequest, ZippyArg,
};

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

/// Parses the `N` command line into a [`NewFilesArg`] following the
/// captured `AquaScan` grammar
/// (`comparison/transcripts/ae_tierd_newfiles.txt` N6/N7):
/// `N [S|mm-dd[-yy]|T|Y|-x|!x|R] [dir] [Q] [NS]`. Exact-head dispatch
/// via [`super::command_tokens`], so the `NS`/`NSU` door siblings
/// (future slices) never bind here.
pub(super) fn parse_new_files_command(line: &str) -> Option<NewFilesArg> {
    let mut tokens = super::command_tokens(line, "N")?;
    let Some(first) = tokens.next() else {
        return Some(NewFilesArg::Prompt);
    };
    if first == "?" {
        return Some(if tokens.next().is_none() {
            NewFilesArg::Help
        } else {
            NewFilesArg::Invalid
        });
    }

    // First-token classification (the N6 grammar). Date shapes are
    // checked before the bare-dir reading so `01-01-26` is never read
    // as dir 1; an all-digit token is a dir (`N 2`, N7d) and consumes
    // the dir slot, while dashed-but-malformed shapes (`01-011-26`)
    // fall through to Invalid rather than reading as dir 1.
    let mut span: Option<FileSpan> = None;
    let request = if first.eq_ignore_ascii_case("S") {
        ScanRequest::SinceLastCall
    } else if first.eq_ignore_ascii_case("T") {
        ScanRequest::Today
    } else if first.eq_ignore_ascii_case("Y") {
        ScanRequest::Yesterday
    } else if first.eq_ignore_ascii_case("R") {
        ScanRequest::Reverse
    } else if let Some(days) = parse_days_back(first) {
        ScanRequest::DaysBack(days)
    } else if let Some(count) = parse_newest_count(first) {
        ScanRequest::NewestLast(count)
    } else if let Some(date) = parse_date_token(first) {
        date
    } else if first.bytes().all(|b| b.is_ascii_digit()) {
        span = parse_span_token(first);
        ScanRequest::SinceLastCall
    } else {
        // `N W` and every junk form — the captured Argument-error
        // envelope (the `F W` precedent; N7e).
        return Some(NewFilesArg::Invalid);
    };

    // Optional `[dir] [Q] [NS]`, in help order (N6's diagram).
    let mut quick = false;
    let mut non_stop = false;
    let mut next = tokens.next();
    if span.is_none() {
        if let Some(token) = next {
            if !token.eq_ignore_ascii_case("Q") && !token.eq_ignore_ascii_case("NS") {
                // Captured: `N R -1` → Argument error (N7e). A `None`
                // here must yield Invalid, never fall through to
                // Unknown — `N` has already bound.
                let Some(resolved) = parse_span_token(token) else {
                    return Some(NewFilesArg::Invalid);
                };
                span = Some(resolved);
                next = tokens.next();
            }
        }
    }
    if let Some(token) = next {
        if token.eq_ignore_ascii_case("Q") {
            quick = true;
            next = tokens.next();
        }
    }
    if let Some(token) = next {
        if token.eq_ignore_ascii_case("NS") {
            non_stop = true;
            next = tokens.next();
        }
    }
    if next.is_some() {
        return Some(NewFilesArg::Invalid);
    }
    Some(NewFilesArg::Scan(NewFilesSpec {
        request,
        span,
        quick,
        non_stop,
    }))
}

/// `-x`: a `-` followed by one or more digits — `N`'s days-back token.
/// Shared with the date-prompt answer parser (`file_list/new_files`).
pub(crate) fn parse_days_back(token: &str) -> Option<u32> {
    let digits = token.strip_prefix('-')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// `!x`: a `!` followed by one or more digits — `N`'s newest-x token.
fn parse_newest_count(token: &str) -> Option<u32> {
    let digits = token.strip_prefix('!')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// `mm-dd[-yy]`: two or three dash-separated 1–2-digit groups — the
/// help-advertised date shape (`ae_tierd_newfiles.txt` N6). Calendar
/// validity is the resolver's concern; the shape alone decides
/// date-vs-dir classification. Shared with the date-prompt answer
/// parser (`file_list/new_files`).
pub(crate) fn parse_date_token(token: &str) -> Option<ScanRequest> {
    let parts: Vec<&str> = token.split('-').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    if parts
        .iter()
        .any(|part| part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(ScanRequest::Date {
        month: parts[0].parse().ok()?,
        day: parts[1].parse().ok()?,
        year: match parts.get(2) {
            Some(yy) => Some(yy.parse().ok()?),
            None => None,
        },
    })
}
