//! `/api/v1/routes`: hosts written to the store (ADR 0004), applied by every instance through
//! the store sync (ADR 0001). Hosts are JSON, in and out.

use serde::Serialize;

use crate::{
    config::host::Host,
    stores::{
        store_trait::Store,
        stored_route::{StoredRoute, StoredRouteError},
    },
};

/// Why a request was refused
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// Invalid input (400)
    Invalid(String),
    /// The store couldn't be reached (503)
    Unavailable(String),
    /// No such host in the store (404)
    NotFound(String),
}

/// `GET /api/v1/routes`
#[derive(Debug, Serialize)]
pub struct RouteList {
    pub revision: u64,
    pub routes: Vec<StoredRoute>,
    /// Routes this version can't read
    pub unreadable: Vec<StoredRouteError>,
}

/// `GET /api/v1/routes/<host>`
#[derive(Debug, Serialize)]
pub struct OneRoute {
    pub host: Host,
    /// The store's revision when it was read
    pub revision: u64,
}

/// `PUT` and `DELETE /api/v1/routes/<host>`
#[derive(Debug, PartialEq, Serialize)]
pub struct Written {
    pub host: String,
    /// The store's revision after the change
    pub revision: u64,
}

/// The routes in the store, and the ones this version can't read
pub async fn list(store: &dyn Store) -> Result<RouteList, Refused> {
    let snapshot = store.routes().await.map_err(unavailable)?;
    let mut routes = RouteList {
        revision: snapshot.revision,
        routes: vec![],
        unreadable: vec![],
    };
    for route in snapshot.routes {
        match route {
            Ok(route) => routes.routes.push(route),
            Err(err) => routes.unreadable.push(err),
        }
    }
    Ok(routes)
}

/// The host `name`, as stored
pub async fn get(store: &dyn Store, name: &str) -> Result<OneRoute, Refused> {
    let name = name.to_lowercase();
    let snapshot = store.routes().await.map_err(unavailable)?;
    let route = snapshot.routes.into_iter().find(|route| match route {
        Ok(route) => route.key() == name,
        Err(err) => err.host == name,
    });
    match route {
        Some(Ok(route)) => Ok(OneRoute {
            host: route.host,
            revision: snapshot.revision,
        }),
        Some(Err(err)) => Err(Refused::Unavailable(err.error)),
        None => Err(Refused::NotFound(format!(
            "no host \"{name}\" in the store"
        ))),
    }
}

/// Writes the host `name` to the store
pub async fn put(store: &dyn Store, name: &str, body: &[u8]) -> Result<Written, Refused> {
    let host = parse(&name.to_lowercase(), body).map_err(Refused::Invalid)?;
    let route = StoredRoute::new(host);
    let host = route.key();
    let revision = store.put_route(route).await.map_err(unavailable)?;
    crate::routing::store_source::sync_now();
    Ok(Written { host, revision })
}

/// Removes the host `name` from the store (the revision is unchanged if it wasn't there)
pub async fn delete(store: &dyn Store, name: &str) -> Result<Written, Refused> {
    let host = name.to_lowercase();
    let revision = store.delete_route(&host).await.map_err(unavailable)?;
    crate::routing::store_source::sync_now();
    Ok(Written { host, revision })
}

fn unavailable(err: impl std::fmt::Display) -> Refused {
    Refused::Unavailable(err.to_string())
}

/// A host from a request body, validated like routes from any other source
fn parse(name: &str, body: &[u8]) -> Result<Host, String> {
    let host =
        serde_json::from_slice::<Host>(body).map_err(|err| format!("invalid JSON: {err}"))?;
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
    use crate::stores::{store_trait::Store, MemoryStore};

    use super::{delete, get, list, parse, put, Refused, Written};

    const JSON: &str = r#"{
        "name": "api.test",
        "locations": [{ "path": "/", "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }] }]
    }"#;

    #[test]
    fn test_parse() {
        let host = parse("api.test", JSON.as_bytes()).unwrap();
        assert_eq!(host.name, "api.test");
        assert_eq!(host.locations[0].upstreams[0].port, 3000);
    }

    #[test]
    fn test_invalid_hosts() {
        let cases = [
            (JSON.replace("api.test", "other.test"), "the body is for host \"other.test\", not \"api.test\""),
            (JSON.replace(r#""name": "api.test","#, ""), "invalid JSON: missing field `name`"),
            ("{ not json".to_string(), "invalid JSON: "),
            ("location \"/\" {}".to_string(), "invalid JSON: "),
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
            let err = parse("api.test", body.as_bytes()).unwrap_err();
            assert!(err.starts_with(expected), "{err}");
        }
    }

    #[tokio::test]
    async fn test_put_get_list_and_delete() {
        let store = MemoryStore::new();

        let written = put(&store, "API.test", JSON.as_bytes()).await.unwrap();
        assert_eq!(
            written,
            Written {
                host: "api.test".to_string(),
                revision: 1
            }
        );
        assert!(store.routes().await.unwrap().routes[0].is_ok());

        let listed = list(&store).await.unwrap();
        assert_eq!(listed.revision, 1);
        assert_eq!(listed.routes[0].host.name, "api.test");
        assert!(listed.unreadable.is_empty());
        let one = get(&store, "API.test").await.unwrap();
        assert_eq!(one.host.name, "api.test");
        assert_eq!(one.revision, 1);
        assert_eq!(
            get(&store, "nope.test").await.map(|_| ()),
            Err(Refused::NotFound(
                "no host \"nope.test\" in the store".to_string()
            ))
        );

        assert_eq!(
            put(&store, "api.test", b"{").await,
            Err(Refused::Invalid(
                "invalid JSON: EOF while parsing an object at line 1 column 1".to_string()
            ))
        );

        let deleted = delete(&store, "api.test").await.unwrap();
        assert_eq!(
            deleted,
            Written {
                host: "api.test".to_string(),
                revision: 2
            }
        );
        assert!(list(&store).await.unwrap().routes.is_empty());
    }
}
