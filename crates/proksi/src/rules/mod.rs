//! Rules (ADR 0007): a condition on a request, and what to do when it holds.
//!
//! - [`model`]: rules as written (HCL, YAML, JSON)
//! - [`compile`]: from that tree to tests grouped by field, and conditions over them
//! - [`fields`]: reading a request's fields once, and running each field's tests together
//! - [`eval`]: the results of a request's tests as bits, and `and`/`or`/`not` on them
//! - [`index`]: the rules a request can match, from the method, host and path they require
//! - [`actions`]: `then { ... }`
//!
//! The proxy calls [`Rules::request`] for the server's, a host's and a location's rules, and
//! [`Applied`] where the blocks of matched rules take part in later stages of the request.

use std::{borrow::Cow, net::IpAddr, sync::Arc, sync::OnceLock};

use bytes::Bytes;
use http::{HeaderName, Method};
use pingora::{
    http::{RequestHeader, ResponseHeader},
    proxy::Session,
};

use crate::proxy_server::https_proxy::RouterContext;

mod actions;
mod compile;
mod eval;
mod fields;
mod index;
mod model;
pub mod schema;
mod text_set;

#[cfg(test)]
mod bench;

pub use model::{one_or_many, unlabel_rules, Rule};

use actions::Then;
use compile::Compiled;
use fields::Request;

/// The compiled rules of a scope (the server, a host or a location)
pub struct Rules {
    compiled: Compiled,
    /// For scopes with many rules
    index: Option<index::Index>,
    /// As written, for the admin API (empty without it)
    source: Vec<Rule>,
}

impl Rules {
    /// `None` without rules: a scope without rules costs nothing
    pub fn compile(rules: &[Rule]) -> Result<Option<Arc<Self>>, String> {
        if rules.is_empty() {
            return Ok(None);
        }
        let compiled = compile::compile(rules)?;
        let index = index::Index::build(&compiled);
        Ok(Some(Arc::new(Self {
            compiled,
            index,
            source: if crate::admin::enabled() {
                rules.to_vec()
            } else {
                vec![]
            },
        })))
    }

    /// The rules as written
    pub fn source(&self) -> &[Rule] {
        &self.source
    }

    /// The rules `request` can match
    fn candidates(&self, request: &impl Request) -> index::RuleSet {
        match &self.index {
            Some(index) => {
                let host = fields::lowercase(request.host());
                index.candidates(request.method(), &host, request.path())
            }
            None => index::all(self.compiled.rules.len()),
        }
    }

    /// The rules `request` matches, as `request` would run them (without their `then`)
    #[cfg(test)]
    fn matching(&self, request: &impl Request) -> usize {
        let compiled = &self.compiled;
        let mut state = eval::State::new(compiled);
        let candidates = self.candidates(request);
        let mut matched = 0;
        let mut from = 0;
        while let Some(rule) = candidates.next(from) {
            from = rule + 1;
            if let Some(rule) = compiled.rules.get(rule) {
                matched += usize::from(state.holds(compiled, &rule.condition, request));
            }
        }
        matched
    }

    /// Runs the rules on a request, in order: `true` when one answered it (a redirect, a
    /// refusal...), which ends the request. Rules that change the request change it for the next
    /// ones, and the blocks of matched rules are kept for the later stages of the request.
    pub async fn request(&self, session: &mut Session, ctx: &mut RouterContext) -> bool {
        let compiled = &self.compiled;
        let mut state = eval::State::new(compiled);
        let mut normalized = normalized_path(session);
        let mut candidates = self.candidates(&ProxiedRequest {
            session,
            host: &ctx.host,
            normalized: normalized.as_deref(),
        });
        let mut from = 0;
        while let Some(index) = candidates.next(from) {
            from = index + 1;
            let Some(rule) = compiled.rules.get(index) else {
                break;
            };
            let holds = {
                let request = ProxiedRequest {
                    session,
                    host: &ctx.host,
                    normalized: normalized.as_deref(),
                };
                state.holds(compiled, &rule.condition, &request)
            };
            if !holds {
                continue;
            }
            if rule.then.change_request(session.req_header_mut()) {
                // The next rules see the changed request
                state.forget();
                normalized = normalized_path(session);
                candidates = self.candidates(&ProxiedRequest {
                    session,
                    host: &ctx.host,
                    normalized: normalized.as_deref(),
                });
            }
            if let Some(split) = &rule.then.split {
                ctx.traffic_split = Some(split.clone());
            }
            if let Some(mirror) = rule.then.mirror {
                ctx.mirror_rule = Some(mirror);
            }
            if let Some(retry) = rule.then.retry {
                ctx.retry_rule = Some(retry);
            }
            if rule.then.blocks.request(session, ctx).await.is_some() {
                return true;
            }
            ctx.rules.0.push(rule.then.clone());
        }
        false
    }
}

static SERVER: OnceLock<Option<Arc<Rules>>> = OnceLock::new();

/// Sets the server's rules (`server { rule ... }`), once, when proksi starts
pub fn set_server_rules(rules: &[Rule]) -> Result<(), String> {
    let _ = SERVER.set(Rules::compile(rules)?);
    Ok(())
}

/// The server's rules, run for every request before its host is chosen
pub fn server() -> Option<&'static Arc<Rules>> {
    SERVER.get()?.as_ref()
}

/// The `then` of the rules a request matched, whose blocks also take part in its later stages
#[derive(Default)]
pub struct Applied(Vec<Arc<Then>>);

impl Applied {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Each part of the request body, as it's proxied: an error ends the request
pub fn request_body(body: Option<&Bytes>, ctx: &mut RouterContext) -> pingora::Result<()> {
    if ctx.rules.is_empty() {
        return Ok(());
    }
    let applied = std::mem::take(&mut ctx.rules);
    let result = applied
        .0
        .iter()
        .try_for_each(|then| then.blocks.request_body(body, ctx).map_err(|(_, err)| err));
    ctx.rules = applied;
    result
}

/// Changes the request sent to the upstream
pub async fn upstream_request(
    session: &mut Session,
    request: &mut RequestHeader,
    ctx: &mut RouterContext,
) {
    if ctx.rules.is_empty() {
        return;
    }
    let applied = std::mem::take(&mut ctx.rules);
    for then in &applied.0 {
        then.blocks.upstream_request(session, request, ctx).await;
    }
    ctx.rules = applied;
}

/// Changes the upstream's response, before it's cached
pub fn upstream_response(
    session: &mut Session,
    response: &mut ResponseHeader,
    ctx: &mut RouterContext,
) {
    if ctx.rules.is_empty() {
        return;
    }
    let applied = std::mem::take(&mut ctx.rules);
    for then in applied.0.iter().rev() {
        then.blocks.upstream_response(session, response, ctx);
    }
    ctx.rules = applied;
}

/// Changes the response sent to the client, innermost rule first (as for blocks)
pub async fn response(
    session: &mut Session,
    response: &mut ResponseHeader,
    ctx: &mut RouterContext,
) {
    if ctx.rules.is_empty() {
        return;
    }
    let applied = std::mem::take(&mut ctx.rules);
    for then in applied.0.iter().rev() {
        then.blocks.response(session, response, ctx).await;
    }
    ctx.rules = applied;
}

/// The path of `session` normalized, when that changes it: worked out once per request, not
/// for each test on the path
fn normalized_path(session: &Session) -> Option<String> {
    match crate::routing::paths::normalize_path(session.req_header().uri.path()) {
        Cow::Owned(path) => Some(path),
        Cow::Borrowed(_) => None,
    }
}

/// A request being proxied, as rules read it
struct ProxiedRequest<'a> {
    session: &'a Session,
    host: &'a str,
    /// Its path normalized, when that changes it
    normalized: Option<&'a str>,
}

impl Request for ProxiedRequest<'_> {
    fn method(&self) -> &Method {
        &self.session.req_header().method
    }

    fn host(&self) -> &str {
        self.host
    }

    fn path(&self) -> &str {
        self.normalized
            .unwrap_or_else(|| self.session.req_header().uri.path())
    }

    fn query(&self) -> Option<&str> {
        self.session.req_header().uri.query()
    }

    fn header(&self, name: &HeaderName) -> Option<Cow<'_, str>> {
        let mut values = self.session.req_header().headers.get_all(name).iter();
        let first = values.next()?.to_str().ok()?;
        let Some(second) = values.next() else {
            return Some(Cow::Borrowed(first));
        };
        let mut joined = format!("{first}, {}", second.to_str().unwrap_or_default());
        for value in values {
            joined.push_str(", ");
            joined.push_str(value.to_str().unwrap_or_default());
        }
        Some(Cow::Owned(joined))
    }

    fn client(&self) -> Option<IpAddr> {
        crate::proxy_server::client::of_session(self.session)
    }
}

#[cfg(test)]
mod tests {
    use figment::providers::Format;

    use crate::config::hcl::HclFormat;

    use super::{one_or_many, Rule, Rules};

    #[test]
    fn test_running_rules_allocates_nothing() {
        let named = std::cell::Cell::new(0);
        let rule = |condition: serde_json::Value| {
            let mut rule = condition;
            named.set(named.get() + 1);
            rule["name"] = serde_json::json!(format!("rule {}", named.get()));
            rule["then"] = serde_json::json!({ "respond": { "status": 403 } });
            Rule(rule)
        };
        let rules = [
            rule(serde_json::json!({ "and": {
                "method": { "eq": "POST" },
                "path": { "prefix": "/api/" },
            } })),
            rule(serde_json::json!({ "and": {
                "host": { "eq": "www.example.com" },
                "not": { "path": { "eq": "/health" } },
            } })),
            rule(serde_json::json!({ "or": {
                "header": { "x-client": { "eq": "legacy" } },
                "cookie": { "beta": { "eq": "1" } },
            } })),
            rule(serde_json::json!({ "and": {
                "client": { "in": ["10.0.0.0/8"] },
                "query": { "ref": { "eq": "home" } },
            } })),
        ];
        let rules = Rules::compile(&rules).unwrap().unwrap();
        let request =
            super::eval::tests::TestRequest::new("GET", "www.example.com", "/products/42?ref=home")
                .header("x-client", "web")
                .header("cookie", "session=abc; theme=dark");

        crate::allocations::assert_no_allocations("running rules", || rules.matching(&request));

        // Scopes with many rules: more tests than fit in a word
        let many = super::bench::rules(100);
        let many = Rules::compile(&many).unwrap().unwrap();
        crate::allocations::assert_no_allocations("running 100 rules", || many.matching(&request));

        // Long lists of values, matched in a time that doesn't grow with them
        let probes = (0..1000).map(|i| format!("/probe-{i}")).collect::<Vec<_>>();
        let lists = [
            rule(serde_json::json!({ "and": { "path": { "contains": probes } } })),
            rule(serde_json::json!({ "and": { "path": { "prefix": probes } } })),
            rule(serde_json::json!({ "and": { "path": { "suffix": probes } } })),
            rule(serde_json::json!({ "and": { "path": { "in": probes } } })),
            rule(serde_json::json!({ "and": { "client": { "in": (0..1000)
                .map(|i| format!("10.{}.{}.0/24", i / 256, i % 256))
                .collect::<Vec<_>>() } } })),
        ];
        let lists = Rules::compile(&lists).unwrap().unwrap();
        crate::allocations::assert_no_allocations("lists of 1000 values", || {
            lists.matching(&request)
        });
    }

    #[test]
    fn test_rules_as_written_with_the_admin_api() {
        crate::admin::enable();
        let rules = vec![Rule(serde_json::json!({
            "name": "writes",
            "and": { "method": { "eq": "POST" } },
            "then": { "respond": { "status": 403 } },
        }))];

        let compiled = Rules::compile(&rules).unwrap().unwrap();

        assert_eq!(compiled.source(), rules.as_slice());
    }

    /// The pages of the Rules section, whose examples must load
    const PAGES: [(&str, &str); 5] = [
        ("index", include_str!("../../../../docs/rules/index.mdx")),
        (
            "conditions",
            include_str!("../../../../docs/rules/conditions.mdx"),
        ),
        (
            "actions",
            include_str!("../../../../docs/rules/actions.mdx"),
        ),
        (
            "examples",
            include_str!("../../../../docs/rules/examples.mdx"),
        ),
        (
            "performance",
            include_str!("../../../../docs/rules/performance.mdx"),
        ),
    ];

    /// Other pages with rules among their examples
    const OTHER_PAGES: [(&str, &str); 2] = [
        ("README", include_str!("../../../../README.md")),
        (
            "blocking-url-scanners",
            include_str!("../../../../docs/examples/blocking-url-scanners.mdx"),
        ),
    ];

    /// The HCL examples of a page
    fn examples(text: &str) -> impl Iterator<Item = &str> {
        text.split("```hcl\n")
            .skip(1)
            .map(|example| example.split("```").next().unwrap_or_default())
    }

    /// Every `rule` of a configuration, wherever it is
    fn rules(value: &serde_json::Value, found: &mut Vec<Vec<super::Rule>>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    rules_at(key, value, found);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| rules(item, found)),
            _ => {}
        }
    }

    /// The rules of `key`, or the ones within it
    fn rules_at(key: &str, value: &serde_json::Value, found: &mut Vec<Vec<super::Rule>>) {
        if key == "rule" {
            found.push(one_or_many(value.clone()).unwrap());
        } else {
            rules(value, found);
        }
    }

    /// Compiles the rules of an HCL example: how many scopes have rules
    fn load(page: &str, example: &str) -> usize {
        let value: serde_json::Value =
            HclFormat::from_str(example).unwrap_or_else(|err| panic!("{page}: {err}\n{example}"));
        let mut found = vec![];
        rules(&value, &mut found);
        for scope in &found {
            if let Err(err) = Rules::compile(scope) {
                panic!("{page}: {err}\n{example}");
            }
        }
        found.len()
    }

    #[test]
    fn test_the_examples_of_the_documentation_load() {
        let mut count = 0;
        for (page, text) in PAGES {
            for example in examples(text) {
                assert!(
                    load(page, example) > 0,
                    "{page}: an example without rules\n{example}"
                );
                count += 1;
            }
        }
        assert!(count >= 10, "{count} examples");

        for (page, text) in OTHER_PAGES {
            for example in examples(text).filter(|example| example.contains("rule \"")) {
                load(page, example);
            }
        }
    }
}
