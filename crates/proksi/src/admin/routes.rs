//! `/api/v1/routes`: hosts written to the store (ADR 0004), applied by every instance through
//! the store sync (ADR 0001)

use serde_json::{json, Value};

use crate::{
    config::{blocks::HostBlock, host::Host},
    stores::{store_trait::Store, stored_route::StoredRoute},
};

/// Why a request was refused
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// Invalid input (400)
    Invalid(String),
    /// The store couldn't be reached (503)
    Unavailable(String),
}

/// The routes in the store, and the ones this version can't read
pub async fn list(store: &dyn Store) -> Result<Value, Refused> {
    let snapshot = store
        .routes()
        .await
        .map_err(|err| Refused::Unavailable(err.to_string()))?;
    let (routes, unreadable): (Vec<_>, Vec<_>) =
        snapshot.routes.into_iter().partition(Result::is_ok);
    Ok(json!({
        "revision": snapshot.revision,
        "routes": routes.into_iter().flatten().collect::<Vec<_>>(),
        "unreadable": unreadable.into_iter().filter_map(Result::err).collect::<Vec<_>>(),
    }))
}

/// Writes the host `name` to the store, from the stored format (JSON) or a `host` block's body
/// (`Content-Type: text/hcl`). Returns the new revision.
pub async fn put(
    store: &dyn Store,
    name: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<Value, Refused> {
    let host = parse(&name.to_lowercase(), content_type, body).map_err(Refused::Invalid)?;
    let route = StoredRoute::new(host);
    let key = route.key();
    let revision = store
        .put_route(route)
        .await
        .map_err(|err| Refused::Unavailable(err.to_string()))?;
    crate::routing::store_source::sync_now();
    Ok(json!({ "host": key, "revision": revision }))
}

/// Removes the host `name` from the store. Returns the (possibly unchanged) revision.
pub async fn delete(store: &dyn Store, name: &str) -> Result<Value, Refused> {
    let name = name.to_lowercase();
    let revision = store
        .delete_route(&name)
        .await
        .map_err(|err| Refused::Unavailable(err.to_string()))?;
    crate::routing::store_source::sync_now();
    Ok(json!({ "host": name, "revision": revision }))
}

/// A host from a request body, validated like routes from any other source
fn parse(name: &str, content_type: Option<&str>, body: &[u8]) -> Result<Host, String> {
    let hcl = content_type.is_some_and(|content_type| content_type.contains("hcl"));
    let host = if hcl {
        let body = std::str::from_utf8(body).map_err(|err| format!("invalid HCL: {err}"))?;
        let block = <crate::config::hcl::HclFormat as figment::providers::Format>::from_str::<
            HostBlock,
        >(body)
        .map_err(|err| format!("invalid HCL: {err}"))?;
        block.to_host(name)?
    } else {
        let mut value: Value =
            serde_json::from_slice(body).map_err(|err| format!("invalid JSON: {err}"))?;
        // The name comes from the path when the body doesn't have one
        if let Some(object) = value.as_object_mut() {
            object.entry("name").or_insert_with(|| json!(name));
        }
        serde_json::from_value::<Host>(value).map_err(|err| format!("invalid JSON: {err}"))?
    };

    if !host.name.eq_ignore_ascii_case(name) {
        return Err(format!(
            "the body is for host \"{}\", not \"{name}\"",
            host.name
        ));
    }
    // Ports are opened when proksi starts; dynamic hosts use the default ones (ADR 0001)
    if !host.listen.is_empty() {
        return Err(
            "`listen` can't be set for hosts in the store: they're served on the default addresses"
                .to_string(),
        );
    }
    crate::routing::validate::validate_host(&host)?;
    Ok(host)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::stores::{store_trait::Store, MemoryStore};

    use super::{delete, list, parse, put, Refused};

    const JSON: &str = r#"{
        "name": "api.test",
        "locations": [{ "path": "/", "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }] }]
    }"#;

    const HCL: &str = r#"
location "/" {
  rate_limit {
    requests = 10
    per      = "1m"
  }
  upstream "10.0.0.1:3000" {}
}
"#;

    #[test]
    fn test_parse_json_and_hcl() {
        let from_json = parse("api.test", Some("application/json"), JSON.as_bytes()).unwrap();
        assert_eq!(from_json.name, "api.test");
        assert_eq!(from_json.locations[0].upstreams[0].port, 3000);

        // The name comes from the path
        let without_name = JSON.replace(r#""name": "api.test","#, "");
        let host = parse("api.test", None, without_name.as_bytes()).unwrap();
        assert_eq!(host.name, "api.test");

        let from_hcl = parse("api.test", Some("text/hcl; charset=utf-8"), HCL.as_bytes()).unwrap();
        assert_eq!(from_hcl.name, "api.test");
        assert_eq!(from_hcl.locations[0].path, "/");
        assert_eq!(
            from_hcl.locations[0].plugins.as_ref().unwrap()[0].name,
            "rate_limit"
        );
    }

    #[test]
    fn test_invalid_hosts() {
        let cases = [
            (JSON.replace("api.test", "other.test"), "the body is for host \"other.test\", not \"api.test\""),
            ("{ not json".to_string(), "invalid JSON: "),
            (
                JSON.replace("3000", "0"),
                "location \"/\": upstreams[0].port must be greater than 0",
            ),
            (
                JSON.replace("\"locations\"", "\"listen\": [\"0.0.0.0:8443\"], \"locations\""),
                "`listen` can't be set for hosts in the store: they're served on the default addresses",
            ),
        ];
        for (body, expected) in cases {
            let err = parse("api.test", None, body.as_bytes()).unwrap_err();
            assert!(err.starts_with(expected), "{err}");
        }
        let err = parse("api.test", Some("text/hcl"), b"locaton \"/\" {}").unwrap_err();
        assert!(err.starts_with("invalid HCL: "), "{err}");
    }

    #[tokio::test]
    async fn test_put_list_and_delete() {
        let store = MemoryStore::new();

        let written = put(&store, "API.test", None, JSON.as_bytes()).await.unwrap();
        assert_eq!(written, json!({ "host": "api.test", "revision": 1 }));
        assert!(store.routes().await.unwrap().routes[0].is_ok());

        let listed = list(&store).await.unwrap();
        assert_eq!(listed["revision"], 1);
        assert_eq!(listed["routes"][0]["host"]["name"], "api.test");
        assert_eq!(listed["routes"][0]["schema_version"], 2);
        assert_eq!(listed["unreadable"], json!([]));

        assert_eq!(
            put(&store, "api.test", None, b"{").await.map(|_| ()),
            Err(Refused::Invalid(
                "invalid JSON: EOF while parsing an object at line 1 column 1".to_string()
            ))
        );

        let deleted = delete(&store, "api.test").await.unwrap();
        assert_eq!(deleted, json!({ "host": "api.test", "revision": 2 }));
        assert_eq!(list(&store).await.unwrap()["routes"], json!([]));
    }
}
