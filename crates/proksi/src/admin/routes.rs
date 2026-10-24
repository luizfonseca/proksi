//! `/api/v1/routes`: hosts written to the store (ADR 0004), applied by every instance through
//! the store sync (ADR 0001). Hosts are JSON, in and out.

use proksi_store::{HistoryEntry, Store, StoreError, Write};
use serde::{Deserialize, Serialize};

use crate::{
    config::host::Host,
    stores::stored_route::{decode_stored_route, HostStore, StoredHost, StoredRoute, StoredRouteError},
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
    /// The host isn't on the version the change was made from (412): it's on `version` (0 if
    /// it isn't in the store)
    Conflict { version: u64 },
    /// A version this version of proksi can't use (422)
    Unusable(String),
}

/// `GET /api/v1/routes`
#[derive(Debug, Serialize)]
pub struct RouteList {
    pub revision: u64,
    pub routes: Vec<StoredHost>,
    /// Routes this version can't read
    pub unreadable: Vec<StoredRouteError>,
}

/// `GET /api/v1/routes/<host>`
#[derive(Debug, Serialize)]
pub struct OneRoute {
    pub host: Host,
    /// The store's revision when it was read
    pub revision: u64,
    /// The revision its definition was saved at (0 if saved before versions were kept), sent
    /// back as `If-Match` so a change is refused if the host is on another version by then
    pub version: u64,
}

/// `GET /api/v1/routes/<host>/history`
#[derive(Debug, Serialize)]
pub struct History {
    pub host: String,
    /// Newest first
    pub changes: Vec<HistoryEntry>,
}

/// `GET /api/v1/routes/<host>/history/<revision>`
#[derive(Debug, Serialize)]
pub struct SavedVersion {
    pub host: Host,
    /// The revision it was saved at
    pub version: u64,
    /// Unix timestamp (seconds)
    pub at: u64,
    pub author: Option<String>,
}

/// `GET /api/v1/deleted-routes`
#[derive(Debug, Serialize)]
pub struct Deleted {
    pub hosts: Vec<String>,
}

/// `POST /api/v1/routes/<host>/rollback`
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rollback {
    /// The version to make active again
    revision: u64,
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
    let snapshot = store.stored_routes().await.map_err(unavailable)?;
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
    let snapshot = store.stored_routes().await.map_err(unavailable)?;
    let route = snapshot.routes.into_iter().find(|route| match route {
        Ok(stored) => stored.route.key() == name,
        Err(err) => err.host == name,
    });
    match route {
        Some(Ok(stored)) => Ok(OneRoute {
            host: stored.route.host,
            revision: snapshot.revision,
            version: stored.version,
        }),
        Some(Err(err)) => Err(Refused::Unavailable(err.error)),
        None => Err(Refused::NotFound(format!(
            "no host \"{name}\" in the store"
        ))),
    }
}

/// Writes the host `name` to the store, as a new version
pub async fn put(
    store: &dyn Store,
    name: &str,
    body: &[u8],
    write: &Write,
) -> Result<Written, Refused> {
    let host = parse(&name.to_lowercase(), body).map_err(Refused::Invalid)?;
    let route = StoredRoute::new(host);
    let host = route.key();
    let revision = store.put_stored(&route, write).await.map_err(refused)?;
    crate::routing::store_source::sync_now();
    Ok(Written { host, revision })
}

/// `POST /api/v1/validate`
#[derive(Debug, PartialEq, Serialize)]
pub struct Valid {
    pub valid: bool,
}

/// Checks a host as `put` would, without writing it
pub fn validate(body: &[u8]) -> Result<Valid, Refused> {
    let name = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|host| host.get("name")?.as_str().map(str::to_lowercase))
        .unwrap_or_default();
    parse(&name, body).map_err(Refused::Invalid)?;
    Ok(Valid { valid: true })
}

/// Removes the host `name` from the store, keeping its versions (the revision is unchanged
/// if it wasn't there)
pub async fn delete(store: &dyn Store, name: &str, write: &Write) -> Result<Written, Refused> {
    let host = name.to_lowercase();
    let revision = store.delete_route(&host, write).await.map_err(refused)?;
    crate::routing::store_source::sync_now();
    Ok(Written { host, revision })
}

/// The changes to the host `name`, newest first
pub async fn history(store: &dyn Store, name: &str) -> Result<History, Refused> {
    let host = name.to_lowercase();
    let changes = store.route_history(&host).await.map_err(refused)?;
    if changes.is_empty() {
        return Err(Refused::NotFound(format!("no history for host \"{host}\"")));
    }
    Ok(History { host, changes })
}

/// The version of the host `name` saved at `revision`
pub async fn version(store: &dyn Store, name: &str, revision: u64) -> Result<SavedVersion, Refused> {
    let host = name.to_lowercase();
    let saved = store
        .route_version(&host, revision)
        .await
        .map_err(refused)?
        .ok_or(Refused::NotFound(format!("no version {revision} of host \"{host}\"")))?;
    let route = decode_stored_route(&host, &saved.record.data)
        .map_err(|err| Refused::Unusable(err.error))?;
    Ok(SavedVersion {
        host: route.host,
        version: revision,
        at: saved.at,
        author: saved.author,
    })
}

/// Makes a version of the host `name` active again (`{"revision": <version>}`), also for a
/// deleted host. The version must still be valid for this version of proksi.
pub async fn rollback(
    store: &dyn Store,
    name: &str,
    body: &[u8],
    write: &Write,
) -> Result<Written, Refused> {
    let Rollback { revision: to } = serde_json::from_slice(body)
        .map_err(|err| Refused::Invalid(format!(r#"expected {{"revision": <version>}}: {err}"#)))?;
    let target = version(store, name, to).await?;
    crate::routing::validate::validate_host(&target.host).map_err(|err| {
        Refused::Unusable(format!("version {to} is no longer valid: {err}"))
    })?;
    let host = name.to_lowercase();
    let revision = store.rollback_route(&host, to, write).await.map_err(refused)?;
    crate::routing::store_source::sync_now();
    Ok(Written { host, revision })
}

/// Deleted hosts that still have versions, which a rollback restores
pub async fn deleted(store: &dyn Store) -> Result<Deleted, Refused> {
    let hosts = store.deleted_routes().await.map_err(refused)?;
    Ok(Deleted { hosts })
}

fn unavailable(err: impl std::fmt::Display) -> Refused {
    Refused::Unavailable(err.to_string())
}

/// Why the store refused a change
pub(super) fn refused(err: StoreError) -> Refused {
    match err {
        StoreError::Conflict { version } => Refused::Conflict { version },
        StoreError::NoVersion(version) => Refused::NotFound(format!("no version {version}")),
        StoreError::Invalid(err) => Refused::Invalid(err),
        StoreError::Unavailable(err) => Refused::Unavailable(err),
    }
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
    use proksi_store::{Change, Expect, MemoryStore, Write};

    use super::{deleted, delete, get, history, list, parse, put, rollback, version, Refused, Written};
    use crate::stores::stored_route::HostStore;

    const JSON: &str = r#"{
        "name": "api.test",
        "locations": [{ "path": "/", "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }] }]
    }"#;

    #[test]
    fn test_validate() {
        assert_eq!(super::validate(JSON.as_bytes()), Ok(super::Valid { valid: true }));
        let err = super::validate(JSON.replace("3000", "0").as_bytes());
        assert_eq!(
            err,
            Err(Refused::Invalid(
                "location \"/\": upstreams[0].port must be greater than 0".to_string()
            ))
        );
    }

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

        let written = put(&store, "API.test", JSON.as_bytes(), &Write::default())
            .await
            .unwrap();
        assert_eq!(
            written,
            Written {
                host: "api.test".to_string(),
                revision: 1
            }
        );
        assert!(store.stored_routes().await.unwrap().routes[0].is_ok());

        let listed = list(&store).await.unwrap();
        assert_eq!(listed.revision, 1);
        assert_eq!(listed.routes[0].route.host.name, "api.test");
        assert!(listed.unreadable.is_empty());
        let one = get(&store, "API.test").await.unwrap();
        assert_eq!(one.host.name, "api.test");
        assert_eq!((one.revision, one.version), (1, 1));
        assert_eq!(
            get(&store, "nope.test").await.map(|_| ()),
            Err(Refused::NotFound(
                "no host \"nope.test\" in the store".to_string()
            ))
        );

        assert_eq!(
            put(&store, "api.test", b"{", &Write::default()).await,
            Err(Refused::Invalid(
                "invalid JSON: EOF while parsing an object at line 1 column 1".to_string()
            ))
        );

        let deleted = delete(&store, "api.test", &Write::default()).await.unwrap();
        assert_eq!(
            deleted,
            Written {
                host: "api.test".to_string(),
                revision: 2
            }
        );
        assert!(list(&store).await.unwrap().routes.is_empty());
    }

    fn by(author: &str, expect: Expect) -> Write {
        Write {
            author: Some(author.to_string()),
            expect,
        }
    }

    /// The host of `JSON`, sent to `upstream`
    fn json(upstream: &str) -> Vec<u8> {
        JSON.replace("10.0.0.1", upstream).into_bytes()
    }

    #[tokio::test]
    async fn test_history_and_rollback() {
        let store = MemoryStore::new();
        let first = put(&store, "api.test", &json("10.0.0.1"), &by("ana", Expect::Absent))
            .await
            .unwrap();
        let second = put(&store, "api.test", &json("10.0.0.2"), &by("bo", Expect::Version(first.revision)))
            .await
            .unwrap();

        // Each version, with who saved it
        let saved = version(&store, "API.test", first.revision).await.unwrap();
        assert_eq!(saved.host.locations[0].upstreams[0].ip.to_string(), "10.0.0.1");
        assert_eq!(saved.author.as_deref(), Some("ana"));
        assert!(matches!(
            version(&store, "api.test", 99).await,
            Err(Refused::NotFound(_))
        ));

        // Rolling back, expecting the host's last change
        let body = format!(r#"{{"revision": {}}}"#, first.revision);
        rollback(&store, "api.test", body.as_bytes(), &by("cy", Expect::Version(second.revision)))
            .await
            .unwrap();
        let one = get(&store, "api.test").await.unwrap();
        assert_eq!(one.version, first.revision);
        let changes = history(&store, "api.test").await.unwrap().changes;
        assert_eq!(changes[0].change, Change::RolledBack { to: first.revision });
        assert_eq!(changes[0].author.as_deref(), Some("cy"));
        assert_eq!(changes.len(), 3);

        // A change made from an older state is refused
        let stale = put(&store, "api.test", &json("10.0.0.9"), &by("bo", Expect::Version(second.revision))).await;
        assert_eq!(stale, Err(Refused::Conflict { version: first.revision }));
        let stale = rollback(&store, "api.test", body.as_bytes(), &by("bo", Expect::Version(second.revision))).await;
        assert!(matches!(stale, Err(Refused::Conflict { .. })));
        // A change made from the active version goes through
        put(&store, "api.test", &json("10.0.0.3"), &by("ana", Expect::Version(first.revision)))
            .await
            .unwrap();

        // Bad bodies, unknown versions and hosts
        let invalid = rollback(&store, "api.test", b"{}", &Write::default()).await;
        assert!(matches!(invalid, Err(Refused::Invalid(_))), "{invalid:?}");
        let missing = rollback(&store, "api.test", br#"{"revision": 99}"#, &Write::default()).await;
        assert!(matches!(missing, Err(Refused::NotFound(_))), "{missing:?}");
        assert!(matches!(history(&store, "nope.test").await, Err(Refused::NotFound(_))));
    }

    #[tokio::test]
    async fn test_deleted_hosts_are_restored() {
        let store = MemoryStore::new();
        let saved = put(&store, "api.test", &json("10.0.0.1"), &Write::default())
            .await
            .unwrap();
        delete(&store, "api.test", &Write::default()).await.unwrap();
        assert_eq!(deleted(&store).await.unwrap().hosts, vec!["api.test".to_string()]);
        assert!(matches!(get(&store, "api.test").await, Err(Refused::NotFound(_))));

        // Restoring expects the host not to have been created again since
        let body = format!(r#"{{"revision": {}}}"#, saved.revision);
        rollback(&store, "api.test", body.as_bytes(), &by("ana", Expect::Absent))
            .await
            .unwrap();
        assert_eq!(get(&store, "api.test").await.unwrap().version, saved.revision);
        assert!(deleted(&store).await.unwrap().hosts.is_empty());
    }

    #[tokio::test]
    async fn test_versions_no_longer_valid_are_not_rolled_back_to() {
        use proksi_store::{RouteRecord, Store};

        let store = MemoryStore::new();
        // Saved by an older proksi, with an upstream port this version refuses
        let data = JSON.replace("3000", "0");
        let stored = format!(r#"{{"schema_version": 2, "host": {data}}}"#);
        let revision = store
            .put_route(RouteRecord::new("api.test", 2, stored), &Write::default())
            .await
            .unwrap();

        let body = format!(r#"{{"revision": {revision}}}"#);
        let refused = rollback(&store, "api.test", body.as_bytes(), &Write::default()).await;
        assert!(
            matches!(&refused, Err(Refused::Unusable(reason)) if reason.starts_with(&format!("version {revision} is no longer valid"))),
            "{refused:?}"
        );
    }
}
