//! Paths to a value in a JSON body, written as in `JSONPath` (RFC 9535): `$.user.role`,
//! `$.items[0].id`, `$['a key']`. Only paths naming a single value are read for now: wildcards
//! (`[*]`), recursive descent (`..`), slices and filters are refused, with what isn't supported
//! in the error, so that paths written today keep their meaning when they're added.

/// One step of a path: a member of an object, or an element of an array
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    Key(String),
    Index(usize),
}

/// The steps of `path`, or why it isn't one
pub(super) fn parse(path: &str) -> Result<Vec<Step>, String> {
    let fail = |why: &str| Err(format!("{path:?}: {why}"));
    let Some(mut rest) = path.strip_prefix('$') else {
        return fail("paths start with `$`, such as \"$.user.role\"");
    };
    let mut steps = vec![];
    while !rest.is_empty() {
        let (step, after) = match next_step(rest) {
            Ok(next) => next,
            Err(why) => return fail(why),
        };
        steps.push(step);
        rest = after;
    }
    if steps.is_empty() {
        return fail("`$` is the whole body: name a value in it, such as \"$.user.role\"");
    }
    Ok(steps)
}

/// The step at the start of `rest`, and what follows it
fn next_step(rest: &str) -> Result<(Step, &str), &'static str> {
    if rest.starts_with("..") {
        return Err("recursive descent (`..`) isn't supported yet");
    }
    if let Some(after) = rest.strip_prefix('.') {
        return member_name(after);
    }
    let Some(inside) = rest.strip_prefix('[') else {
        return Err("expected `.name` or `[...]`");
    };
    let Some((selector, after)) = inside.split_once(']') else {
        return Err("a `[` isn't closed");
    };
    bracketed(selector.trim()).map(|step| (step, after))
}

/// `name` in `.name`: letters, digits, `_` and anything not ASCII, not starting with a digit
fn member_name(rest: &str) -> Result<(Step, &str), &'static str> {
    if rest.starts_with('*') {
        return Err("wildcards (`*`) aren't supported yet");
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(end);
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return Err("a name after `.` starts with a letter or `_`: use `['name']` for others");
    }
    Ok((Step::Key(name.to_string()), after))
}

/// What's between `[` and `]`: a quoted name, or an index
fn bracketed(selector: &str) -> Result<Step, &'static str> {
    if let Some(name) = quoted(selector) {
        return Ok(Step::Key(name));
    }
    if selector == "*" {
        return Err("wildcards (`[*]`) aren't supported yet");
    }
    if selector.starts_with('?') {
        return Err("filters (`[?...]`) aren't supported yet");
    }
    if selector.contains(':') {
        return Err("slices (`[a:b]`) aren't supported yet");
    }
    if selector.contains(',') {
        return Err("several selectors (`[a,b]`) aren't supported yet");
    }
    if selector.starts_with('-') {
        return Err("negative indexes aren't supported yet");
    }
    selector
        .parse()
        .map(Step::Index)
        .map_err(|_| "expected a quoted name (`['name']`) or an index (`[0]`)")
}

/// The name in `'name'` or `"name"`, with `\'`, `\"` and `\\` unescaped
fn quoted(selector: &str) -> Option<String> {
    let quote = selector
        .chars()
        .next()
        .filter(|c| *c == '\'' || *c == '"')?;
    let inner = selector.strip_prefix(quote)?.strip_suffix(quote)?;
    let mut name = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => name.push(chars.next()?),
            // `'a','b'`: several names, not one
            c if c == quote => return None,
            c => name.push(c),
        }
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::{parse, Step};

    fn key(name: &str) -> Step {
        Step::Key(name.to_string())
    }

    #[test]
    fn test_paths() {
        assert_eq!(parse("$.user.role"), Ok(vec![key("user"), key("role")]));
        assert_eq!(
            parse("$.items[0].id"),
            Ok(vec![key("items"), Step::Index(0), key("id")])
        );
        assert_eq!(
            parse("$['a key'][\"b.c\"]"),
            Ok(vec![key("a key"), key("b.c")])
        );
        assert_eq!(parse("$['it\\'s']"), Ok(vec![key("it's")]));
        assert_eq!(parse("$.名前"), Ok(vec![key("名前")]));
        assert_eq!(parse("$[ 3 ]"), Ok(vec![Step::Index(3)]));
    }

    #[test]
    fn test_what_isnt_supported_yet_is_refused() {
        for (path, why) in [
            ("$.items[*].id", "wildcards (`[*]`) aren't supported yet"),
            ("$.items.*", "wildcards (`*`) aren't supported yet"),
            ("$..id", "recursive descent (`..`) isn't supported yet"),
            (
                "$.items[?@.price > 10]",
                "filters (`[?...]`) aren't supported yet",
            ),
            ("$.items[0:2]", "slices (`[a:b]`) aren't supported yet"),
            ("$.items[-1]", "negative indexes aren't supported yet"),
            (
                "$['a','b']",
                "several selectors (`[a,b]`) aren't supported yet",
            ),
        ] {
            assert_eq!(parse(path), Err(format!("{path:?}: {why}")));
        }
        for path in ["user.role", "$", "$.", "$.1a", "$[x]", "$[0"] {
            assert!(parse(path).is_err(), "{path}");
        }
    }
}
