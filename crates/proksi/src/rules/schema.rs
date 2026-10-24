//! What rules accept (ADR 0007), for the admin UI's rule builder: the fields a condition tests,
//! the operators each takes and the values they need, and the actions of `then`. A test compiles
//! every field with every operator, so this can't say more or less than the compiler accepts.

use serde::Serialize;
use serde_json::Value;

/// A field a condition tests
#[derive(Debug, Serialize)]
pub struct Field {
    pub name: &'static str,
    /// Tested by name (`header "x-api-key"`)
    pub named: bool,
    pub description: &'static str,
    pub operators: &'static [&'static str],
}

/// What an operator compares a field with
#[derive(Debug, Serialize)]
pub struct Operator {
    pub name: &'static str,
    /// `text` (one value), `list` (one or more) or `bool`
    pub value: &'static str,
    pub description: &'static str,
}

const EQUALITY: &[&str] = &["eq", "neq", "in", "not_in"];
const TEXT: &[&str] = &[
    "eq", "neq", "in", "not_in", "prefix", "suffix", "contains", "wildcard", "matches",
];
const NAMED_TEXT: &[&str] = &[
    "eq", "neq", "in", "not_in", "prefix", "suffix", "contains", "wildcard", "matches", "exists",
];

pub const FIELDS: &[Field] = &[
    Field {
        name: "method",
        named: false,
        description: "The method (GET, POST...), ignoring case",
        operators: EQUALITY,
    },
    Field {
        name: "host",
        named: false,
        description: "The host the request is for, without its port, ignoring case",
        operators: TEXT,
    },
    Field {
        name: "path",
        named: false,
        description: "The path, as used to choose locations",
        operators: TEXT,
    },
    Field {
        name: "query",
        named: true,
        description: "The value of a query parameter, decoded",
        operators: NAMED_TEXT,
    },
    Field {
        name: "header",
        named: true,
        description: "The value of a request header; names ignore case",
        operators: NAMED_TEXT,
    },
    Field {
        name: "cookie",
        named: true,
        description: "The value of a cookie",
        operators: NAMED_TEXT,
    },
    Field {
        name: "client",
        named: false,
        description: "The client's address (behind trusted proxies, the one they forwarded)",
        operators: EQUALITY,
    },
];

pub const OPERATORS: &[Operator] = &[
    Operator { name: "eq", value: "text", description: "is" },
    Operator { name: "neq", value: "text", description: "is not" },
    Operator { name: "in", value: "list", description: "is one of" },
    Operator { name: "not_in", value: "list", description: "is none of" },
    Operator { name: "prefix", value: "list", description: "starts with one of" },
    Operator { name: "suffix", value: "list", description: "ends with one of" },
    Operator { name: "contains", value: "list", description: "contains one of" },
    Operator { name: "wildcard", value: "text", description: "matches (* is any text)" },
    Operator { name: "matches", value: "text", description: "matches the regular expression" },
    Operator { name: "exists", value: "bool", description: "is there" },
];

/// Fields, operators, how conditions combine, and the actions of `then`
pub fn schema() -> Value {
    serde_json::json!({
        "fields": FIELDS,
        "operators": OPERATORS,
        "groups": [
            { "name": "and", "description": "all of" },
            { "name": "or", "description": "any of" },
            { "name": "not", "description": "none of" },
        ],
        "actions": super::actions::schema(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Field, Operator, FIELDS, OPERATORS};
    use crate::rules::{Rule, Rules};

    /// A value each kind of field takes, for `operator`
    fn value(field: &str, operator: &str) -> serde_json::Value {
        let one = match field {
            "method" => json!("GET"),
            "client" => json!("10.0.0.1"),
            _ => json!("x"),
        };
        match OPERATORS.iter().find(|known| known.name == operator).map(|known| known.value) {
            Some("list") => json!([one]),
            Some("bool") => json!(true),
            _ => one,
        }
    }

    /// A condition testing `field` with `operator`
    fn condition(field: &Field, operator: &Operator) -> serde_json::Value {
        let test = json!({ operator.name: value(field.name, operator.name) });
        if field.named {
            json!({ field.name: { "x-name": test } })
        } else {
            json!({ field.name: test })
        }
    }

    #[test]
    fn test_fields_take_exactly_their_operators() {
        for field in FIELDS {
            for operator in OPERATORS {
                let condition = condition(field, operator);
                let rule = Rule(json!({ "name": "a", "and": condition, "then": { "respond": {} } }));

                let compiled = Rules::compile(&[rule], "");

                let listed = field.operators.contains(&operator.name);
                assert_eq!(
                    compiled.is_ok(),
                    listed,
                    "{} {}: {:?}",
                    field.name,
                    operator.name,
                    compiled.err()
                );
            }
        }
    }

    #[test]
    fn test_actions() {
        let schema = super::schema();
        let actions = schema["actions"].as_object().unwrap();
        for name in ["respond", "redirect", "rate_limit", "basic_auth", "headers", "method"] {
            assert!(actions.contains_key(name), "{name}");
        }
        // Every action is one `then` takes
        for name in actions.keys() {
            let rule = Rule(json!({ "name": "a", "and": { "method": { "eq": "GET" } }, "then": { name: {} } }));
            let err = Rules::compile(&[rule], "").err().unwrap_or_default();
            assert!(!err.contains("unknown field"), "{name}: {err}");
        }
    }
}
