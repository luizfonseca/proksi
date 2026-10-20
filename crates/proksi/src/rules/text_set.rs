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

/// The values a text is one of (`in`, `not_in`)
#[derive(Debug)]
pub(super) enum Exact {
    Few(Vec<String>),
    Many(HashSet<String>),
}

impl Exact {
    pub fn new(values: Vec<String>) -> Self {
        if values.len() <= FEW {
            return Exact::Few(values);
        }
        Exact::Many(values.into_iter().collect())
    }

    /// Whether `text` is one of the values
    pub fn holds(&self, text: &str) -> bool {
        match self {
            Exact::Few(values) => values.iter().any(|value| value == text),
            Exact::Many(values) => values.contains(text),
        }
    }

    /// The values, in no particular order
    pub fn values(&self) -> Vec<String> {
        match self {
            Exact::Few(values) => values.clone(),
            Exact::Many(values) => values.iter().cloned().collect(),
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
    use super::{Ends, Exact, Parts};

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

            let exact = Exact::new(values(count));
            assert!(exact.holds("/.env") && exact.holds("é"));
            assert!(!exact.holds("/.env.local"));

            let parts = Parts::new(values(count)).unwrap();
            assert!(parts.found_in("/app/.env.local"));
            assert!(!parts.found_in("/environment"));
        }
    }

    /// Random lists and texts: the sets give the answers of trying each value
    #[test]
    fn test_the_same_as_trying_each_value() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // Short texts over few letters, so values often start, end or appear in them
        let mut text = |length: u64| {
            (0..length)
                .map(|_| ['a', 'b', '/', '.', 'é'][usize::try_from(next() % 5).unwrap()])
                .collect::<String>()
        };
        let values = (0..300).map(|i| text(1 + i % 5)).collect::<Vec<_>>();
        let (ends, exact) = (Ends::new(values.clone()), Exact::new(values.clone()));
        let parts = Parts::new(values.clone()).unwrap();
        for i in 0..5_000 {
            let candidate = text(i % 12);
            let any =
                |test: fn(&str, &str) -> bool| values.iter().any(|value| test(&candidate, value));
            assert_eq!(
                ends.starts(&candidate),
                any(|text, value| text.starts_with(value)),
                "{candidate}"
            );
            assert_eq!(
                ends.ends(&candidate),
                any(|text, value| text.ends_with(value)),
                "{candidate}"
            );
            assert_eq!(
                parts.found_in(&candidate),
                any(|text, value| text.contains(value)),
                "{candidate}"
            );
            assert_eq!(
                exact.holds(&candidate),
                any(|text, value| text == value),
                "{candidate}"
            );
        }
    }
}
