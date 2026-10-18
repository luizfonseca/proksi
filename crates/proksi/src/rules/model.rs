//! Rules as written: the same tree from HCL (`rule "name" { ... }`), YAML and JSON

use serde::{Deserialize, Deserializer, Serialize};

/// A rule as written: its `name`, one condition and `then`. Kept as the tree it was read as, and
/// checked when compiled, so errors can point at what's wrong in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rule(pub serde_json::Value);

/// The rules of a scope, in the order they're written: HCL gives one `rule` block as an object
/// and several as a list
pub fn one_or_many<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Rule>, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Array(rules) => rules.into_iter().map(Rule).collect(),
        serde_json::Value::Null => vec![],
        rule => vec![Rule(rule)],
    })
}

/// `rule "<name>" { ... }` becomes `rule { name = "<name>" ... }`, so that rules read as a list
/// in the order they're written (labeled blocks are read as a map, ordered by name)
pub fn unlabel_rules(body: &mut hcl::Body) {
    for structure in &mut body.0 {
        let hcl::Structure::Block(block) = structure else {
            continue;
        };
        if block.identifier.as_str() == "rule" && block.labels.len() == 1 {
            let name = block.labels.remove(0).into_inner();
            block.body.0.insert(
                0,
                hcl::Structure::Attribute(hcl::Attribute::new("name", name)),
            );
        }
        unlabel_rules(&mut block.body);
    }
}

#[cfg(test)]
mod tests {
    use super::{unlabel_rules, Rule};

    #[test]
    fn test_rules_keep_their_order() {
        let mut body: hcl::Body = r#"
rule "b" {
  method { eq = "GET" }
}
host "a.test" {
  rule "z" {
    method { eq = "GET" }
  }
  rule "a" {
    method { eq = "POST" }
  }
}
"#
        .parse()
        .unwrap();
        unlabel_rules(&mut body);
        let value: serde_json::Value = hcl::from_body(body).unwrap();

        assert_eq!(value["rule"]["name"], "b");
        let names = value["host"]["a.test"]["rule"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["z", "a"]);

        #[derive(serde::Deserialize)]
        struct Scope {
            #[serde(deserialize_with = "super::one_or_many")]
            rule: Vec<Rule>,
        }
        let one: Scope = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(one.rule.len(), 1);
        let two: Scope = serde_json::from_value(value["host"]["a.test"].clone()).unwrap();
        assert_eq!(two.rule.len(), 2);
    }
}
