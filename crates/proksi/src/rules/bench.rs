//! The cost of evaluating rules (ADR 0007's budget): `cargo test --release -p proksi --bin proksi
//! rules::bench -- --ignored --nocapture`

use std::time::Instant;

use serde_json::json;

use super::{eval::tests::TestRequest, model::Rule, Rules};

/// Rules on what routing already reads: methods, hosts, paths and clients
fn request_line_rules(count: usize) -> Vec<Rule> {
    (0..count)
        .map(|i| {
            let condition = match i % 4 {
                0 => json!({ "and": {
                    "method": { "eq": "POST" },
                    "path": { "prefix": format!("/api/v{i}/") },
                } }),
                1 => json!({ "and": {
                    "host": { "eq": format!("app-{i}.example.com") },
                    "not": { "path": { "eq": "/health" } },
                } }),
                2 => json!({ "and": { "path": { "wildcard": format!("/admin/{i}/*") } } }),
                _ => json!({ "and": {
                    "client": { "in": [format!("10.{}.0.0/16", i % 256)] },
                    "method": { "neq": "GET" },
                } }),
            };
            named(condition, i)
        })
        .collect()
}

fn named(condition: serde_json::Value, i: usize) -> Rule {
    let mut rule = condition;
    rule["name"] = json!(format!("rule {i}"));
    rule["then"] = json!({ "respond": { "status": 403 } });
    Rule(rule)
}

/// Rules that also read headers, a cookie and regular expressions, each rule testing one to
/// three fields
fn rules(count: usize) -> Vec<Rule> {
    (0..count)
        .map(|i| {
            let condition = match i % 5 {
                0 => json!({ "and": {
                    "method": { "eq": "POST" },
                    "path": { "prefix": format!("/api/v{i}/") },
                } }),
                1 => json!({ "and": {
                    "host": { "eq": format!("app-{i}.example.com") },
                    "not": { "path": { "eq": "/health" } },
                } }),
                2 => json!({ "or": {
                    "header": { "x-client": { "eq": format!("legacy-{i}") } },
                    "cookie": { "beta": { "eq": format!("{i}") } },
                } }),
                3 => json!({ "and": {
                    "client": { "in": [format!("10.{}.0.0/16", i % 256)] },
                    "path": { "wildcard": format!("/admin/{i}/*") },
                } }),
                _ => json!({ "and": {
                    "header": { "user-agent": { "matches": format!("(?i)bot-{i}") } },
                } }),
            };
            named(condition, i)
        })
        .collect()
}

/// Nanoseconds to run every rule of a scope on a request matching none of them
fn cost(rules: &[Rule]) -> f64 {
    let count = rules.len();
    let rules = Rules::compile(rules).unwrap().unwrap();
    let request = TestRequest::new("GET", "www.example.com", "/products/42?ref=home")
        .header(
            "user-agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) Safari/605.1.15",
        )
        .header("cookie", "session=abc; theme=dark")
        .header("accept", "text/html");

    let iterations = 2_000_000 / count.max(1) + 20_000;
    let mut matched = 0;
    // Warm up: caches, branch predictors, the CPU's clock
    for _ in 0..iterations {
        matched += rules.matching(&request);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        matched += rules.matching(&request);
    }
    assert_eq!(matched, 0);
    start.elapsed().as_nanos() as f64 / iterations as f64
}

#[test]
#[ignore = "a benchmark: run with --release"]
fn bench_rules() {
    // (rules, budget in nanoseconds), from ADR 0007: rules on the request line, then rules that
    // also read headers (each header, cookie or regular expression costs once per request)
    let mixes: [(&str, fn(usize) -> Vec<Rule>, [f64; 3]); 2] = [
        (
            "method, host, path, client",
            request_line_rules,
            [150.0, 300.0, 1500.0],
        ),
        ("also headers, cookie, regex", rules, [250.0, 600.0, 4000.0]),
    ];
    for (mix, make, budgets) in mixes {
        for (count, budget) in [10, 100, 1000].into_iter().zip(budgets) {
            let nanos = cost(&make(count));
            println!("{mix:>28}, {count:>4} rules: {nanos:>7.1}ns (budget {budget}ns)");
            if !cfg!(debug_assertions) {
                assert!(
                    nanos <= budget,
                    "{mix}, {count} rules: {nanos:.1}ns, over {budget}ns"
                );
            }
        }
    }
}
