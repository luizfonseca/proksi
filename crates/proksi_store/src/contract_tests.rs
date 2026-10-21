//! Tests that every store backend must pass

use std::time::Duration;

use crate::{
    certificates::Certificate,
    route::{ReplicaStatus, RouteRecord},
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

/// Routes and revisions
pub async fn routes_contract(store: &dyn Store) {
    let initial = store.revision().await.unwrap();
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Adding a route increments the revision
    let revision = store
        .put_route(route("A.Contract.Test", "10.0.0.1"))
        .await
        .unwrap();
    assert_eq!(revision, initial + 1);
    assert_eq!(store.revision().await.unwrap(), revision);

    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    // Kept as written, under the lowercase host
    assert_eq!(snapshot.routes, vec![route("a.contract.test", "10.0.0.1")]);

    // Replacing the route of a host (hosts are case-insensitive)
    let revision = store
        .put_route(route("a.contract.test", "10.0.0.2"))
        .await
        .unwrap();
    assert_eq!(revision, initial + 2);
    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.routes, vec![route("a.contract.test", "10.0.0.2")]);

    // Any schema version is kept as it is: Proksi decides what it can read
    let newer = versioned("b.contract.test", 99, "10.0.0.3");
    let revision = store.put_route(newer.clone()).await.unwrap();
    let mut snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    snapshot.routes.sort_by(|a, b| a.host.cmp(&b.host));
    assert_eq!(snapshot.routes, vec![route("a.contract.test", "10.0.0.2"), newer]);

    // Deleting
    let revision = store.delete_route("B.contract.test").await.unwrap();
    assert_eq!(revision, initial + 4);
    let revision = store.delete_route("a.contract.test").await.unwrap();
    assert_eq!(revision, initial + 5);
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Deleting a missing route doesn't change the revision
    assert_eq!(
        store.delete_route("missing.contract.test").await.unwrap(),
        initial + 5
    );
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
