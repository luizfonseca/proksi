//! Tests that every store backend must pass

use std::time::Duration;

use crate::{
    certificates::Certificate,
    notify::NotifyRecord,
    route::{Change, Expect, ReplicaStatus, RouteRecord, StoreError, Write},
    store_trait::Store,
    test_util::certificate,
    waiting_room::{Admission, RoomLimits},
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

/// `notify` blocks: kept as written, with a revision of their own
pub async fn notify_contract(store: &dyn Store) {
    let routes = store.revision().await.unwrap();
    let initial = store.notify_revision().await.unwrap();
    assert!(store.notify_blocks().await.unwrap().blocks.is_empty());

    let ops = r#"{"url":"https://hooks.example.com/a"}"#;
    let revision = store
        .put_notify(
            NotifyRecord::new("ops", ops.into()),
            &by("ops", Expect::Absent),
        )
        .await
        .unwrap();
    assert_eq!(revision, initial + 1);
    assert_eq!(store.notify_revision().await.unwrap(), revision);
    // Blocks don't change the routes' revision: replicas don't rebuild their routes for them
    assert_eq!(store.revision().await.unwrap(), routes);

    let snapshot = store.notify_blocks().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(
        snapshot.blocks,
        vec![NotifyRecord {
            version: revision,
            ..NotifyRecord::new("ops", ops.into())
        }]
    );

    // Created once: a second create is refused, with the version saved
    let again = store
        .put_notify(
            NotifyRecord::new("ops", "{}".into()),
            &by("ops", Expect::Absent),
        )
        .await;
    assert_eq!(again, Err(StoreError::Conflict { version: revision }));

    // Changed from the version read; a change from an older one is refused
    let changed = r#"{"url":"https://hooks.example.com/b"}"#;
    let next = store
        .put_notify(
            NotifyRecord::new("ops", changed.into()),
            &by("ops", Expect::Version(revision)),
        )
        .await
        .unwrap();
    assert_eq!(next, revision + 1);
    let stale = store
        .put_notify(
            NotifyRecord::new("ops", "{}".into()),
            &by("ops", Expect::Version(revision)),
        )
        .await;
    assert_eq!(stale, Err(StoreError::Conflict { version: next }));
    let other = store
        .put_notify(NotifyRecord::new("pager", "{}".into()), &Write::default())
        .await
        .unwrap();
    let names: Vec<String> = store
        .notify_blocks()
        .await
        .unwrap()
        .blocks
        .into_iter()
        .map(|block| block.name)
        .collect();
    assert_eq!(names, vec!["ops".to_string(), "pager".to_string()]);

    // Removed: from an older version refused, then removed; removing it again changes nothing
    let stale = store
        .delete_notify("ops", &by("ops", Expect::Version(revision)))
        .await;
    assert_eq!(stale, Err(StoreError::Conflict { version: next }));
    let removed = store
        .delete_notify("ops", &by("ops", Expect::Version(next)))
        .await
        .unwrap();
    assert_eq!(removed, other + 1);
    assert_eq!(
        store.delete_notify("ops", &Write::default()).await.unwrap(),
        removed
    );
    assert_eq!(
        store
            .delete_notify("ops", &by("ops", Expect::Version(next)))
            .await,
        Err(StoreError::Conflict { version: 0 })
    );
    let blocks = store.notify_blocks().await.unwrap();
    assert_eq!(blocks.revision, removed);
    assert_eq!(blocks.blocks.len(), 1);
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
    assert_eq!(
        written(snapshot.routes),
        vec![route("a.contract.test", "10.0.0.1")]
    );

    // Replacing the route of a host (hosts are case-insensitive)
    let revision = store
        .put_route(route("a.contract.test", "10.0.0.2"), &Write::default())
        .await
        .unwrap();
    assert_eq!(revision, initial + 2);
    let snapshot = store.routes().await.unwrap();
    assert_eq!(
        written(snapshot.routes),
        vec![route("a.contract.test", "10.0.0.2")]
    );

    // Any schema version is kept as it is: Proksi decides what it can read
    let newer = versioned("b.contract.test", 99, "10.0.0.3");
    let revision = store
        .put_route(newer.clone(), &Write::default())
        .await
        .unwrap();
    let snapshot = store.routes().await.unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(
        written(snapshot.routes),
        vec![route("a.contract.test", "10.0.0.2"), newer]
    );

    // Deleting
    let revision = store
        .delete_route("B.contract.test", &Write::default())
        .await
        .unwrap();
    assert_eq!(revision, initial + 4);
    let revision = store
        .delete_route("a.contract.test", &Write::default())
        .await
        .unwrap();
    assert_eq!(revision, initial + 5);
    assert!(store.routes().await.unwrap().routes.is_empty());

    // Deleting a missing route doesn't change the revision
    assert_eq!(
        store
            .delete_route("missing.contract.test", &Write::default())
            .await
            .unwrap(),
        initial + 5
    );
}

/// Versions, history and rollbacks of a host, for a store keeping 3 versions per host
pub async fn history_contract(store: &dyn Store) {
    let anyone = Write::default();
    let active = |routes: Vec<RouteRecord>| routes.into_iter().find(|r| r.host == "h.test");
    let r1 = store
        .put_route(route("h.test", "10.0.0.1"), &by("ana", Expect::Absent))
        .await
        .unwrap();
    let r2 = store
        .put_route(route("h.test", "10.0.0.2"), &by("bo", Expect::Version(r1)))
        .await
        .unwrap();

    // The active version
    let current = active(store.routes().await.unwrap().routes).unwrap();
    assert_eq!(current.version, r2);
    assert_eq!(current.data, route("h.test", "10.0.0.2").data);

    // Every version is kept, with who saved it
    let first = store.route_version("H.test", r1).await.unwrap().unwrap();
    assert_eq!(first.record.data, route("h.test", "10.0.0.1").data);
    assert_eq!(
        (first.record.version, first.author.as_deref()),
        (r1, Some("ana"))
    );
    assert_eq!(store.route_version("h.test", r2 + 100).await.unwrap(), None);

    // Rolling back makes an older version active again, as a new change
    let r3 = store
        .rollback_route("h.test", r1, &by("cy", Expect::Version(r2)))
        .await
        .unwrap();
    assert_eq!(r3, r2 + 1);
    assert_eq!(store.revision().await.unwrap(), r3);
    let current = active(store.routes().await.unwrap().routes).unwrap();
    assert_eq!(current.version, r1);
    assert_eq!(current.data, route("h.test", "10.0.0.1").data);
    let history = store.route_history("h.test").await.unwrap();
    let changes = history
        .iter()
        .map(|e| (e.revision, e.change, e.author.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        changes,
        vec![
            (r3, Change::RolledBack { to: r1 }, Some("cy")),
            (r2, Change::Saved, Some("bo")),
            (r1, Change::Saved, Some("ana")),
        ]
    );

    // Changes made from another version than the active one are refused, and change nothing
    let stale = store
        .put_route(route("h.test", "10.0.0.9"), &by("bo", Expect::Version(r2)))
        .await;
    assert_eq!(stale, Err(StoreError::Conflict { version: r1 }));
    let stale = store
        .rollback_route("h.test", r2, &by("bo", Expect::Version(r2)))
        .await;
    assert_eq!(stale, Err(StoreError::Conflict { version: r1 }));
    let exists = store
        .put_route(route("h.test", "10.0.0.9"), &by("bo", Expect::Absent))
        .await;
    assert_eq!(exists, Err(StoreError::Conflict { version: r1 }));
    assert_eq!(store.revision().await.unwrap(), r3);
    assert_eq!(store.route_history("h.test").await.unwrap().len(), 3);

    // A version that isn't kept can't be rolled back to
    let missing = store.rollback_route("h.test", r3 + 100, &anyone).await;
    assert_eq!(missing, Err(StoreError::NoVersion(r3 + 100)));

    // Deleting keeps the versions: rolling back restores the host
    store
        .delete_route("h.test", &by("di", Expect::Version(r1)))
        .await
        .unwrap();
    assert!(active(store.routes().await.unwrap().routes).is_none());
    assert_eq!(
        store.deleted_routes().await.unwrap(),
        vec!["h.test".to_string()]
    );
    assert_eq!(
        store.route_history("h.test").await.unwrap()[0].change,
        Change::Deleted
    );
    let gone = store
        .delete_route("h.test", &by("di", Expect::Version(r1)))
        .await;
    assert_eq!(gone, Err(StoreError::Conflict { version: 0 }));
    store
        .rollback_route("h.test", r2, &by("di", Expect::Absent))
        .await
        .unwrap();
    assert_eq!(
        active(store.routes().await.unwrap().routes)
            .unwrap()
            .version,
        r2
    );
    assert!(store.deleted_routes().await.unwrap().is_empty());

    // The last 3 versions and changes are kept
    let mut last = r3;
    for upstream in ["10.0.1.1", "10.0.1.2", "10.0.1.3"] {
        last = store
            .put_route(route("h.test", upstream), &anyone)
            .await
            .unwrap();
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

/// Waiting rooms: slots given in the order visitors came, kept while they call, and freed when
/// they expire; places lost by visitors who stop waiting
pub async fn waiting_rooms_contract(store: &dyn Store) {
    let limits = RoomLimits {
        active: 2,
        session: Duration::from_millis(1500),
        abandon: Duration::from_secs(60),
    };
    let wait = |visitor: &'static str| store.wait_in_room("launch", visitor, limits);

    assert_eq!(wait("a").await.unwrap(), Admission::Admitted);
    assert_eq!(wait("b").await.unwrap(), Admission::Admitted);
    assert_eq!(
        wait("c").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 1
        }
    );
    assert_eq!(
        wait("d").await.unwrap(),
        Admission::Waiting {
            position: 2,
            waiting: 2
        }
    );
    // Calling again keeps a slot, or a place
    assert_eq!(wait("a").await.unwrap(), Admission::Admitted);
    assert_eq!(
        wait("c").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 2
        }
    );
    // Rooms are independent
    assert_eq!(
        store.wait_in_room("other", "c", limits).await.unwrap(),
        Admission::Admitted
    );

    // Slots expire, and go to the visitors in line, in order: one is kept for `c`, who came
    // before `e`
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert_eq!(wait("d").await.unwrap(), Admission::Admitted);
    assert_eq!(
        wait("e").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 2
        }
    );
    assert_eq!(wait("c").await.unwrap(), Admission::Admitted);
    assert_eq!(
        wait("e").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 1
        }
    );

    // Visitors who stop waiting lose their place
    let impatient = RoomLimits {
        active: 1,
        session: Duration::from_secs(60),
        abandon: Duration::from_secs(1),
    };
    let wait = |visitor: &'static str| store.wait_in_room("sale", visitor, impatient);
    assert_eq!(wait("f").await.unwrap(), Admission::Admitted);
    assert_eq!(
        wait("g").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 1
        }
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        wait("h").await.unwrap(),
        Admission::Waiting {
            position: 1,
            waiting: 1
        }
    );

    // One key, kept
    let key = store.waiting_room_key().await.unwrap();
    assert_ne!(key, [0; 32]);
    assert_eq!(store.waiting_room_key().await.unwrap(), key);
}

pub(crate) async fn cursors_contract(store: &dyn Store) {
    let step = |key: &str, step_us| (key.to_string(), step_us);

    // Each cursor starts at the store's now, moves by its step, and says how far ahead it is
    let ahead = store
        .advance_cursors("n1", &[step("a", 2_000_000), step("b", 1_000_000)])
        .await
        .unwrap();
    assert!((1_990_000..=2_000_000).contains(&ahead[0]), "{ahead:?}");
    assert!((990_000..=1_000_000).contains(&ahead[1]), "{ahead:?}");
    // A cursor ahead of now moves on from where it is
    let ahead = store
        .advance_cursors("n1", &[step("a", 2_000_000)])
        .await
        .unwrap();
    assert!((3_980_000..=4_000_000).contains(&ahead[0]), "{ahead:?}");
    // Each namespace is its own
    let ahead = store
        .advance_cursors("n2", &[step("a", 1_000_000)])
        .await
        .unwrap();
    assert!((990_000..=1_000_000).contains(&ahead[0]), "{ahead:?}");
    // No step: it's read
    let ahead = store.advance_cursors("n1", &[step("b", 0)]).await.unwrap();
    assert!((900_000..=1_000_000).contains(&ahead[0]), "{ahead:?}");
    // Nothing to move: nothing to answer
    assert!(store.advance_cursors("n1", &[]).await.unwrap().is_empty());
}
