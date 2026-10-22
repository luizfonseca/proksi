//! Reading a spec into a [`Plan`]: only what's checked is kept (paths, methods, required query
//! and header parameters). The paths are compiled as they're read, one at a time: a spec is
//! never held whole, whatever its size. Strings are borrowed from the spec's bytes (mapped from
//! its file) where they can be.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

use serde::Deserialize;

use super::contract::{Name, PathPlan, Plan, METHODS};

/// Specs are read with a budget large enough for the largest ones (YAML)
const MAX_YAML_NODES: usize = 500_000_000;

/// The plan of a spec, `OpenAPI` 3 in JSON or YAML
pub(super) fn compile(bytes: &[u8]) -> Result<Plan, String> {
    let json = bytes
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'{');
    let spec: Spec = if json {
        serde_json::from_slice(bytes).map_err(|err| format!("not a JSON spec: {err}"))?
    } else {
        let options = serde_saphyr::options! {
            budget: serde_saphyr::budget! {
                max_nodes: MAX_YAML_NODES,
                max_events: MAX_YAML_NODES,
                max_total_scalar_bytes: usize::MAX,
            },
        };
        serde_saphyr::from_slice_with_options(bytes, options)
            .map_err(|err| format!("not a YAML spec: {err}"))?
    };
    if spec.swagger.is_some() {
        return Err("Swagger 2.0 specs aren't supported: convert it to OpenAPI 3".to_string());
    }
    if !spec
        .openapi
        .as_deref()
        .is_some_and(|version| version.starts_with("3."))
    {
        return Err("not an OpenAPI 3 spec (no `openapi: 3.x`)".to_string());
    }
    Ok(spec.paths.plan(&spec.components.parameters))
}

#[derive(Deserialize)]
struct Spec<'a> {
    #[serde(borrow, default)]
    openapi: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    swagger: Option<Cow<'a, str>>,
    #[serde(default)]
    paths: Paths,
    #[serde(borrow, default)]
    components: Components<'a>,
}

#[derive(Deserialize, Default)]
struct Components<'a> {
    #[serde(borrow, default)]
    parameters: BTreeMap<Cow<'a, str>, Parameter<'a>>,
}

#[derive(Deserialize, Default)]
struct PathItem<'a> {
    #[serde(borrow, default)]
    parameters: Vec<Parameter<'a>>,
    #[serde(borrow)]
    get: Option<Operation<'a>>,
    #[serde(borrow)]
    put: Option<Operation<'a>>,
    #[serde(borrow)]
    post: Option<Operation<'a>>,
    #[serde(borrow)]
    delete: Option<Operation<'a>>,
    #[serde(borrow)]
    options: Option<Operation<'a>>,
    #[serde(borrow)]
    head: Option<Operation<'a>>,
    #[serde(borrow)]
    patch: Option<Operation<'a>>,
    #[serde(borrow)]
    trace: Option<Operation<'a>>,
}

impl<'a> PathItem<'a> {
    fn operations(&self) -> [(u8, Option<&Operation<'a>>); 8] {
        let [get, put, post, delete, options, head, patch, trace] = METHODS.map(|(_, bit)| bit);
        [
            (get, self.get.as_ref()),
            (put, self.put.as_ref()),
            (post, self.post.as_ref()),
            (delete, self.delete.as_ref()),
            (options, self.options.as_ref()),
            (head, self.head.as_ref()),
            (patch, self.patch.as_ref()),
            (trace, self.trace.as_ref()),
        ]
    }
}

#[derive(Deserialize, Default)]
struct Operation<'a> {
    #[serde(borrow, default)]
    parameters: Vec<Parameter<'a>>,
}

#[derive(Deserialize, Default)]
struct Parameter<'a> {
    #[serde(rename = "$ref", borrow, default)]
    reference: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    name: Option<Cow<'a, str>>,
    #[serde(rename = "in", borrow, default)]
    location: Option<Cow<'a, str>>,
    #[serde(default)]
    required: bool,
}

/// A parameter of an operation, as declared
#[derive(Debug, Clone)]
enum Declared {
    /// Query or header: its name's index, and whether it's required
    Here { name: u32, required: bool },
    /// `#/components/parameters/<key>`
    Shared(String),
}

/// A path as read: its template, its parameters, and its operations' (by method)
struct Read {
    template: String,
    shared: Vec<Declared>,
    operations: Vec<(u8, Vec<Declared>)>,
}

/// The paths, compiled as they're read
#[derive(Default)]
struct Paths {
    names: Vec<Name>,
    ids: HashMap<Name, u32>,
    read: Vec<Read>,
}

impl Paths {
    fn declare(&mut self, parameter: &Parameter) -> Option<Declared> {
        if let Some(reference) = &parameter.reference {
            let key = reference.strip_prefix("#/components/parameters/")?;
            return Some(Declared::Shared(key.to_string()));
        }
        let header = match parameter.location.as_deref()? {
            "query" => false,
            "header" => true,
            // Path parameters are in the template; cookies aren't checked
            _ => return None,
        };
        let name = Name {
            header,
            // Header names are case-insensitive
            name: if header {
                parameter.name.as_deref()?.to_ascii_lowercase()
            } else {
                parameter.name.as_deref()?.to_string()
            },
        };
        let next = u32::try_from(self.names.len()).ok()?;
        let id = *self.ids.entry(name.clone()).or_insert_with(|| {
            self.names.push(name);
            next
        });
        Some(Declared::Here {
            name: id,
            required: parameter.required,
        })
    }

    fn add(&mut self, template: &str, item: &PathItem) {
        let shared: Vec<Declared> = item
            .parameters
            .iter()
            .filter_map(|p| self.declare(p))
            .collect();
        let operations = item
            .operations()
            .into_iter()
            .filter_map(|(bit, operation)| {
                let declared = operation?
                    .parameters
                    .iter()
                    .filter_map(|p| self.declare(p))
                    .collect();
                Some((bit, declared))
            })
            .collect();
        self.read.push(Read {
            template: template.to_string(),
            shared,
            operations,
        });
    }

    /// The plan, with shared parameters resolved: an operation's parameter replaces the
    /// path's with its name and place
    fn plan(mut self, components: &BTreeMap<Cow<str>, Parameter>) -> Plan {
        let shared: HashMap<String, Declared> = components
            .iter()
            .filter(|(_, parameter)| parameter.reference.is_none())
            .filter_map(|(key, parameter)| Some((key.to_string(), self.declare(parameter)?)))
            .collect();
        let resolve = |declared: &Declared| match declared {
            Declared::Here { name, required } => Some((*name, *required)),
            Declared::Shared(key) => match shared.get(key)? {
                Declared::Here { name, required } => Some((*name, *required)),
                Declared::Shared(_) => None,
            },
        };
        let paths = std::mem::take(&mut self.read)
            .into_iter()
            .map(
                |Read {
                     template,
                     shared: path_level,
                     operations,
                 }| {
                    let operations = operations
                        .into_iter()
                        .map(|(bit, declared)| {
                            let mut effective: BTreeMap<u32, bool> = BTreeMap::new();
                            for (name, required) in
                                path_level.iter().chain(&declared).filter_map(resolve)
                            {
                                effective.insert(name, required);
                            }
                            let required = effective
                                .into_iter()
                                .filter(|(_, required)| *required)
                                .map(|(name, _)| name)
                                .collect();
                            (bit, required)
                        })
                        .collect();
                    PathPlan {
                        template,
                        operations,
                    }
                },
            )
            .collect();
        Plan {
            names: self.names,
            paths,
        }
    }
}

impl<'de> Deserialize<'de> for Paths {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Paths;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("the paths of a spec")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Paths, A::Error> {
                let mut paths = Paths::default();
                while let Some((template, item)) =
                    map.next_entry::<Cow<'de, str>, PathItem<'de>>()?
                {
                    paths.add(&template, &item);
                }
                Ok(paths)
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::compile;
    use crate::openapi::contract::{Name, PathPlan};

    const SPEC: &str = r##"{
      "openapi": "3.1.0",
      "info": { "title": "Shop", "version": "1", "description": "ignored" },
      "paths": {
        "/orders": {
          "parameters": [
            { "$ref": "#/components/parameters/Tenant" },
            { "name": "debug", "in": "query", "required": true }
          ],
          "get": {
            "parameters": [
              { "name": "limit", "in": "query", "required": true, "schema": { "type": "integer" } },
              { "name": "debug", "in": "query", "required": false },
              { "name": "id", "in": "path", "required": true },
              { "name": "session", "in": "cookie", "required": true }
            ],
            "responses": { "200": { "description": "a list", "content": { "application/json": { "example": [1, 2] } } } }
          },
          "post": { "requestBody": { "content": {} }, "responses": {} }
        }
      },
      "components": {
        "parameters": { "Tenant": { "name": "X-Tenant", "in": "header", "required": true } },
        "schemas": { "Order": { "type": "object" } }
      }
    }"##;

    #[test]
    fn test_only_the_contract_is_kept() {
        let plan = compile(SPEC.as_bytes()).unwrap();
        let name = |header, name: &str| Name {
            header,
            name: name.to_string(),
        };
        let id = |n: &Name| {
            u32::try_from(plan.names.iter().position(|known| known == n).unwrap()).unwrap()
        };
        let (tenant, debug, limit) = (
            id(&name(true, "x-tenant")),
            id(&name(false, "debug")),
            id(&name(false, "limit")),
        );
        let mut get = vec![tenant, limit];
        get.sort_unstable();
        let mut post = vec![tenant, debug];
        post.sort_unstable();
        assert_eq!(
            plan.paths,
            // `GET` overrides `debug` as not required; `POST` keeps the path's
            vec![PathPlan {
                template: "/orders".into(),
                operations: vec![(1, get), (4, post)]
            }]
        );
    }

    #[test]
    fn test_yaml_is_read_as_json() {
        let yaml = "openapi: 3.0.3\npaths:\n  /orders/{id}:\n    get:\n      parameters:\n        - name: expand\n          in: query\n          required: true\n";
        let plan = compile(yaml.as_bytes()).unwrap();
        assert_eq!(plan.paths[0].template, "/orders/{id}");
        assert_eq!(plan.paths[0].operations.len(), 1);
        assert_eq!(
            plan.names,
            vec![Name {
                header: false,
                name: "expand".into()
            }]
        );
    }

    #[test]
    fn test_what_isnt_a_spec_is_refused() {
        assert!(compile(br#"{"swagger": "2.0", "paths": {}}"#)
            .unwrap_err()
            .starts_with("Swagger 2.0"));
        assert!(compile(br#"{"paths": {}}"#)
            .unwrap_err()
            .starts_with("not an OpenAPI 3"));
        assert!(compile(b"{ not json")
            .unwrap_err()
            .starts_with("not a JSON spec"));
        assert!(compile(b"- just\n- a list\n")
            .unwrap_err()
            .starts_with("not a YAML spec"));
    }
}
