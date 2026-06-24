use super::{NumberArg, PostArg};

pub(super) fn parse_number_command(line: &str, command: &str) -> Option<NumberArg> {
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

pub(super) fn parse_post_command(line: &str) -> Option<PostArg> {
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
