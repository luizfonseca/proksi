use std::borrow::Cow;

/// Path used to choose a location, with letters, digits, `-`, `.`, `_` and `~` decoded
/// (`%61` is `a`), repeated slashes collapsed and `.`/`..` resolved, so that `/%61dmin`,
/// `//v1/session` or `/x/../v1/session` can't get around a location. Other escapes, like
/// `%2F`, are kept: decoding them changes the path. The request itself is forwarded unchanged.
pub(crate) fn normalize_path(path: &str) -> Cow<'_, str> {
    if !needs_normalizing(path) {
        return Cow::Borrowed(path);
    }

    let decoded = decode_unreserved(path);
    let mut segments: Vec<&str> = Vec::new();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }

    let mut normalized = format!("/{}", segments.join("/"));
    let trailing_slash =
        decoded.ends_with('/') || decoded.ends_with("/.") || decoded.ends_with("/..");
    if trailing_slash && normalized.len() > 1 {
        normalized.push('/');
    }
    Cow::Owned(normalized)
}

/// Whether `path` has an empty, `.` or `..` segment, or an escaped unreserved character: one
/// pass over its bytes, as nearly every path has none
fn needs_normalizing(path: &str) -> bool {
    let bytes = path.as_bytes();
    // A segment of dots (`.`, `..`) starting at `at`
    let dots = |at: usize| {
        matches!(
            (bytes.get(at), bytes.get(at + 1), bytes.get(at + 2)),
            (Some(b'.'), None | Some(b'/'), _) | (Some(b'.'), Some(b'.'), None | Some(b'/'))
        )
    };
    if dots(0) {
        return true;
    }
    bytes.iter().enumerate().any(|(at, byte)| match byte {
        b'/' => bytes.get(at + 1) == Some(&b'/') || dots(at + 1),
        b'%' => bytes.get(at..).and_then(unreserved).is_some(),
        _ => false,
    })
}

/// `path` with its escaped unreserved characters decoded, once
fn decode_unreserved(path: &str) -> String {
    let mut decoded = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(at) = rest.find('%') {
        decoded.push_str(&rest[..at]);
        let escape = &rest[at..];
        if let Some(byte) = unreserved(escape.as_bytes()) {
            decoded.push(char::from(byte));
            rest = &escape[3..];
        } else {
            decoded.push('%');
            rest = &escape[1..];
        }
    }
    decoded.push_str(rest);
    decoded
}

/// The unreserved character (RFC 3986) `escape` (`%XX...`) starts with, if it's one
fn unreserved(escape: &[u8]) -> Option<u8> {
    let digit = |at: usize| char::from(*escape.get(at)?).to_digit(16);
    let byte = u8::try_from(digit(1)? * 16 + digit(2)?).ok()?;
    let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
    unreserved.then_some(byte)
}

#[cfg(test)]
mod tests {
    use super::normalize_path;

    #[test]
    fn test_only_tricky_paths_need_normalizing() {
        for path in [
            "/",
            "/a/",
            "/.well-known/acme",
            "/a/.hidden",
            "/a/..b",
            "/a.b/c",
            "a",
        ] {
            assert!(!super::needs_normalizing(path), "{path}");
        }
        for path in ["/a%20b", "/a%2Fb", "/a%2", "/%zz", "/100%"] {
            assert!(!super::needs_normalizing(path), "{path}");
        }
        for path in ["/%61", "/%2E", "/%2e", "/a%7E"] {
            assert!(super::needs_normalizing(path), "{path}");
        }
        for path in [
            "//a", "/a//", "/a/./b", "/a/.", "/a/..", "/a/../b", ".", "..", "./a",
        ] {
            assert!(super::needs_normalizing(path), "{path}");
        }
    }

    #[test]
    fn test_paths_without_tricks_are_unchanged() {
        assert_eq!(normalize_path("/v1/session"), "/v1/session");
        assert_eq!(normalize_path("/v1/session/"), "/v1/session/");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path(""), "");
    }

    #[test]
    fn test_repeated_slashes_are_collapsed() {
        assert_eq!(normalize_path("//v1//session/"), "/v1/session/");
    }

    #[test]
    fn test_dot_segments_are_resolved() {
        assert_eq!(normalize_path("/a/./b/../c"), "/a/c");
        assert_eq!(normalize_path("/../../x"), "/x");
        assert_eq!(normalize_path("/a/.."), "/");
        assert_eq!(normalize_path("/a/b/.."), "/a/");
    }

    #[test]
    fn test_unreserved_characters_are_decoded() {
        assert_eq!(normalize_path("/%61dmin"), "/admin");
        assert_eq!(normalize_path("/%2Eenv"), "/.env");
        assert_eq!(normalize_path("/%7Euser/%41-%5a_%30"), "/~user/A-Z_0");
    }

    #[test]
    fn test_decoded_dot_segments_are_resolved() {
        assert_eq!(normalize_path("/a/%2E%2E/admin"), "/admin");
        assert_eq!(normalize_path("/a/%2e/b"), "/a/b");
    }

    #[test]
    fn test_other_escapes_are_kept() {
        for path in [
            "/a%2Fb", "/a%20b", "/%252E", "/100%", "/%zz", "/%4", "/%C3%A9",
        ] {
            assert_eq!(normalize_path(path), path, "{path}");
        }
        // Only once: `%252E` doesn't become `.`
        assert_eq!(normalize_path("/%61%252E"), "/a%252E");
    }
}
