//! Candidates: the rules a request can match, from what their conditions require (a method, a
//! host, a path under a first segment), so a request skips the rules it can't match without
//! evaluating them

use std::collections::HashMap;

use http::Method;

use super::compile::{Compiled, Field, Node, Test, TextTest};

/// Rules a scope can have, in 64-bit words (`MAX_RULES` in `compile`)
pub(super) const WORDS: usize = 16;

/// Below this many rules, evaluating them all costs less than looking them up
const MIN_RULES: usize = 16;

/// Rules as bits, in the order they're written
#[derive(Clone, Copy)]
pub(super) struct RuleSet {
    words: [u64; WORDS],
}

impl RuleSet {
    fn none() -> Self {
        Self { words: [0; WORDS] }
    }

    fn all(len: usize) -> Self {
        let mut set = Self::none();
        for (word, bits) in set.words.iter_mut().enumerate() {
            let from = word * 64;
            *bits = match len.saturating_sub(from) {
                0 => 0,
                64.. => u64::MAX,
                rest => (1 << rest) - 1,
            };
        }
        set
    }

    fn insert(&mut self, rule: usize) {
        if let Some(word) = self.words.get_mut(rule / 64) {
            *word |= 1 << (rule % 64);
        }
    }

    fn and(&mut self, other: &RuleSet, words: usize) {
        for (word, other) in self.words.iter_mut().zip(&other.words).take(words) {
            *word &= other;
        }
    }

    fn or(&mut self, other: &RuleSet, words: usize) {
        for (word, other) in self.words.iter_mut().zip(&other.words).take(words) {
            *word |= other;
        }
    }

    /// The first rule from `from` on
    pub fn next(&self, from: usize) -> Option<usize> {
        let first = from / 64;
        let mut bits = self.words.get(first)? & (u64::MAX << (from % 64));
        let mut word = first;
        loop {
            if bits != 0 {
                return Some(word * 64 + bits.trailing_zeros() as usize);
            }
            word += 1;
            bits = *self.words.get(word)?;
        }
    }
}

/// What a condition requires of a request, as far as the index knows (`None`: anything)
#[derive(Default)]
struct Requires {
    /// Method slots ([`slot`])
    methods: Option<u16>,
    hosts: Option<Vec<String>>,
    /// First path segments
    segments: Option<Vec<String>>,
}

const SLOTS: usize = 10;

/// GET, POST... and every other method in the last slot
fn slot(method: &Method) -> usize {
    match *method {
        Method::GET => 0,
        Method::POST => 1,
        Method::PUT => 2,
        Method::DELETE => 3,
        Method::HEAD => 4,
        Method::OPTIONS => 5,
        Method::PATCH => 6,
        Method::CONNECT => 7,
        Method::TRACE => 8,
        _ => 9,
    }
}

/// The first segment of a path (`api` in `/api/users`), when `path` holds all of it: a path
/// starting with `prefix` then always has it
fn first_segment(prefix: &str, whole: bool) -> Option<String> {
    let rest = prefix.strip_prefix('/')?;
    match rest.find('/') {
        Some(end) => Some(rest[..end].to_string()),
        None if whole && !rest.is_empty() => Some(rest.to_string()),
        None => None,
    }
}

/// What a request needs to pass a test (unless it's negated)
fn test_requirements(field: &Field, test: &Test) -> Requires {
    let mut requires = Requires::default();
    match (field, test) {
        (
            Field::Method,
            Test::Method {
                methods,
                negate: false,
            },
        ) => {
            requires.methods = Some(
                methods
                    .iter()
                    .fold(0, |mask, method| mask | 1 << slot(method)),
            );
        }
        (
            Field::Host,
            Test::Text {
                test,
                negate: false,
            },
        ) => {
            requires.hosts = match test {
                TextTest::Eq(host) => Some(vec![host.clone()]),
                TextTest::In(hosts) => Some(hosts.clone()),
                _ => None,
            };
        }
        (
            Field::Path,
            Test::Text {
                test,
                negate: false,
            },
        ) => {
            let segment = match test {
                TextTest::Eq(path) => first_segment(path, true),
                TextTest::Prefix(prefix) => first_segment(prefix, false),
                TextTest::Wildcard(parts) if parts.len() > 1 => {
                    parts.first().and_then(|first| first_segment(first, false))
                }
                _ => None,
            };
            requires.segments = segment.map(|segment| vec![segment]);
        }
        _ => {}
    }
    requires
}

fn requirements(compiled: &Compiled, node: &Node) -> Requires {
    match node {
        Node::Test(test) => compiled
            .test(*test)
            .map_or_else(Requires::default, |(_, group, test)| {
                test_requirements(&group.field, test)
            }),
        // Every condition holds: any of their requirements applies
        Node::All(nodes) => {
            let mut requires = Requires::default();
            for node in nodes {
                let child = requirements(compiled, node);
                requires.methods = requires.methods.or(child.methods);
                requires.hosts = requires.hosts.or(child.hosts);
                requires.segments = requires.segments.or(child.segments);
            }
            requires
        }
        // One condition holds: a requirement applies when every one of them has it
        Node::Any(nodes) => {
            let children = nodes
                .iter()
                .map(|node| requirements(compiled, node))
                .collect::<Vec<_>>();
            let mut requires = Requires::default();
            if children.iter().all(|child| child.methods.is_some()) {
                requires.methods = Some(
                    children
                        .iter()
                        .filter_map(|child| child.methods)
                        .fold(0, |a, b| a | b),
                );
            }
            if children.iter().all(|child| child.hosts.is_some()) {
                requires.hosts = Some(
                    children
                        .iter()
                        .flat_map(|child| child.hosts.clone().unwrap_or_default())
                        .collect(),
                );
            }
            if children.iter().all(|child| child.segments.is_some()) {
                requires.segments = Some(
                    children
                        .iter()
                        .flat_map(|child| child.segments.clone().unwrap_or_default())
                        .collect(),
                );
            }
            requires
        }
        Node::Not(_) => Requires::default(),
    }
}

/// The rules each method, host and first path segment allows
pub(super) struct Index {
    words: usize,
    methods: [RuleSet; SLOTS],
    any_host: RuleSet,
    hosts: HashMap<String, RuleSet>,
    any_segment: RuleSet,
    segments: HashMap<String, RuleSet>,
}

impl Index {
    /// `None` for scopes with few rules, where evaluating them all is cheaper
    pub fn build(compiled: &Compiled) -> Option<Self> {
        let rules = compiled.rules.len();
        if rules < MIN_RULES {
            return None;
        }
        let mut index = Self {
            words: rules.div_ceil(64),
            methods: [RuleSet::none(); SLOTS],
            any_host: RuleSet::none(),
            hosts: HashMap::new(),
            any_segment: RuleSet::none(),
            segments: HashMap::new(),
        };
        for (rule, compiled_rule) in compiled.rules.iter().enumerate() {
            index.add(rule, requirements(compiled, &compiled_rule.condition));
        }
        Some(index)
    }

    /// Rule `rule`, under what it requires
    fn add(&mut self, rule: usize, requires: Requires) {
        let methods = requires.methods.unwrap_or(u16::MAX);
        for (slot, set) in self.methods.iter_mut().enumerate() {
            if methods & (1 << slot) != 0 {
                set.insert(rule);
            }
        }
        match requires.hosts {
            Some(hosts) => {
                for host in hosts {
                    self.hosts
                        .entry(host)
                        .or_insert_with(RuleSet::none)
                        .insert(rule);
                }
            }
            None => self.any_host.insert(rule),
        }
        match requires.segments {
            Some(segments) => {
                for segment in segments {
                    self.segments
                        .entry(segment)
                        .or_insert_with(RuleSet::none)
                        .insert(rule);
                }
            }
            None => self.any_segment.insert(rule),
        }
    }

    /// The rules a request with this method, host (lowercase) and path (normalized) can match
    pub fn candidates(&self, method: &Method, host: &str, path: &str) -> RuleSet {
        let Some(mut set) = self.methods.get(slot(method)).copied() else {
            return RuleSet::all(WORDS * 64);
        };

        let mut hosts = self.any_host;
        if let Some(rules) = self.hosts.get(host) {
            hosts.or(rules, self.words);
        }
        set.and(&hosts, self.words);

        let mut segments = self.any_segment;
        let segment = path
            .strip_prefix('/')
            .map(|rest| rest.split('/').next().unwrap_or_default());
        if let Some(rules) = segment.and_then(|segment| self.segments.get(segment)) {
            segments.or(rules, self.words);
        }
        set.and(&segments, self.words);
        set
    }
}

/// Every rule, for scopes without an index
pub(super) fn all(rules: usize) -> RuleSet {
    RuleSet::all(rules)
}

#[cfg(test)]
mod tests {
    use http::Method;
    use serde_json::json;

    use crate::rules::{compile::compile, model::Rule};

    use super::{first_segment, Index};

    fn index(conditions: &[serde_json::Value]) -> (Index, usize) {
        // Padded with rules that require nothing, past the size an index is built for
        let mut rules = conditions
            .iter()
            .enumerate()
            .map(|(i, condition)| {
                let mut rule = crate::rules::compile::tests::in_and(condition.clone());
                rule["name"] = json!(format!("r{i}"));
                rule["then"] = json!({ "respond": { "status": 403 } });
                Rule(rule)
            })
            .collect::<Vec<_>>();
        let count = rules.len();
        for i in 0..super::MIN_RULES {
            rules.push(Rule(json!({
                "name": format!("pad{i}"),
                "not": { "method": { "eq": "TRACE" } },
                "then": { "respond": { "status": 403 } },
            })));
        }
        (Index::build(&compile(&rules).unwrap()).unwrap(), count)
    }

    fn candidates(index: &(Index, usize), method: Method, host: &str, path: &str) -> Vec<usize> {
        let set = index.0.candidates(&method, host, path);
        let mut found = vec![];
        let mut from = 0;
        while let Some(rule) = set.next(from) {
            if rule < index.1 {
                found.push(rule);
            }
            from = rule + 1;
        }
        found
    }

    #[test]
    fn test_requirements_skip_rules() {
        let index = index(&[
            json!({ "method": { "eq": "POST" } }),
            json!({ "host": { "in": ["a.test", "b.test"] } }),
            json!({ "path": { "prefix": "/api/v1/" } }),
            json!({ "and": { "method": { "in": ["PUT", "PATCH"] }, "path": { "eq": "/health" } } }),
            json!({ "or": { "method": { "eq": "GET" }, "path": { "prefix": "/x/" } } }),
            json!({ "not": { "method": { "eq": "GET" } } }),
            json!({ "path": { "prefix": "/ap" } }),
        ]);

        assert_eq!(
            candidates(&index, Method::GET, "c.test", "/other"),
            [4, 5, 6]
        );
        assert_eq!(
            candidates(&index, Method::POST, "a.test", "/api/v1/x"),
            [0, 1, 2, 4, 5, 6]
        );
        assert_eq!(
            candidates(&index, Method::PATCH, "c.test", "/health"),
            [3, 4, 5, 6]
        );
    }

    #[test]
    fn test_first_segment() {
        assert_eq!(first_segment("/api/v1/", false).as_deref(), Some("api"));
        assert_eq!(first_segment("/api", false), None);
        assert_eq!(first_segment("/api", true).as_deref(), Some("api"));
        assert_eq!(first_segment("/", true), None);
        assert_eq!(first_segment("api/", false), None);
    }
}
