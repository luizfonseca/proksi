//! Tests that every store backend must pass

use std::time::Duration;

use crate::{
    certificates::Certificate,
    route::{Change, Expect, ReplicaStatus, RouteRecord, StoreError, Write},
    store_trait::Store,
    test_util::certificate,
};

/// A route as Proksi writes it (the store doesn't read it), in schema version `version`
fn versioned(host: &str, version: u32, upstream: &str) -> RouteRecord {
    let data = format!(r#"{{"schema_version":{version},"upstream":"{upstream}"}}"#);
    RouteRecord::new(host, version, data)
}

fn route(host: &str, upstream: &str) -> RouteRecord {
    versioned(host, 2, upstream)
}

/// The routes as written (host, schema version, data), without what the store adds
fn written(routes: Vec<RouteRecord>) -> Vec<RouteRecord> {
    let mut routes = routes
        .into_iter()
        .map(|r| RouteRecord::new(&r.host, r.schema_version, r.data))
        .collect::<Vec<_>>();
    routes.sort_by(|a, b| a.host.cmp(&b.host));
    routes
}

fn by(author: &str, expect: Expect) -> Write {
    Write {
        author: Some(author.to_string()),
        expect,
    }
}

/// Routes and revisions
pub async fn routes_contract(store: &dyn Store) {
    let initial = store.revision().await.unwrap();
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Adding a route increments the revision
    let revision = store
        .put_route(route("A.Contract.Test", "10.0.0.1"), &Write::default())
        .await
        .unwrap();
    assert_eq!(revision, initial + 1);
    assert_eq!(store.revision().await.unwrap(), revision);

    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    // Kept as written, under the lowercase host
    assert_eq!(written(snapshot.routes), vec![route("a.contract.test", "10.0.0.1")]);

    // Replacing the route of a host (hosts are case-insensitive)
    let revision = store
        .put_route(route("a.contract.test", "10.0.0.2"), &Write::default())
        .await
        .unwrap();
    assert_eq!(revision, initial + 2);
    let snapshot = store.routes().await.unwrap();
    assert_eq!(written(snapshot.routes), vec![route("a.contract.test", "10.0.0.2")]);

    // Any schema version is kept as it is: Proksi decides what it can read
    let newer = versioned("b.contract.test", 99, "10.0.0.3");
    let revision = store.put_route(newer.clone(), &Write::default()).await.unwrap();
    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(written(snapshot.routes), vec![route("a.contract.test", "10.0.0.2"), newer]);

    // Deleting
    let revision = store.delete_route("B.contract.test", &Write::default()).await.unwrap();
    assert_eq!(revision, initial + 4);
    let revision = store.delete_route("a.contract.test", &Write::default()).await.unwrap();
    assert_eq!(revision, initial + 5);
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Deleting a missing route doesn't change the revision
    assert_eq!(
        store.delete_route("missing.contract.test", &Write::default()).await.unwrap(),
        initial + 5
    );
}

/// Versions, history and rollbacks of a host, for a store keeping 3 versions per host
pub async fn history_contract(store: &dyn Store) {
    let anyone = Write::default();
    let r1 = store.put_route(route("h.test", "10.0.0.1"), &by("ana", Expect::Absent)).await.unwrap();
    let r2 = store.put_route(route("h.test", "10.0.0.2"), &by("bo", Expect::Changed(r1))).await.unwrap();

    // The active version, and the host's last change
    let active = store.routes().await.unwrap().routes.into_iter().find(|r| r.host == "h.test").unwrap();
    assert_eq!((active.version, active.changed), (r2, r2));
    assert_eq!(active.data, route("h.test", "10.0.0.2").data);

    // Every version is kept, with who saved it
    let first = store.route_version("H.test", r1).await.unwrap().unwrap();
    assert_eq!(first.record.data, route("h.test", "10.0.0.1").data);
    assert_eq!((first.record.version, first.author.as_deref()), (r1, Some("ana")));
    assert_eq!(store.route_version("h.test", r2 + 100).await.unwrap(), None);

    // Rolling back makes an older version active again, as a new change
    let r3 = store.rollback_route("h.test", r1, &by("cy", Expect::Changed(r2))).await.unwrap();
    assert_eq!(r3, r2 + 1);
    assert_eq!(store.revision().await.unwrap(), r3);
    let active = store.routes().await.unwrap().routes.into_iter().find(|r| r.host == "h.test").unwrap();
    assert_eq!((active.version, active.changed), (r1, r3));
    assert_eq!(active.data, route("h.test", "10.0.0.1").data);
    let history = store.route_history("h.test").await.unwrap();
    let changes = history.iter().map(|e| (e.revision, e.change, e.author.as_deref())).collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec![
            (r3, Change::RolledBack { to: r1 }, Some("cy")),
            (r2, Change::Saved, Some("bo")),
            (r1, Change::Saved, Some("ana")),
        ]
    );

    // Changes made from an older state are refused, and change nothing
    let stale = store.put_route(route("h.test", "10.0.0.9"), &by("bo", Expect::Changed(r2))).await;
    assert_eq!(stale, Err(StoreError::Conflict { changed: r3, present: true }));
    let stale = store.rollback_route("h.test", r2, &by("bo", Expect::Changed(r2))).await;
    assert_eq!(stale, Err(StoreError::Conflict { changed: r3, present: true }));
    let exists = store.put_route(route("h.test", "10.0.0.9"), &by("bo", Expect::Absent)).await;
    assert_eq!(exists, Err(StoreError::Conflict { changed: r3, present: true }));
    assert_eq!(store.revision().await.unwrap(), r3);
    assert_eq!(store.route_history("h.test").await.unwrap().len(), 3);

    // A version that isn't kept can't be rolled back to
    let missing = store.rollback_route("h.test", r3 + 100, &anyone).await;
    assert_eq!(missing, Err(StoreError::NoVersion(r3 + 100)));

    // Deleting keeps the versions: rolling back restores the host
    let r4 = store.delete_route("h.test", &by("di", Expect::Changed(r3))).await.unwrap();
    assert!(store.routes().await.unwrap().routes.iter().all(|r| r.host != "h.test"));
    assert_eq!(store.deleted_routes().await.unwrap(), vec!["h.test".to_string()]);
    assert_eq!(store.route_history("h.test").await.unwrap()[0].change, Change::Deleted);
    let gone = store.delete_route("h.test", &by("di", Expect::Changed(r3))).await;
    assert_eq!(gone, Err(StoreError::Conflict { changed: r4, present: false }));
    let r5 = store.rollback_route("h.test", r2, &by("di", Expect::Absent)).await.unwrap();
    let active = store.routes().await.unwrap().routes.into_iter().find(|r| r.host == "h.test").unwrap();
    assert_eq!((active.version, active.changed), (r2, r5));
    assert!(store.deleted_routes().await.unwrap().is_empty());

    // The last 3 versions and changes are kept
    let mut last = r5;
    for upstream in ["10.0.1.1", "10.0.1.2", "10.0.1.3"] {
        last = store.put_route(route("h.test", upstream), &anyone).await.unwrap();
    }
    assert_eq!(store.route_history("h.test").await.unwrap().len(), 3);
    assert_eq!(store.route_version("h.test", r1).await.unwrap(), None);
    assert_eq!(store.route_version("h.test", r2).await.unwrap(), None);
    assert!(store.route_version("h.test", last).await.unwrap().is_some());

    // Other hosts aren't affected
    assert!(store.route_history("other.test").await.unwrap().is_empty());
    store.delete_route("h.test", &anyone).await.unwrap();
}

/// Replica heartbeats
pub async fn replicas_contract(store: &dyn Store) {
    let status = |id: &str, revision| ReplicaStatus {
        id: id.to_string(),
        revision,
        applied_at: 1_700_000_000,
        version: "0.0.0".to_string(),
        hostname: "host".to_string(),
    };

    store
        .report(status("replica-1", 1), Duration::from_secs(60))
        .await
        .unwrap();
    // Expiries can be kept in whole seconds: 2 seconds is at least 1
    store
        .report(status("replica-2", 1), Duration::from_secs(2))
        .await
        .unwrap();
    // A new report replaces the previous one
    store
        .report(status("replica-1", 2), Duration::from_secs(60))
        .await
        .unwrap();

    let mut replicas = store.replicas().await.unwrap();
    replicas.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        replicas,
        vec![status("replica-1", 2), status("replica-2", 1)]
    );

    // Expired replicas aren't listed
    tokio::time::sleep(Duration::from_millis(3100)).await;
    let replicas = store.replicas().await.unwrap();
    assert_eq!(replicas, vec![status("replica-1", 2)]);
}

fn pem(certificate: &Certificate) -> Vec<u8> {
    certificate.leaf.to_pem().unwrap()
}

/// Leases: one holder at a time, released by their holder only, and expiring
pub async fn leases_contract(store: &dyn Store) {
    let minute = Duration::from_secs(60);

    assert!(store.try_lock("acme:a.test", "r1", minute).await.unwrap());
    assert!(!store.try_lock("acme:a.test", "r2", minute).await.unwrap());
    // The holder can take it again (extending it)
    assert!(store.try_lock("acme:a.test", "r1", minute).await.unwrap());
    // Other leases are independent
    assert!(store.try_lock("acme:b.test", "r2", minute).await.unwrap());

    // Only the holder releases it
    store.unlock("acme:a.test", "r2").await.unwrap();
    assert!(!store.try_lock("acme:a.test", "r2", minute).await.unwrap());
    store.unlock("acme:a.test", "r1").await.unwrap();
    assert!(store.try_lock("acme:a.test", "r2", minute).await.unwrap());

    // Who holds a lease
    assert_eq!(
        store.lock_holder("acme:a.test").await.unwrap().as_deref(),
        Some("r2")
    );
    assert_eq!(store.lock_holder("acme:missing.test").await.unwrap(), None);

    // An expired lease can be taken by someone else
    assert!(store
        .try_lock("acme:c.test", "r1", Duration::from_secs(1))
        .await
        .unwrap());
    assert!(!store.try_lock("acme:c.test", "r2", minute).await.unwrap());
    tokio::time::sleep(Duration::from_millis(2100)).await;
    // Expired leases have no holder
    assert_eq!(store.lock_holder("acme:c.test").await.unwrap(), None);
    assert!(store.try_lock("acme:c.test", "r2", minute).await.unwrap());
}

/// Certificate revisions: every write bumps it, and reloading picks up what another replica
/// (`other`, on the same store) wrote
pub async fn certificates_contract(store: &dyn Store, other: &dyn Store) {
    let initial = store.certificates_revision().await.unwrap();

    let first = certificate("renewed.test", 10);
    store
        .set_certificate("renewed.test", first.clone())
        .await
        .unwrap();
    assert_eq!(store.certificates_revision().await.unwrap(), initial + 1);
    assert_eq!(other.certificates_revision().await.unwrap(), initial + 1);

    // `other` caches the certificate, then `store` renews it
    let cached = other.get_certificate("renewed.test").await.unwrap();
    assert_eq!(pem(&cached), pem(&first));
    let renewed = certificate("renewed.test", 90);
    store
        .set_certificate("renewed.test", renewed.clone())
        .await
        .unwrap();
    assert_eq!(other.certificates_revision().await.unwrap(), initial + 2);

    other.reload_certificates().await.unwrap();
    let reloaded = other.get_certificate("renewed.test").await.unwrap();
    assert_eq!(pem(&reloaded), pem(&renewed));
}
