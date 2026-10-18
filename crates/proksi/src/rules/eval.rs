//! Evaluating conditions: the results of a request's tests are bits, filled one field at a time
//! (all the tests of a field run when one of them is first needed), and `and`, `or` and `not`
//! read them, stopping as soon as their result is known

use super::{
    compile::{Compiled, Group, Node},
    fields::{self, Request},
};

/// One word covers scopes with up to 64 tests (and field groups), which most are; larger ones
/// use a list of words
enum Bits {
    Word(u64),
    Words(Vec<u64>),
}

impl Bits {
    fn new(len: usize) -> Self {
        if len <= 64 {
            Bits::Word(0)
        } else {
            Bits::Words(vec![0; len.div_ceil(64)])
        }
    }

    #[inline]
    fn get(&self, index: usize) -> bool {
        match self {
            Bits::Word(word) => word & (1 << index) != 0,
            Bits::Words(words) => words
                .get(index / 64)
                .is_some_and(|word| word & (1 << (index % 64)) != 0),
        }
    }

    #[inline]
    fn set(&mut self, index: usize, value: bool) {
        let (word, bit) = match self {
            Bits::Word(word) => (word, index),
            Bits::Words(words) => match words.get_mut(index / 64) {
                Some(word) => (word, index % 64),
                None => return,
            },
        };
        if value {
            *word |= 1 << bit;
        } else {
            *word &= !(1 << bit);
        }
    }

    fn clear(&mut self) {
        match self {
            Bits::Word(word) => *word = 0,
            Bits::Words(words) => words.fill(0),
        }
    }
}

/// The tests of one request, against one scope's rules
pub(super) struct State {
    results: Bits,
    /// The field groups already read
    read: Bits,
    /// The tests of lazy groups already run
    known: Bits,
}

impl State {
    pub fn new(compiled: &Compiled) -> Self {
        Self {
            results: Bits::new(compiled.test_at.len()),
            read: Bits::new(compiled.groups.len()),
            known: Bits::new(compiled.test_at.len()),
        }
    }

    /// Forgets what was read: the request changed (a rule changed its method or headers)
    pub fn forget(&mut self) {
        self.read.clear();
        self.known.clear();
    }

    #[inline]
    pub fn holds(&mut self, compiled: &Compiled, node: &Node, request: &impl Request) -> bool {
        // One test: no recursion
        if let Node::Test(test) = node {
            return self.test(compiled, *test, request);
        }
        self.holds_tree(compiled, node, request)
    }

    fn holds_tree(&mut self, compiled: &Compiled, node: &Node, request: &impl Request) -> bool {
        match node {
            Node::Test(test) => self.test(compiled, *test, request),
            Node::All(nodes) => nodes.iter().all(|node| self.holds(compiled, node, request)),
            Node::Any(nodes) => nodes.iter().any(|node| self.holds(compiled, node, request)),
            Node::Not(node) => !self.holds(compiled, node, request),
        }
    }

    #[inline]
    fn test(&mut self, compiled: &Compiled, test: u32, request: &impl Request) -> bool {
        let Some((group_index, group, written)) = compiled.test(test) else {
            return false;
        };
        let index = test as usize;
        if group.lazy {
            if !self.known.get(index) {
                let holds = fields::evaluate_one(&group.field, written, request);
                self.results.set(index, holds);
                self.known.set(index, true);
            }
        } else if !self.read.get(group_index) {
            self.read_group(group_index, group, request);
        }
        self.results.get(index)
    }

    #[cold]
    fn read_group(&mut self, index: usize, group: &Group, request: &impl Request) {
        let results = &mut self.results;
        fields::evaluate(group, request, |test, value| {
            results.set(test as usize, value);
        });
        self.read.set(index, true);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::{borrow::Cow, net::IpAddr};

    use http::{HeaderMap, HeaderName, Method};
    use serde_json::json;

    use crate::rules::{compile::compile, fields::Request, model::Rule};

    use super::State;

    /// A request built in tests and benchmarks
    pub struct TestRequest {
        pub method: Method,
        pub host: String,
        pub path: String,
        pub query: Option<String>,
        pub headers: HeaderMap,
        pub client: Option<IpAddr>,
    }

    impl TestRequest {
        pub fn new(method: &str, host: &str, uri: &str) -> Self {
            let (path, query) = match uri.split_once('?') {
                Some((path, query)) => (path, Some(query.to_string())),
                None => (uri, None),
            };
            Self {
                method: Method::from_bytes(method.as_bytes()).unwrap(),
                host: host.to_string(),
                path: path.to_string(),
                query,
                headers: HeaderMap::new(),
                client: Some("203.0.113.9".parse().unwrap()),
            }
        }

        pub fn header(mut self, name: &'static str, value: &str) -> Self {
            self.headers.append(name, value.parse().unwrap());
            self
        }
    }

    impl Request for TestRequest {
        fn method(&self) -> &Method {
            &self.method
        }
        fn host(&self) -> &str {
            &self.host
        }
        fn path(&self) -> &str {
            &self.path
        }
        fn query(&self) -> Option<&str> {
            self.query.as_deref()
        }
        fn header(&self, name: &HeaderName) -> Option<Cow<'_, str>> {
            // As the proxy's: borrowed, unless the header is repeated
            let mut values = self.headers.get_all(name).iter();
            let first = values.next()?.to_str().ok()?;
            match values.next() {
                None => Some(Cow::Borrowed(first)),
                Some(second) => Some(Cow::Owned(format!("{first}, {}", second.to_str().ok()?))),
            }
        }
        fn client(&self) -> Option<IpAddr> {
            self.client
        }
    }

    /// The names of the rules a request matches
    fn matching(rules: &[serde_json::Value], request: &TestRequest) -> Vec<String> {
        let compiled = compile(&rules.iter().cloned().map(Rule).collect::<Vec<_>>()).unwrap();
        let mut state = State::new(&compiled);
        compiled
            .rules
            .iter()
            .filter(|rule| state.holds(&compiled, &rule.condition, request))
            .map(|rule| rule.name.clone())
            .collect()
    }

    fn rule(name: &str, condition: serde_json::Value) -> serde_json::Value {
        let mut rule = condition;
        rule["name"] = json!(name);
        rule["then"] = json!({ "respond": { "status": 403 } });
        rule
    }

    #[test]
    fn test_fields_and_operators() {
        let rules = [
            rule("get", json!({ "method": { "eq": "get" } })),
            rule("not get", json!({ "method": { "neq": "GET" } })),
            rule(
                "writes",
                json!({ "method": { "not_in": ["GET", "HEAD", "OPTIONS"] } }),
            ),
            rule("host", json!({ "host": { "eq": "Old.Example.com" } })),
            rule(
                "subdomain",
                json!({ "host": { "wildcard": "*.example.com" } }),
            ),
            rule("api", json!({ "path": { "prefix": "/api/" } })),
            rule("normalized", json!({ "path": { "eq": "/api/users" } })),
            rule("json", json!({ "path": { "suffix": ".json" } })),
            rule("debug", json!({ "query": { "debug": { "eq": "1" } } })),
            rule(
                "no debug",
                json!({ "query": { "debug": { "exists": false } } }),
            ),
            rule(
                "bot",
                json!({ "header": { "user-agent": { "matches": "(?i)bot" } } }),
            ),
            rule("beta", json!({ "cookie": { "beta": { "eq": "1" } } })),
            rule("office", json!({ "client": { "in": ["203.0.113.0/24"] } })),
            rule(
                "not office",
                json!({ "client": { "not_in": ["203.0.113.0/24"] } }),
            ),
        ];

        let request = TestRequest::new("GET", "old.example.com", "//api/./users?debug=1")
            .header("user-agent", "GoodBot/1.0")
            .header("cookie", "a=b; beta=1");
        assert_eq!(
            matching(&rules, &request),
            [
                "get",
                "host",
                "subdomain",
                "api",
                "normalized",
                "debug",
                "bot",
                "beta",
                "office"
            ]
        );

        let mut request = TestRequest::new("DELETE", "example.com", "/data.json");
        request.client = Some("::ffff:203.0.113.5".parse().unwrap());
        assert_eq!(
            matching(&rules, &request),
            ["not get", "writes", "json", "no debug", "office"]
        );
    }

    #[test]
    fn test_and_or_not() {
        let rules = [rule(
            "legacy",
            // `or { and { ... } and { ... } }`: repeated blocks are a list
            json!({
                "or": {
                    "and": [
                        { "method": { "eq": "GET" }, "host": { "eq": "old.example.com" } },
                        {
                            "header": { "x-client": { "eq": "legacy" } },
                            "not": { "path": { "prefix": "/health" } },
                        },
                    ],
                },
            }),
        )];
        let matches = |request: TestRequest| !matching(&rules, &request).is_empty();

        assert!(matches(TestRequest::new("GET", "old.example.com", "/")));
        assert!(!matches(TestRequest::new("POST", "old.example.com", "/")));
        assert!(matches(
            TestRequest::new("POST", "a.test", "/").header("x-client", "legacy")
        ));
        assert!(!matches(
            TestRequest::new("POST", "a.test", "/health").header("x-client", "legacy")
        ));
    }

    #[test]
    fn test_ranges_repeat_a_field() {
        // `and` holding the same field twice: a list (JSON, or repeated blocks in HCL)
        let rules = [rule(
            "middle",
            json!({ "and": { "path": [{ "prefix": "/a" }, { "suffix": "/z" }] } }),
        )];
        assert_eq!(
            matching(&rules, &TestRequest::new("GET", "a.test", "/a/b/z")),
            ["middle"]
        );
        assert!(matching(&rules, &TestRequest::new("GET", "a.test", "/a/b")).is_empty());
    }
}
