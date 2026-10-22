//! A spec's contract, as kept: its paths, the methods of each, and their required query and
//! header parameters (`Plan`, written to the cache), and the same ready for requests
//! (`Contract`: a path tree, and header names already parsed).

use std::fmt::Write;

use http::{HeaderMap, HeaderName, Method};
use path_tree::PathTree;
use serde::{Deserialize, Serialize};

/// Methods, as bits
pub(super) const METHODS: [(&str, u8); 8] = [
    ("get", 1),
    ("put", 2),
    ("post", 4),
    ("delete", 8),
    ("options", 16),
    ("head", 32),
    ("patch", 64),
    ("trace", 128),
];

/// The contract as compiled from a spec, and as written to the cache
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Plan {
    /// Required parameters, referred to by their index
    pub(super) names: Vec<Name>,
    pub(super) paths: Vec<PathPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Name {
    pub(super) header: bool,
    pub(super) name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct PathPlan {
    /// As written in the spec (`/users/{id}`)
    pub(super) template: String,
    /// The methods, and the required parameters of each
    pub(super) operations: Vec<(u8, Vec<u32>)>,
}

/// Why a request isn't in the contract
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// No path of the spec
    NotFound,
    /// A path of the spec, but not this method: the ones it has
    Method(String),
    /// A required parameter is missing: where, and its name
    Missing(&'static str, String),
}

/// A contract, ready for requests
pub(super) struct Contract {
    tree: PathTree<usize>,
    /// For each path: its methods (bits), and their required parameters
    operations: Vec<Vec<(u8, Box<[u32]>)>>,
    queries: Vec<Option<String>>,
    headers: Vec<Option<HeaderName>>,
}

impl Contract {
    pub(super) fn new(plan: &Plan) -> Self {
        let mut tree = PathTree::new();
        let mut operations = Vec::with_capacity(plan.paths.len());
        for path in &plan.paths {
            let _ = tree.insert(&pattern(&path.template), operations.len());
            operations.push(
                path.operations
                    .iter()
                    .map(|(methods, required)| (*methods, required.clone().into_boxed_slice()))
                    .collect(),
            );
        }
        Self {
            tree,
            operations,
            queries: plan
                .names
                .iter()
                .map(|name| (!name.header).then(|| name.name.clone()))
                .collect(),
            headers: plan
                .names
                .iter()
                .map(|name| {
                    name.header
                        .then(|| HeaderName::try_from(&name.name).ok())
                        .flatten()
                })
                .collect(),
        }
    }

    /// Whether the request is in the contract
    pub(super) fn check(
        &self,
        method: &Method,
        path: &str,
        query: Option<&str>,
        headers: &HeaderMap,
    ) -> Result<(), Refusal> {
        let (index, _) = self.tree.find(path).ok_or(Refusal::NotFound)?;
        let operations = self.operations.get(*index).ok_or(Refusal::NotFound)?;
        let bit = method_bit(method);
        // A `GET` answers `HEAD` too
        let found = operations
            .iter()
            .find(|(methods, _)| methods & bit != 0)
            .or_else(|| {
                (bit == 32)
                    .then(|| operations.iter().find(|(methods, _)| methods & 1 != 0))
                    .flatten()
            });
        let Some((_, required)) = found else {
            return Err(Refusal::Method(allowed(operations)));
        };
        for id in required.iter().map(|id| *id as usize) {
            self.has(id, query, headers)?;
        }
        Ok(())
    }

    /// Whether the required parameter `id` is in the request
    fn has(&self, id: usize, query: Option<&str>, headers: &HeaderMap) -> Result<(), Refusal> {
        if let Some(Some(name)) = self.headers.get(id) {
            return if headers.contains_key(name) {
                Ok(())
            } else {
                Err(Refusal::Missing("header", name.to_string()))
            };
        }
        let Some(Some(name)) = self.queries.get(id) else {
            return Ok(());
        };
        if in_query(query.unwrap_or_default(), name) {
            Ok(())
        } else {
            Err(Refusal::Missing("query parameter", name.clone()))
        }
    }
}

/// Whether the query string has the parameter `name` (`a=1&b`, decoded when it's encoded)
fn in_query(query: &str, name: &str) -> bool {
    query.split('&').any(|pair| {
        let key = pair.split_once('=').map_or(pair, |(key, _)| key);
        key == name || (key.contains(['%', '+']) && decoded(key) == name)
    })
}

fn decoded(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = [bytes.next().unwrap_or(b'0'), bytes.next().unwrap_or(b'0')];
                let value = std::str::from_utf8(&hex)
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                out.push(value.unwrap_or(b'%'));
            }
            byte => out.push(byte),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The bit of `method` (0 for methods `OpenAPI` doesn't name)
fn method_bit(method: &Method) -> u8 {
    METHODS
        .iter()
        .find(|(name, _)| method.as_str().eq_ignore_ascii_case(name))
        .map_or(0, |(_, bit)| *bit)
}

/// `Allow` for a path: its methods, in upper case
fn allowed(operations: &[(u8, Box<[u32]>)]) -> String {
    let all = operations.iter().fold(0, |all, (methods, _)| all | methods);
    METHODS
        .iter()
        .filter(|(_, bit)| all & bit != 0)
        .map(|(name, _)| name.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A spec's path template (`/users/{id}`) as a path tree pattern (`/users/:p0`): parameters
/// get names of their own, and characters the tree reads as patterns are escaped
pub(super) fn pattern(template: &str) -> String {
    let mut pattern = String::with_capacity(template.len() + 8);
    let mut params = 0;
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        escape(&mut pattern, rest.get(..open).unwrap_or_default());
        let after = rest.get(open..).unwrap_or_default();
        let Some(close) = after.find('}') else {
            escape(&mut pattern, after);
            return pattern;
        };
        let _ = write!(pattern, ":p{params}");
        params += 1;
        rest = after.get(close + 1..).unwrap_or_default();
    }
    escape(&mut pattern, rest);
    pattern
}

fn escape(pattern: &mut String, literal: &str) {
    for c in literal.chars() {
        if matches!(c, ':' | '*' | '+' | '?' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, Method};

    use super::{pattern, Contract, Name, PathPlan, Plan, Refusal};

    fn plan() -> Plan {
        Plan {
            names: vec![
                Name {
                    header: false,
                    name: "limit".into(),
                },
                Name {
                    header: true,
                    name: "x-tenant".into(),
                },
            ],
            paths: vec![
                PathPlan {
                    template: "/users".into(),
                    operations: vec![(1, vec![0]), (4, vec![1])],
                },
                PathPlan {
                    template: "/users/{id}".into(),
                    operations: vec![(1 | 8, vec![])],
                },
                PathPlan {
                    template: "/users/me".into(),
                    operations: vec![(1, vec![])],
                },
                PathPlan {
                    template: "/projects/{name}:run".into(),
                    operations: vec![(4, vec![])],
                },
                PathPlan {
                    template: "/files/{name}.json".into(),
                    operations: vec![(1, vec![])],
                },
            ],
        }
    }

    fn check(
        method: Method,
        path: &str,
        query: Option<&str>,
        headers: &[(&'static str, &str)],
    ) -> Result<(), Refusal> {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(*name, value.parse().unwrap());
        }
        Contract::new(&plan()).check(&method, path, query, &map)
    }

    #[test]
    fn test_paths_and_methods() {
        assert_eq!(check(Method::GET, "/users/42", None, &[]), Ok(()));
        assert_eq!(check(Method::DELETE, "/users/42", None, &[]), Ok(()));
        assert_eq!(check(Method::GET, "/users/me", None, &[]), Ok(()));
        assert_eq!(check(Method::POST, "/projects/p1:run", None, &[]), Ok(()));
        assert_eq!(check(Method::GET, "/files/report.json", None, &[]), Ok(()));
        // `HEAD` with `GET`
        assert_eq!(check(Method::HEAD, "/users/42", None, &[]), Ok(()));

        assert_eq!(
            check(Method::GET, "/accounts", None, &[]),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            check(Method::GET, "/users/42/", None, &[]),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            check(Method::GET, "/files/report.csv", None, &[]),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            check(Method::PUT, "/users/42", None, &[]),
            Err(Refusal::Method("GET, DELETE".into()))
        );
        assert_eq!(
            check(
                Method::from_bytes(b"PURGE").unwrap(),
                "/users/me",
                None,
                &[]
            ),
            Err(Refusal::Method("GET".into()))
        );
    }

    #[test]
    fn test_required_parameters() {
        assert_eq!(
            check(Method::GET, "/users", Some("limit=10&sort=a"), &[]),
            Ok(())
        );
        assert_eq!(
            check(Method::GET, "/users", Some("sort=a&limit"), &[]),
            Ok(())
        );
        assert_eq!(check(Method::GET, "/users", Some("li%6Dit=1"), &[]), Ok(()));
        assert_eq!(
            check(Method::GET, "/users", Some("limits=1"), &[]),
            Err(Refusal::Missing("query parameter", "limit".into()))
        );
        assert_eq!(
            check(Method::GET, "/users", None, &[]),
            Err(Refusal::Missing("query parameter", "limit".into()))
        );
        assert_eq!(
            check(Method::POST, "/users", None, &[("x-tenant", "a")]),
            Ok(())
        );
        assert_eq!(
            check(Method::POST, "/users", None, &[]),
            Err(Refusal::Missing("header", "x-tenant".into()))
        );
    }

    #[test]
    fn test_checking_allocates_nothing() {
        let contract = Contract::new(&plan());
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant", "a".parse().unwrap());
        crate::allocations::assert_no_allocations("checking a request", || {
            contract
                .check(&Method::GET, "/users", Some("sort=a&limit=1"), &headers)
                .is_ok()
                && contract
                    .check(&Method::POST, "/users", None, &headers)
                    .is_ok()
                && contract
                    .check(&Method::GET, "/users/42", None, &headers)
                    .is_ok()
        });
    }

    #[test]
    fn test_patterns() {
        assert_eq!(pattern("/users/{id}"), "/users/:p0");
        assert_eq!(pattern("/a/{x}/b/{y-z}.json"), "/a/:p0/b/:p1.json");
        assert_eq!(pattern("/v1/{name}:run"), "/v1/:p0\\:run");
        assert_eq!(pattern("/a+b/*/c?"), "/a\\+b/\\*/c\\?");
        assert_eq!(pattern("/broken/{id"), "/broken/{id");
    }
}
