use path_tree::PathTree;
use regex_automata::{meta, util::syntax, Anchored, Input};

use super::normalize::normalize_path;

/// The most regex locations a host can have: one automaton finds the first matching one in a
/// single pass, but its memory grows quickly past a few thousand (ADR 0010)
pub const MAX_REGEXES: usize = 1000;

/// The largest compiled size of a regex location
const MAX_REGEX_SIZE: usize = 1 << 20;

/// Finds the location serving a path, after normalizing it: the first regex location (in
/// order) matching it, then the most specific path (a static path beats a parameter, which
/// beats a wildcard)
#[derive(Default)]
pub struct LocationMatcher {
    /// Pattern -> index of the location
    tree: PathTree<usize>,
    /// Regex locations, as one automaton, and the location of each of its patterns
    regexes: Option<(meta::Regex, Vec<usize>)>,
}

impl LocationMatcher {
    /// Takes the path patterns of each location, in order (the index identifies the location).
    /// Patterns starting with `^` are regexes, valid as [`compile_regexes`] checks them.
    pub fn new(locations: impl IntoIterator<Item = Vec<String>>) -> Self {
        let mut tree = PathTree::new();
        let (mut regexes, mut regex_locations) = (vec![], vec![]);
        let patterns = locations
            .into_iter()
            .enumerate()
            .flat_map(|(index, patterns)| {
                patterns.into_iter().map(move |pattern| (index, pattern))
            });
        for (index, pattern) in patterns {
            if pattern.starts_with('^') {
                regexes.push(pattern);
                regex_locations.push(index);
            } else {
                let _ = tree.insert(&pattern, index);
            }
        }
        let regexes = match compile_regexes(&regexes) {
            Ok(regex) => regex.map(|regex| (regex, regex_locations)),
            Err(err) => {
                tracing::error!("regex locations ignored: {err}");
                None
            }
        };
        Self { tree, regexes }
    }

    /// Index of the location serving `path`, if any
    pub fn find(&self, path: &str) -> Option<usize> {
        let path = normalize_path(path);
        if let Some((regex, locations)) = &self.regexes {
            let input = Input::new(path.as_ref()).anchored(Anchored::Yes);
            if let Some(found) = regex.search_half(&input) {
                return locations.get(found.pattern().as_usize()).copied();
            }
        }
        self.tree.find(&path).map(|(index, _)| *index)
    }
}

/// The regex locations of a host as one automaton, the first written winning (`None` without
/// any), or why they can't be
pub fn compile_regexes<P: AsRef<str>>(patterns: &[P]) -> Result<Option<meta::Regex>, String> {
    if patterns.is_empty() {
        return Ok(None);
    }
    if patterns.len() > MAX_REGEXES {
        return Err(format!(
            "{} regex locations, a host can have {MAX_REGEXES} at most",
            patterns.len()
        ));
    }
    meta::Regex::builder()
        .syntax(syntax::Config::new())
        .configure(meta::Config::new().nfa_size_limit(Some(MAX_REGEX_SIZE * patterns.len())))
        .build_many(patterns)
        .map(Some)
        .map_err(|err| err.to_string())
}

/// Checks one regex location, for an error naming it
pub fn compile_regex(pattern: &str) -> Result<(), String> {
    regex::RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE)
        .build()
        .map(|_| ())
        .map_err(|err| match err {
            regex::Error::CompiledTooBig(_) => "the regex is too large".to_string(),
            err => format!("invalid regex: {err}"),
        })
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
    fn test_encoded_letters_and_dots_are_matched_decoded() {
        let matcher = matcher();
        assert_eq!(matcher.find("/v1/%73ession"), Some(1));
        assert_eq!(matcher.find("/%61uth"), Some(2));
        assert_eq!(matcher.find("/x/%2E%2e/v1/session"), Some(1));
        // An encoded slash is a different path
        assert_eq!(matcher.find("/v1%2Fsession"), Some(0));
    }

    #[test]
    fn test_exact_location_and_its_trailing_slash() {
        let matcher = LocationMatcher::new([patterns(&["/*"]), patterns(&["/login"])]);
        assert_eq!(matcher.find("/login"), Some(1));
        assert_eq!(matcher.find("/login/"), Some(0));
    }

    #[test]
    fn test_regex_locations_come_first_in_order() {
        let matcher = LocationMatcher::new([
            patterns(&["/*"]),
            patterns(&["/blog", "/blog/*"]),
            patterns(&["^/blog/[a-z]{8}/(posts|drafts)$"]),
            patterns(&["^/blog/[a-z]+/posts"]),
        ]);
        assert_eq!(matcher.find("/blog/abcdefgh/posts"), Some(2));
        assert_eq!(matcher.find("/blog/abcdefgh/drafts"), Some(2));
        // Not eight letters, or something after `posts`: the next regex
        assert_eq!(matcher.find("/blog/abc/posts"), Some(3));
        assert_eq!(matcher.find("/blog/abcdefgh/posts/1"), Some(3));
        // No regex: the most specific path
        assert_eq!(matcher.find("/blog/ABCDEFGH/posts"), Some(1));
        assert_eq!(matcher.find("/other"), Some(0));
    }

    #[test]
    fn test_regex_locations_match_the_normalized_path_from_its_start() {
        let matcher = LocationMatcher::new([patterns(&["^/a/b$"]), patterns(&["/*"])]);
        assert_eq!(matcher.find("//a/./b"), Some(0));
        assert_eq!(matcher.find("/x/a/b"), Some(1));
    }

    #[test]
    fn test_no_matching_location() {
        let matcher = LocationMatcher::new([patterns(&["/api/*"])]);
        assert_eq!(matcher.find("/other"), None);
    }
}
