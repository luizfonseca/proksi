//! From rules as written to what a request is evaluated against: every test of a scope's rules
//! deduplicated and grouped by the field it reads, and each condition as a tree over those tests,
//! cheapest first

use std::{collections::HashMap, net::IpAddr, sync::Arc};

use http::{HeaderName, Method};
use ipnet::IpNet;
use regex::RegexSet;
use serde_json::Value;

use super::{actions::Then, model::Rule};

/// Rules in a scope (the server, a host or a location)
const MAX_RULES: usize = 1000;
/// Tests across a scope's rules (after deduplication)
const MAX_TESTS: usize = 4096;
/// Nesting of `and`, `or` and `not`
const MAX_DEPTH: usize = 16;
/// Compiled size of a regular expression
const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// From this many tests on a cheap field, they run one at a time (below, reading the field once
/// for all of them costs less)
const LAZY_FROM: usize = 8;

/// The fields a condition can test
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Field {
    Method,
    Host,
    Path,
    Query(String),
    Header(HeaderName),
    Cookie(String),
    Client,
}

impl Field {
    /// Relative cost of reading the field, to test cheap fields first
    fn cost(&self) -> u32 {
        match self {
            Field::Method => 1,
            Field::Client => 2,
            Field::Host => 3,
            Field::Path => 4,
            Field::Header(_) | Field::Query(_) => 6,
            Field::Cookie(_) => 8,
        }
    }
}

/// One test of a field's value
#[derive(Debug)]
pub(super) enum Test {
    /// Text: what it holds, and `negate` for `neq`/`not_in`
    Text {
        test: TextTest,
        negate: bool,
    },
    Method {
        methods: Vec<Method>,
        negate: bool,
    },
    Client {
        networks: Vec<Network>,
        negate: bool,
    },
}

/// A network as a prefix and its mask, over IPv6 (IPv4 as `::ffff:a.b.c.d`): checking an address
/// is one comparison
#[derive(Debug, Clone, Copy)]
pub(super) struct Network {
    prefix: u128,
    mask: u128,
}

impl Network {
    fn new(network: IpNet) -> Self {
        let (address, length) = match network {
            IpNet::V4(v4) => (
                u128::from(v4.network().to_ipv6_mapped()),
                96 + u32::from(v4.prefix_len()),
            ),
            IpNet::V6(v6) => (u128::from(v6.network()), u32::from(v6.prefix_len())),
        };
        let mask = u128::MAX.checked_shl(128 - length).unwrap_or(0);
        Self {
            prefix: address & mask,
            mask,
        }
    }

    #[inline]
    pub fn contains(&self, address: u128) -> bool {
        address & self.mask == self.prefix
    }
}

#[derive(Debug)]
pub(super) enum TextTest {
    Eq(String),
    In(Vec<String>),
    Prefix(String),
    Suffix(String),
    Contains(String),
    /// The parts between `*`s
    Wildcard(Vec<String>),
    /// Tested with the regex set of its field
    Matches,
    Exists(bool),
}

impl Test {
    fn cost(&self) -> u32 {
        match self {
            Test::Text {
                test: TextTest::Matches,
                ..
            } => 20,
            Test::Text {
                test: TextTest::Wildcard(_) | TextTest::Contains(_),
                ..
            } => 3,
            _ => 1,
        }
    }
}

/// The tests reading one field: the field is read once for all of them
#[derive(Debug)]
pub(super) struct Group {
    pub field: Field,
    /// The tests (their index) and what they check
    pub tests: Vec<(u32, Test)>,
    /// The `matches` tests, as one set, and the test each of its patterns is
    pub regexes: Option<(RegexSet, Vec<u32>)>,
    /// Tests run one at a time, when a rule needs them: the field is cheap to read again (the
    /// method, host, path or client), and a request usually needs few of its tests. Other fields
    /// are read once, and their tests run together.
    pub lazy: bool,
}

/// A condition, over the tests' results
#[derive(Debug)]
pub(super) enum Node {
    Test(u32),
    All(Box<[Node]>),
    Any(Box<[Node]>),
    Not(Box<Node>),
}

pub(super) struct CompiledRule {
    pub name: String,
    pub condition: Node,
    pub then: Arc<Then>,
}

/// A scope's rules, ready to evaluate
pub(super) struct Compiled {
    pub groups: Vec<Group>,
    /// Each test's group, and its place in it
    pub test_at: Vec<(u16, u32)>,
    pub rules: Vec<CompiledRule>,
}

impl Compiled {
    /// A test, and the group reading its field
    #[inline]
    pub fn test(&self, test: u32) -> Option<(usize, &Group, &Test)> {
        test_in(&self.groups, &self.test_at, test)
    }
}

#[inline]
fn test_in<'a>(
    groups: &'a [Group],
    test_at: &[(u16, u32)],
    test: u32,
) -> Option<(usize, &'a Group, &'a Test)> {
    let (group, position) = *test_at.get(test as usize)?;
    let group_index = usize::from(group);
    let group = groups.get(group_index)?;
    let (_, test) = group.tests.get(position as usize)?;
    Some((group_index, group, test))
}

/// Compiles a scope's rules; errors name the rule and the path to what's wrong in it
pub(super) fn compile(rules: &[Rule]) -> Result<Compiled, String> {
    if rules.len() > MAX_RULES {
        return Err(format!("more than {MAX_RULES} rules"));
    }
    let mut builder = Builder::default();
    let mut compiled = vec![];
    let mut names = std::collections::HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let rule = builder
            .rule(&rule.0)
            .map_err(|err| match rule_name(&rule.0) {
                Some(name) => format!("rule \"{name}\": {err}"),
                None => format!("rule {}: {err}", index + 1),
            })?;
        if !names.insert(rule.name.clone()) {
            return Err(format!(
                "rule \"{}\": another rule has the same name",
                rule.name
            ));
        }
        compiled.push(rule);
    }
    builder.finish(compiled)
}

fn rule_name(rule: &Value) -> Option<&str> {
    rule.get("name").and_then(Value::as_str)
}

/// A condition as written, before compiling
enum Written<'a> {
    And(&'a Value),
    Or(&'a Value),
    Not(&'a Value),
    Field {
        field: &'a str,
        name: Option<&'a str>,
        test: &'a Value,
    },
}

impl Written<'_> {
    fn path(&self) -> String {
        match self {
            Written::And(_) => "and".to_string(),
            Written::Or(_) => "or".to_string(),
            Written::Not(_) => "not".to_string(),
            Written::Field {
                field,
                name: Some(name),
                ..
            } => format!("{field} \"{name}\""),
            Written::Field { field, .. } => (*field).to_string(),
        }
    }
}

const FIELDS: &str = "`method`, `host`, `path`, `query`, `header`, `cookie`, `client`";

/// The conditions in a block (`and { ... }`), or a list of blocks (JSON, YAML): each key is one,
/// or several when it's repeated (a list) or labeled (`header "a" { }` and `header "b" { }`)
fn conditions(value: &Value) -> Result<Vec<Written<'_>>, String> {
    let mut found = vec![];
    match value {
        Value::Array(items) => {
            for item in items {
                found.extend(conditions(item)?);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let items: Vec<&Value> = match value {
                    Value::Array(items) => items.iter().collect(),
                    value => vec![value],
                };
                for item in items {
                    found.extend(condition(key, item)?);
                }
            }
        }
        _ => return Err("must hold conditions".to_string()),
    }
    Ok(found)
}

fn condition<'a>(key: &'a str, value: &'a Value) -> Result<Vec<Written<'a>>, String> {
    Ok(match key {
        "and" => vec![Written::And(value)],
        "or" => vec![Written::Or(value)],
        "not" => vec![Written::Not(value)],
        "method" | "host" | "path" | "client" => vec![Written::Field {
            field: key,
            name: None,
            test: value,
        }],
        "query" | "header" | "cookie" => {
            let Value::Object(named) = value else {
                return Err(format!(
                    "`{key}` needs a name: {key} \"<name>\" {{ eq = \"...\" }}"
                ));
            };
            let mut found = vec![];
            for (name, test) in named {
                let tests: Vec<&Value> = match test {
                    Value::Array(tests) => tests.iter().collect(),
                    test => vec![test],
                };
                found.extend(tests.into_iter().map(|test| Written::Field {
                    field: key,
                    name: Some(name),
                    test,
                }));
            }
            found
        }
        "status" | "response_header" => {
            return Err(format!(
                "`{key}` is a field of responses: response rules aren't supported yet"
            ))
        }
        other => {
            return Err(format!(
                "`{other}` isn't a condition (`and`, `or`, `not`, {FIELDS})"
            ))
        }
    })
}

#[derive(Default)]
struct Builder {
    /// Each distinct test, by what it checks
    seen: HashMap<String, u32>,
    groups: Vec<Group>,
    group_of: HashMap<Field, u16>,
    test_at: Vec<(u16, u32)>,
    /// The patterns of each group's `matches` tests, compiled into a set when done
    patterns: HashMap<u16, Vec<(String, u32)>>,
}

impl Builder {
    fn rule(&mut self, rule: &Value) -> Result<CompiledRule, String> {
        let Value::Object(map) = rule else {
            return Err("must be a block".to_string());
        };
        let name = map
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or("needs a name")?
            .to_string();
        let then = match map.get("then") {
            Some(Value::Array(thens)) if thens.len() > 1 => {
                return Err("has several `then` blocks: only one is allowed".to_string())
            }
            Some(Value::Array(thens)) => thens.first().ok_or("needs a `then` block")?,
            Some(then) => then,
            None => return Err("needs a `then` block".to_string()),
        };
        let then = Then::build(then).map_err(|err| format!("then: {err}"))?;

        let mut rest = map.clone();
        rest.remove("name");
        rest.remove("then");
        let rest = Value::Object(rest);
        let mut found = conditions(&rest)?;
        let condition = match found.len() {
            0 => return Err("needs a condition".to_string()),
            1 => found.remove(0),
            _ => {
                return Err(format!(
                    "has {} conditions side by side: combine them with `and {{ }}` or `or {{ }}`",
                    found.len()
                ))
            }
        };
        let path = condition.path();
        let condition = self
            .node(&condition, 1)
            .map_err(|err| format!("{path}{err}"))?;
        Ok(CompiledRule {
            name,
            condition,
            then: Arc::new(then),
        })
    }

    /// Errors start with the rest of their path (`.and.method.eq: ...`)
    fn node(&mut self, written: &Written<'_>, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return Err(format!(": nested more than {MAX_DEPTH} levels deep"));
        }
        let children = |builder: &mut Self, value: &Value| -> Result<Vec<Node>, String> {
            let found = conditions(value).map_err(|err| format!(": {err}"))?;
            if found.is_empty() {
                return Err(": needs at least one condition".to_string());
            }
            let mut nodes = vec![];
            for child in &found {
                let node = builder
                    .node(child, depth + 1)
                    .map_err(|err| format!(".{}{err}", child.path()))?;
                nodes.push(node);
            }
            // Cheapest first: `and` and `or` stop as soon as their result is known
            nodes.sort_by_key(|node| builder.cost(node));
            Ok(nodes)
        };
        match written {
            Written::And(value) => Ok(Node::All(children(self, value)?.into_boxed_slice())),
            Written::Or(value) => Ok(Node::Any(children(self, value)?.into_boxed_slice())),
            Written::Not(value) => {
                let mut found = conditions(value).map_err(|err| format!(": {err}"))?;
                if found.len() != 1 {
                    return Err(format!(
                        ": holds {} conditions, `not` holds one (combine them with `and {{ }}` \
                         or `or {{ }}` inside it)",
                        found.len()
                    ));
                }
                let child = found.remove(0);
                let node = self
                    .node(&child, depth + 1)
                    .map_err(|err| format!(".{}{err}", child.path()))?;
                Ok(Node::Not(Box::new(node)))
            }
            Written::Field { field, name, test } => self.field_test(field, *name, test),
        }
    }

    fn cost(&self, node: &Node) -> u32 {
        match node {
            Node::Test(test) => test_in(&self.groups, &self.test_at, *test)
                .map_or(1, |(_, group, test)| group.field.cost() + test.cost()),
            Node::All(nodes) | Node::Any(nodes) => nodes.iter().map(|node| self.cost(node)).sum(),
            Node::Not(node) => self.cost(node),
        }
    }

    fn field_test(
        &mut self,
        field: &str,
        name: Option<&str>,
        test: &Value,
    ) -> Result<Node, String> {
        let Value::Object(test) = test else {
            return Err(": must hold an operator, such as `eq = \"...\"`".to_string());
        };
        let (operator, value) = match test.len() {
            1 => test.iter().next().ok_or(": needs an operator")?,
            0 => return Err(": needs an operator, such as `eq = \"...\"`".to_string()),
            n => {
                return Err(format!(
                    ": has {n} operators, a test has one (combine tests with `and {{ }}`)"
                ))
            }
        };
        let field = match (field, name) {
            ("method", _) => Field::Method,
            ("host", _) => Field::Host,
            ("path", _) => Field::Path,
            ("client", _) => Field::Client,
            ("query", Some(name)) => Field::Query(name.to_string()),
            ("cookie", Some(name)) => Field::Cookie(name.to_string()),
            ("header", Some(name)) => Field::Header(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| format!(": {name:?} isn't a header name"))?,
            ),
            _ => return Err(format!(": `{field}` needs a name")),
        };
        let test =
            parse_test(&field, operator, value).map_err(|err| format!(".{operator}: {err}"))?;
        let key = format!("{field:?} {operator} {value}");
        if let Some(index) = self.seen.get(&key) {
            return Ok(Node::Test(*index));
        }

        let index = u32::try_from(self.test_at.len()).unwrap_or(u32::MAX);
        if self.test_at.len() >= MAX_TESTS {
            return Err(format!(
                ": more than {MAX_TESTS} different tests in the rules"
            ));
        }
        let group = if let Some(group) = self.group_of.get(&field) {
            *group
        } else {
            let group = u16::try_from(self.groups.len()).unwrap_or(u16::MAX);
            self.groups.push(Group {
                field: field.clone(),
                tests: vec![],
                regexes: None,
                lazy: false,
            });
            self.group_of.insert(field.clone(), group);
            group
        };
        if let Test::Text {
            test: TextTest::Matches,
            ..
        } = &test
        {
            let pattern = value.as_str().unwrap_or_default();
            // Checked on its own, for an error naming this pattern
            regex::RegexBuilder::new(pattern)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(|err| format!(".{operator}: {err}"))?;
            self.patterns
                .entry(group)
                .or_default()
                .push((pattern.to_string(), index));
        }
        let tests = &mut self
            .groups
            .get_mut(usize::from(group))
            .ok_or(": the rules' tests are inconsistent")?
            .tests;
        let position = u32::try_from(tests.len()).unwrap_or(u32::MAX);
        tests.push((index, test));
        self.test_at.push((group, position));
        self.seen.insert(key, index);
        Ok(Node::Test(index))
    }

    fn finish(mut self, rules: Vec<CompiledRule>) -> Result<Compiled, String> {
        for group in &mut self.groups {
            group.lazy = matches!(
                group.field,
                Field::Method | Field::Host | Field::Path | Field::Client
            ) && group.tests.len() >= LAZY_FROM;
        }
        for (group, patterns) in self.patterns {
            let set = regex::RegexSetBuilder::new(patterns.iter().map(|(pattern, _)| pattern))
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(|err| format!("regular expressions: {err}"))?;
            if let Some(group) = self.groups.get_mut(usize::from(group)) {
                group.regexes = Some((set, patterns.into_iter().map(|(_, test)| test).collect()));
                // Its patterns run as one set
                group.lazy = false;
            }
        }
        Ok(Compiled {
            groups: self.groups,
            test_at: self.test_at,
            rules,
        })
    }
}

/// What an operator checks, for a field
fn parse_test(field: &Field, operator: &str, value: &Value) -> Result<Test, String> {
    let negate = matches!(operator, "neq" | "not_in");
    match field {
        Field::Method => {
            let methods = match operator {
                "eq" | "neq" => vec![method(value)?],
                "in" | "not_in" => list(value)?.iter().map(method).collect::<Result<_, _>>()?,
                _ => return Err(unsupported("method", "`eq`, `neq`, `in` and `not_in`")),
            };
            Ok(Test::Method { methods, negate })
        }
        Field::Client => {
            let networks = match operator {
                "eq" | "neq" => vec![Network::new(network(value)?)],
                "in" | "not_in" => list(value)?
                    .iter()
                    .map(|value| network(value).map(Network::new))
                    .collect::<Result<_, _>>()?,
                _ => return Err(unsupported("client", "`eq`, `neq`, `in` and `not_in`")),
            };
            Ok(Test::Client { networks, negate })
        }
        Field::Host | Field::Path | Field::Query(_) | Field::Header(_) | Field::Cookie(_) => {
            // Hosts are compared ignoring case
            let fold = |text: String| {
                if *field == Field::Host {
                    text.to_ascii_lowercase()
                } else {
                    text
                }
            };
            let test = match operator {
                "eq" | "neq" => TextTest::Eq(fold(text(value)?)),
                "in" | "not_in" => TextTest::In(
                    list(value)?
                        .iter()
                        .map(|value| text(value).map(fold))
                        .collect::<Result<_, _>>()?,
                ),
                "prefix" => TextTest::Prefix(fold(text(value)?)),
                "suffix" => TextTest::Suffix(fold(text(value)?)),
                "contains" => TextTest::Contains(fold(text(value)?)),
                "wildcard" => TextTest::Wildcard(
                    fold(text(value)?)
                        .split('*')
                        .map(ToString::to_string)
                        .collect(),
                ),
                "matches" => {
                    value.as_str().ok_or("must be a regular expression")?;
                    TextTest::Matches
                }
                "exists" => {
                    if matches!(field, Field::Host | Field::Path) {
                        return Err(unsupported(
                            "host and path",
                            "all but `exists`: they're always there",
                        ));
                    }
                    TextTest::Exists(value.as_bool().ok_or("must be true or false")?)
                }
                "gt" | "gte" | "lt" | "lte" => {
                    return Err("compares numbers, which this field isn't".to_string())
                }
                other => return Err(format!("`{other}` isn't an operator")),
            };
            Ok(Test::Text { test, negate })
        }
    }
}

fn unsupported(field: &str, supported: &str) -> String {
    format!("isn't an operator of {field} (it takes {supported})")
}

fn list(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .filter(|values| !values.is_empty())
        .ok_or_else(|| "must be a list of values".to_string())
}

fn text(value: &Value) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        _ => Err("must be text".to_string()),
    }
}

fn method(value: &Value) -> Result<Method, String> {
    let name = value.as_str().ok_or("must be a method, such as \"GET\"")?;
    Method::from_bytes(name.to_ascii_uppercase().as_bytes())
        .map_err(|_| format!("{name:?} isn't a method"))
}

fn network(value: &Value) -> Result<IpNet, String> {
    let text = value.as_str().ok_or("must be an address or a network")?;
    text.parse::<IpNet>()
        .or_else(|_| text.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| format!("{text:?} isn't an address or a network (\"10.0.0.0/8\")"))
}

/// Field names of a written condition map (for tests)
#[cfg(test)]
pub(super) fn written(map: &serde_json::Map<String, Value>) -> usize {
    conditions(&Value::Object(map.clone())).map_or(0, |found| found.len())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{compile, Field, Node};
    use crate::rules::model::Rule;

    fn rules(values: &[serde_json::Value]) -> Result<super::Compiled, String> {
        compile(&values.iter().cloned().map(Rule).collect::<Vec<_>>())
    }

    fn err(value: serde_json::Value) -> String {
        rules(&[value]).err().unwrap_or_default()
    }

    const THEN: fn() -> serde_json::Value = || json!({ "respond": { "status": 403 } });

    #[test]
    fn test_a_rule_compiles() {
        let compiled = rules(&[json!({
            "name": "legacy clients",
            "or": {
                "and": [
                    { "method": { "eq": "get" }, "host": { "eq": "Old.Example.com" } },
                    {
                        "header": { "x-client": { "eq": "legacy" } },
                        "not": { "path": { "prefix": "/health" } }
                    }
                ]
            },
            "then": THEN(),
        })])
        .unwrap();

        assert_eq!(compiled.rules.len(), 1);
        let Node::Any(any) = &compiled.rules[0].condition else {
            panic!("an `or`");
        };
        assert_eq!(any.len(), 2);
        // One group per field read
        let fields = compiled
            .groups
            .iter()
            .map(|group| &group.field)
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        assert!(fields.contains(&&Field::Method));
    }

    #[test]
    fn test_tests_are_shared_across_rules() {
        let rule = |name: &str| json!({ "name": name, "method": { "eq": "GET" }, "then": THEN() });
        let compiled = rules(&[rule("a"), rule("b"), rule("c")]).unwrap();
        assert_eq!(compiled.test_at.len(), 1);
    }

    #[test]
    fn test_cheap_tests_first() {
        let compiled = rules(&[json!({
            "name": "a",
            "and": {
                "header": { "user-agent": { "matches": "bot" } },
                "method": { "eq": "GET" },
            },
            "then": THEN(),
        })])
        .unwrap();
        let Node::All(all) = &compiled.rules[0].condition else {
            panic!("an `and`");
        };
        let Node::Test(first) = &all[0] else {
            panic!("a test");
        };
        assert_eq!(compiled.test(*first).unwrap().1.field, Field::Method);
    }

    #[test]
    fn test_nothing_is_implicit() {
        assert_eq!(
            err(json!({
                "name": "a",
                "method": { "eq": "GET" },
                "path": { "prefix": "/api" },
                "then": THEN(),
            })),
            "rule \"a\": has 2 conditions side by side: combine them with `and { }` or `or { }`"
        );
        assert_eq!(
            err(json!({ "name": "a", "status": { "gte": 500, "lt": 600 }, "then": THEN() })),
            "rule \"a\": `status` is a field of responses: response rules aren't supported yet"
        );
        assert_eq!(
            err(json!({ "name": "a", "path": { "prefix": "/a", "suffix": "b" }, "then": THEN() })),
            "rule \"a\": path: has 2 operators, a test has one (combine tests with `and { }`)"
        );
        assert_eq!(
            err(json!({
                "name": "a",
                "not": { "method": { "eq": "GET" }, "path": { "eq": "/" } },
                "then": THEN(),
            })),
            "rule \"a\": not: holds 2 conditions, `not` holds one (combine them with `and { }` or \
             `or { }` inside it)"
        );
    }

    #[test]
    fn test_errors_point_at_the_test() {
        assert_eq!(
            err(json!({
                "name": "a",
                "or": {
                    "method": { "eq": "GET" },
                    "and": { "path": { "eq": "/" }, "method": { "gt": 1 } },
                },
                "then": THEN(),
            })),
            "rule \"a\": or.and.method.gt: isn't an operator of method (it takes `eq`, `neq`, \
             `in` and `not_in`)"
        );
        assert_eq!(
            err(json!({ "name": "a", "client": { "in": ["10.0.0"] }, "then": THEN() })),
            "rule \"a\": client.in: \"10.0.0\" isn't an address or a network (\"10.0.0.0/8\")"
        );
        assert!(
            err(json!({ "name": "a", "path": { "matches": "(" }, "then": THEN() }))
                .starts_with("rule \"a\": path.matches: regex parse error")
        );
        assert_eq!(
            err(json!({ "name": "a", "path": { "exists": true }, "then": THEN() })),
            "rule \"a\": path.exists: isn't an operator of host and path (it takes all but \
             `exists`: they're always there)"
        );
        assert_eq!(
            err(json!({ "name": "a", "verb": { "eq": "GET" }, "then": THEN() })),
            "rule \"a\": `verb` isn't a condition (`and`, `or`, `not`, `method`, `host`, `path`, \
             `query`, `header`, `cookie`, `client`)"
        );
    }

    #[test]
    fn test_rules_need_a_name_a_condition_and_then() {
        assert_eq!(
            err(json!({ "method": { "eq": "GET" }, "then": THEN() })),
            "rule 1: needs a name"
        );
        assert_eq!(
            err(json!({ "name": "a", "then": THEN() })),
            "rule \"a\": needs a condition"
        );
        assert_eq!(
            err(json!({ "name": "a", "method": { "eq": "GET" } })),
            "rule \"a\": needs a `then` block"
        );
        let rule = json!({ "name": "a", "method": { "eq": "GET" }, "then": THEN() });
        assert_eq!(
            rules(&[rule.clone(), rule]).err().as_deref(),
            Some("rule \"a\": another rule has the same name")
        );
    }

    #[test]
    fn test_lists_are_repeated_blocks() {
        // As in HCL: two `or` blocks side by side, not one `or` of two conditions
        assert_eq!(
            err(json!({
                "name": "a",
                "or": [{ "method": { "eq": "GET" } }, { "path": { "eq": "/" } }],
                "then": THEN(),
            })),
            "rule \"a\": has 2 conditions side by side: combine them with `and { }` or `or { }`"
        );
    }

    #[test]
    fn test_labeled_fields_read_as_several_conditions() {
        let map = json!({ "header": { "a": { "eq": "1" }, "b": { "eq": "2" } } });
        assert_eq!(super::written(map.as_object().unwrap()), 2);
    }
}
