//! Lists of values tested against a text (`prefix`, `suffix` and `contains` with a list), in a
//! time that doesn't grow with how many values there are: a hash set looked up once for each
//! distinct length (prefixes and suffixes), or one Aho-Corasick automaton (anywhere in the
//! text). Short lists are tried one by one, which is faster for them.

use std::collections::HashSet;

use aho_corasick::AhoCorasick;

/// Prefixes and suffixes tried one by one, up to this many (as fast as the hash set there)
const FEW: usize = 8;

/// The values a text starts or ends with
#[derive(Debug)]
pub(super) enum Ends {
    Few(Vec<String>),
    Many {
        /// The values' lengths, each once, shortest first
        lengths: Vec<usize>,
        values: HashSet<Box<[u8]>>,
    },
}

impl Ends {
    pub fn new(values: Vec<String>) -> Self {
        if values.len() <= FEW {
            return Ends::Few(values);
        }
        let mut lengths = values.iter().map(String::len).collect::<Vec<_>>();
        lengths.sort_unstable();
        lengths.dedup();
        let values = values
            .into_iter()
            .map(|value| value.into_bytes().into_boxed_slice())
            .collect();
        Ends::Many { lengths, values }
    }

    /// Whether `text` starts with one of the values
    pub fn starts(&self, text: &str) -> bool {
        match self {
            Ends::Few(values) => values.iter().any(|value| text.starts_with(value.as_str())),
            Ends::Many { lengths, values } => lengths.iter().any(|length| {
                text.as_bytes()
                    .get(..*length)
                    .is_some_and(|start| values.contains(start))
            }),
        }
    }

    /// Whether `text` ends with one of the values
    pub fn ends(&self, text: &str) -> bool {
        match self {
            Ends::Few(values) => values.iter().any(|value| text.ends_with(value.as_str())),
            Ends::Many { lengths, values } => lengths.iter().any(|length| {
                text.len()
                    .checked_sub(*length)
                    .and_then(|from| text.as_bytes().get(from..))
                    .is_some_and(|end| values.contains(end))
            }),
        }
    }

    /// The only value, if there's one
    pub fn single(&self) -> Option<&str> {
        match self {
            Ends::Few(values) if values.len() == 1 => values.first().map(String::as_str),
            _ => None,
        }
    }
}

/// The values a text contains
#[derive(Debug)]
pub(super) enum Parts {
    Few(Vec<String>),
    Many(AhoCorasick),
}

impl Parts {
    pub fn new(values: Vec<String>) -> Result<Self, String> {
        // From two values on, one search of the automaton beats a search for each
        if values.len() <= 1 {
            return Ok(Parts::Few(values));
        }
        AhoCorasick::new(&values)
            .map(Parts::Many)
            .map_err(|err| format!("too many values to search for: {err}"))
    }

    /// Whether `text` contains one of the values
    pub fn found_in(&self, text: &str) -> bool {
        match self {
            Parts::Few(values) => values.iter().any(|value| text.contains(value.as_str())),
            Parts::Many(automaton) => automaton.is_match(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Ends, Parts};

    fn values(count: usize) -> Vec<String> {
        let mut values = (0..count)
            .map(|i| format!("/probe-{i}"))
            .collect::<Vec<_>>();
        values.push("/.env".to_string());
        values.push("é".to_string());
        values
    }

    #[test]
    fn test_few_and_many_agree() {
        for count in [2, 1000] {
            let ends = Ends::new(values(count));
            assert!(ends.starts("/.env.local"));
            assert!(ends.starts("/probe-1/x"));
            assert!(!ends.starts("/products"));
            assert!(ends.ends("/app/.env"));
            assert!(ends.ends("café"));
            assert!(!ends.ends("/.env.local"));
            // Lengths falling inside a character (`é` is 2 bytes, as is `ë`)
            assert!(!ends.starts("ëx"));
            assert!(!ends.ends("xë"));

            let parts = Parts::new(values(count)).unwrap();
            assert!(parts.found_in("/app/.env.local"));
            assert!(!parts.found_in("/environment"));
        }
    }
}
