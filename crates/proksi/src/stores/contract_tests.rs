//! Tests that every store backend must pass

use std::time::Duration;

use crate::routing::test_utils::host;

use super::{
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
