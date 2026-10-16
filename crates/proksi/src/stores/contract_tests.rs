//! Tests that every store backend must pass

use std::time::Duration;

use openssl::{
    asn1::Asn1Time,
    hash::MessageDigest,
    pkey::PKey,
    rsa::Rsa,
    x509::{X509Name, X509},
};

use crate::routing::test_utils::host;

use super::{
    certificates::Certificate,
    store_trait::Store,
    stored_route::{ReplicaStatus, StoredRoute},
};

fn upstream_ip(stored: &StoredRoute) -> String {
    stored.host.locations[0].upstreams[0].ip.to_string()
}

/// Routes and revisions
pub async fn routes_contract(store: &dyn Store) {
    let initial = store.revision().await.unwrap();
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Adding a route increments the revision
    let revision = store
        .put_route(StoredRoute::new(host("A.Contract.Test", "10.0.0.1")))
        .await
        .unwrap();
    assert_eq!(revision, initial + 1);
    assert_eq!(store.revision().await.unwrap(), revision);

    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.routes.len(), 1);
    let stored = snapshot.routes[0].as_ref().unwrap();
    assert_eq!(stored.host.name, "A.Contract.Test");
    assert_eq!(upstream_ip(stored), "10.0.0.1");

    // Replacing the route of a host (hosts are case-insensitive)
    let revision = store
        .put_route(StoredRoute::new(host("a.contract.test", "10.0.0.2")))
        .await
        .unwrap();
    assert_eq!(revision, initial + 2);
    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(
        upstream_ip(snapshot.routes[0].as_ref().unwrap()),
        "10.0.0.2"
    );

    // Routes this version can't read are reported, without affecting the others
    let mut newer = StoredRoute::new(host("b.contract.test", "10.0.0.3"));
    newer.schema_version = crate::stores::stored_route::ROUTE_SCHEMA_VERSION + 1;
    let revision = store.put_route(newer).await.unwrap();
    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.routes.len(), 2);
    let errors = snapshot
        .routes
        .iter()
        .filter_map(|r| r.as_ref().err())
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].host, "b.contract.test");

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
    store
        .report(status("replica-2", 1), Duration::from_secs(1))
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
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let replicas = store.replicas().await.unwrap();
    assert_eq!(replicas, vec![status("replica-1", 2)]);
}

/// A self-signed certificate for `domain`, valid for `days`
pub fn certificate(domain: &str, days: u32) -> Certificate {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509Name::builder().unwrap();
    name.append_entry_by_text("CN", domain).unwrap();
    let name = name.build();

    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_subject_name(&name).unwrap();
    leaf.set_issuer_name(&name).unwrap();
    leaf.set_pubkey(&key).unwrap();
    leaf.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    leaf.set_not_after(&Asn1Time::days_from_now(days).unwrap())
        .unwrap();
    leaf.sign(&key, MessageDigest::sha256()).unwrap();

    Certificate {
        key,
        leaf: leaf.build(),
        chain: None,
    }
}

fn days_left(certificate: &Certificate) -> i32 {
    Asn1Time::days_from_now(0)
        .unwrap()
        .diff(certificate.leaf.not_after())
        .unwrap()
        .days
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

    // An expired lease can be taken by someone else
    assert!(store
        .try_lock("acme:c.test", "r1", Duration::from_secs(1))
        .await
        .unwrap());
    assert!(!store.try_lock("acme:c.test", "r2", minute).await.unwrap());
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(store.try_lock("acme:c.test", "r2", minute).await.unwrap());
}

/// Certificate revisions: every write bumps it, and reloading picks up what another replica
/// (`other`, on the same store) wrote
pub async fn certificates_contract(store: &dyn Store, other: &dyn Store) {
    let initial = store.certificates_revision().await.unwrap();

    store
        .set_certificate("renewed.test", certificate("renewed.test", 10))
        .await
        .unwrap();
    assert_eq!(store.certificates_revision().await.unwrap(), initial + 1);
    assert_eq!(other.certificates_revision().await.unwrap(), initial + 1);

    // `other` caches the certificate, then `store` renews it
    let cached = other.get_certificate("renewed.test").await.unwrap();
    assert_eq!(days_left(&cached), 10);
    store
        .set_certificate("renewed.test", certificate("renewed.test", 90))
        .await
        .unwrap();
    assert_eq!(other.certificates_revision().await.unwrap(), initial + 2);

    other.reload_certificates().await.unwrap();
    let reloaded = other.get_certificate("renewed.test").await.unwrap();
    assert_eq!(days_left(&reloaded), 90);
}
