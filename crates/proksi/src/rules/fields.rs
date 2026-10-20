//! Reading the fields of a request, once each, and running every test of a field on it

use std::{borrow::Cow, net::IpAddr};

use http::{HeaderName, Method};

use super::compile::{Field, Group, Test, TextTest};

/// What rules read of a request: implemented for the proxy's requests, and for requests built in
/// tests and benchmarks
pub(crate) trait Request {
    fn method(&self) -> &Method;
    /// Without its port
    fn host(&self) -> &str;
    fn path(&self) -> &str;
    fn query(&self) -> Option<&str>;
    /// All the values of a header, joined with `, `
    fn header(&self, name: &HeaderName) -> Option<Cow<'_, str>>;
    /// The client, behind trusted proxies the one they forwarded
    fn client(&self) -> Option<IpAddr>;
}

/// Runs one test of a request-line field (method, host, path, client), reading the field again
pub(super) fn evaluate_one(field: &Field, test: &Test, request: &impl Request) -> bool {
    match (field, test) {
        (Field::Method, Test::Method { methods, negate }) => {
            methods.contains(request.method()) != *negate
        }
        (Field::Client, Test::Client { networks, negate }) => {
            let listed = client(request)
                .is_some_and(|client| networks.iter().any(|network| network.contains(client)));
            listed != *negate
        }
        (Field::Host, Test::Text { test, negate }) => {
            let host = lowercase(request.host());
            text_test(test, Some(&host)) != *negate
        }
        (Field::Path, Test::Text { test, negate }) => {
            let path = crate::routing::paths::normalize_path(request.path());
            text_test(test, Some(&path)) != *negate
        }
        _ => false,
    }
}

/// IPv4 as `::ffff:a.b.c.d`, as networks are: an IPv4 client over IPv6 is the same
fn client(request: &impl Request) -> Option<u128> {
    request.client().map(|ip| match ip {
        IpAddr::V4(v4) => u128::from(v4.to_ipv6_mapped()),
        IpAddr::V6(v6) => u128::from(v6),
    })
}

pub(super) fn lowercase(host: &str) -> Cow<'_, str> {
    if host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    }
}

/// A text test on a value (`None`: the field isn't there), before `negate`
fn text_test(test: &TextTest, value: Option<&str>) -> bool {
    match (test, value) {
        (TextTest::Exists(exists), value) => value.is_some() == *exists,
        // `matches` runs with the group's set
        (TextTest::Matches, _) | (_, None) => false,
        (TextTest::Eq(expected), Some(value)) => value == expected,
        (TextTest::In(expected), Some(value)) => expected.iter().any(|expected| value == expected),
        (TextTest::Prefix(prefixes), Some(value)) => prefixes.starts(value),
        (TextTest::Suffix(suffixes), Some(value)) => suffixes.ends(value),
        (TextTest::Contains(parts), Some(value)) => parts.found_in(value),
        (TextTest::Wildcard(parts), Some(value)) => wildcard(parts, value),
    }
}

/// Runs the tests of `group` on `request`, reading its field once: `set` gets each test's result
pub(super) fn evaluate(group: &Group, request: &impl Request, mut set: impl FnMut(u32, bool)) {
    match &group.field {
        Field::Method => {
            let method = request.method();
            for (index, test) in &group.tests {
                let Test::Method { methods, negate } = test else {
                    continue;
                };
                set(*index, methods.contains(method) != *negate);
            }
        }
        Field::Client => {
            let client = client(request);
            for (index, test) in &group.tests {
                let Test::Client { networks, negate } = test else {
                    continue;
                };
                let listed = client
                    .is_some_and(|client| networks.iter().any(|network| network.contains(client)));
                set(*index, listed != *negate);
            }
        }
        Field::Host => {
            let host = lowercase(request.host());
            text_tests(group, Some(&host), set);
        }
        Field::Path => {
            let path = crate::routing::paths::normalize_path(request.path());
            text_tests(group, Some(&path), set);
        }
        Field::Query(name) => text_tests(group, query(request.query(), name).as_deref(), set),
        Field::Header(name) => text_tests(group, request.header(name).as_deref(), set),
        Field::Cookie(name) => {
            let cookies = request.header(&http::header::COOKIE);
            text_tests(group, cookie(cookies.as_deref(), name), set);
        }
    }
}

fn text_tests(group: &Group, value: Option<&str>, mut set: impl FnMut(u32, bool)) {
    for (index, test) in &group.tests {
        let Test::Text { test, negate } = test else {
            continue;
        };
        // Run with the group's set, below
        if matches!(test, TextTest::Matches) {
            continue;
        }
        set(*index, text_test(test, value) != *negate);
    }
    if let Some((patterns, tests)) = &group.regexes {
        // Most values match none of the patterns: finding that out stops at the first match,
        // and only then are the patterns that matched worked out
        let matched = value
            .filter(|value| patterns.is_match(value))
            .map(|value| patterns.matches(value));
        for (pattern, index) in tests.iter().enumerate() {
            set(
                *index,
                matched
                    .as_ref()
                    .is_some_and(|matched| matched.matched(pattern)),
            );
        }
    }
}

/// `parts` (the text between `*`s) in order: the first at the start, the last at the end
fn wildcard(parts: &[String], value: &str) -> bool {
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
        return false;
    };
    if parts.len() == 1 {
        return value == first;
    }
    let Some(mut rest) = value.strip_prefix(first.as_str()) else {
        return false;
    };
    let Some(between) = rest.strip_suffix(last.as_str()) else {
        return false;
    };
    rest = between;
    for part in parts.get(1..parts.len() - 1).unwrap_or_default() {
        match rest
            .find(part.as_str())
            .and_then(|at| rest.get(at + part.len()..))
        {
            Some(after) => rest = after,
            None => return false,
        }
    }
    true
}

/// The value of the first query parameter `name`, decoded (`%20`, `+`)
fn query<'a>(query: Option<&'a str>, name: &str) -> Option<Cow<'a, str>> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decode(key) == name).then(|| decode(value))
    })
}

fn decode(text: &str) -> Cow<'_, str> {
    if !text.contains(['%', '+']) {
        return Cow::Borrowed(text);
    }
    let mut decoded = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = tail;
        match byte {
            b'+' => decoded.push(b' '),
            b'%' => decode_percent(&mut rest, &mut decoded),
            byte => decoded.push(byte),
        }
    }
    Cow::Owned(String::from_utf8_lossy(&decoded).into_owned())
}

/// After a `%`: the byte of the two hex digits in `rest` (taken), or the `%` itself
fn decode_percent(rest: &mut &[u8], decoded: &mut Vec<u8>) {
    let hex = rest
        .get(..2)
        .and_then(|hex| std::str::from_utf8(hex).ok())
        .and_then(|hex| u8::from_str_radix(hex, 16).ok());
    match hex {
        Some(byte) => {
            decoded.push(byte);
            *rest = rest.get(2..).unwrap_or_default();
        }
        None => decoded.push(b'%'),
    }
}

/// The value of cookie `name` in a `Cookie` header
fn cookie<'a>(cookies: Option<&'a str>, name: &str) -> Option<&'a str> {
    cookies?.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::{cookie, decode, query, wildcard};

    fn parts(pattern: &str) -> Vec<String> {
        pattern.split('*').map(ToString::to_string).collect()
    }

    #[test]
    fn test_wildcard() {
        assert!(wildcard(&parts("/api/*"), "/api/users"));
        assert!(wildcard(&parts("/api/*"), "/api/"));
        assert!(!wildcard(&parts("/api/*"), "/apis"));
        assert!(wildcard(&parts("*.example.com"), "a.b.example.com"));
        assert!(!wildcard(&parts("*.example.com"), "example.com"));
        assert!(wildcard(&parts("/a/*/c/*"), "/a/b/c/d"));
        assert!(!wildcard(&parts("/a/*/c"), "/a/b/d"));
        assert!(wildcard(&parts("*"), "anything"));
        assert!(wildcard(&parts("/exact"), "/exact"));
        assert!(!wildcard(&parts("ab*ba"), "aba"));
    }

    #[test]
    fn test_query_and_cookies() {
        assert_eq!(
            query(Some("a=1&q=a+b%21&q=2"), "q").as_deref(),
            Some("a b!")
        );
        assert_eq!(query(Some("flag&b=2"), "flag").as_deref(), Some(""));
        assert_eq!(query(Some("a=1"), "b"), None);
        assert_eq!(query(None, "b"), None);
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");

        assert_eq!(cookie(Some("a=1; beta=1; c=3"), "beta"), Some("1"));
        assert_eq!(cookie(Some("betas=1"), "beta"), None);
    }
}
