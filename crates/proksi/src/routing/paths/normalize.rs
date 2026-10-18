use std::borrow::Cow;

/// Path used to choose a location, with repeated slashes collapsed and `.`/`..` resolved,
/// so that `//v1/session` or `/x/../v1/session` can't get around `location "/v1/session"`.
/// The request itself is forwarded unchanged.
pub(crate) fn normalize_path(path: &str) -> Cow<'_, str> {
    if !needs_normalizing(path) {
        return Cow::Borrowed(path);
    }

    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }

    let mut normalized = format!("/{}", segments.join("/"));
    let trailing_slash = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    if trailing_slash && normalized.len() > 1 {
        normalized.push('/');
    }
    Cow::Owned(normalized)
}

/// Whether `path` has an empty, `.` or `..` segment: one pass over its bytes, as nearly every
/// path has none
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
    bytes.iter().enumerate().any(|(at, byte)| {
        *byte == b'/' && (bytes.get(at + 1) == Some(&b'/') || dots(at + 1))
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_path;

    #[test]
    fn test_only_tricky_paths_need_normalizing() {
        for path in ["/", "/a/", "/.well-known/acme", "/a/.hidden", "/a/..b", "/a.b/c", "a"] {
            assert!(!super::needs_normalizing(path), "{path}");
        }
        for path in ["//a", "/a//", "/a/./b", "/a/.", "/a/..", "/a/../b", ".", "..", "./a"] {
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
}
