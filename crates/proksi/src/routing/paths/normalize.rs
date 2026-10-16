use std::borrow::Cow;

/// Path used to choose a location, with repeated slashes collapsed and `.`/`..` resolved,
/// so that `//v1/session` or `/x/../v1/session` can't get around `location "/v1/session"`.
/// The request itself is forwarded unchanged.
pub(super) fn normalize_path(path: &str) -> Cow<'_, str> {
    let needs_normalizing = path.contains("//")
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..");
    if !needs_normalizing {
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

#[cfg(test)]
mod tests {
    use super::normalize_path;

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
