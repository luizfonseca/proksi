use path_tree::PathTree;

use super::normalize::normalize_path;

/// Finds the location serving a path: the most specific match (a static path beats
/// a parameter, which beats a wildcard), after normalizing the path
#[derive(Default)]
pub struct LocationMatcher {
    /// Pattern -> index of the location
    tree: PathTree<usize>,
}

impl LocationMatcher {
    /// Takes the path patterns of each location, in order (the index identifies the location)
    pub fn new(locations: impl IntoIterator<Item = Vec<String>>) -> Self {
        let mut tree = PathTree::new();
        for (index, patterns) in locations.into_iter().enumerate() {
            for pattern in patterns {
                let _ = tree.insert(&pattern, index);
            }
        }
        Self { tree }
    }

    /// Index of the location serving `path`, if any
    pub fn find(&self, path: &str) -> Option<usize> {
        self.tree
            .find(&normalize_path(path))
            .map(|(index, _)| *index)
    }
}

#[cfg(test)]
mod tests {
    use super::LocationMatcher;

    fn patterns(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(ToString::to_string).collect()
    }

    /// `/*` (0), `/v1/session` and below (1), exactly `/auth` (2)
    fn matcher() -> LocationMatcher {
        LocationMatcher::new([
            patterns(&["/*"]),
            patterns(&["/v1/session", "/v1/session/*"]),
            patterns(&["/auth"]),
        ])
    }

    #[test]
    fn test_most_specific_location_wins() {
        let matcher = matcher();
        assert_eq!(matcher.find("/"), Some(0));
        assert_eq!(matcher.find("/v1/session"), Some(1));
        assert_eq!(matcher.find("/v1/session/"), Some(1));
        assert_eq!(matcher.find("/v1/session/x/y"), Some(1));
        assert_eq!(matcher.find("/v1/sessions"), Some(0));
    }

    #[test]
    fn test_exact_location_only_matches_its_path() {
        let matcher = matcher();
        assert_eq!(matcher.find("/auth"), Some(2));
        assert_eq!(matcher.find("/auth/x"), Some(0));
    }

    #[test]
    fn test_paths_are_normalized_before_matching() {
        let matcher = matcher();
        assert_eq!(matcher.find("//v1/session"), Some(1));
        assert_eq!(matcher.find("/v1//session"), Some(1));
        assert_eq!(matcher.find("/x/../v1/session"), Some(1));
        assert_eq!(matcher.find("/v1/./session"), Some(1));
    }

    #[test]
    fn test_no_matching_location() {
        let matcher = LocationMatcher::new([patterns(&["/api/*"])]);
        assert_eq!(matcher.find("/other"), None);
    }
}
