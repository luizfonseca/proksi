use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use serde::Serialize;

use crate::{config::host::Host, stores};

use super::{
    build::{build_host_container, load_host_certificate},
    merge::{merge, Conflict, InvalidRoute, MergeResult},
    RouteSnapshot, RouteSourceKind,
};

/// Current state of the routes, for logs and the admin API
#[derive(Debug, Default, Clone, Serialize)]
pub struct RoutingStatus {
    /// Host and source of every route being served
    pub routes: Vec<(String, RouteSourceKind)>,
    pub conflicts: Vec<Conflict>,
    pub invalid: Vec<InvalidRoute>,
    /// Revision of the store routes being served
    pub store_revision: Option<u64>,
}

static ROUTING_STATUS: Lazy<ArcSwap<RoutingStatus>> =
    Lazy::new(|| ArcSwap::from_pointee(RoutingStatus::default()));

// Read by the store source and the admin API
#[allow(dead_code)]
pub fn routing_status() -> Arc<RoutingStatus> {
    ROUTING_STATUS.load_full()
}

/// Hosts changed by applying a snapshot
#[derive(Debug, Default, PartialEq)]
pub struct ApplyReport {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

/// Keeps the latest routes of every source and applies the merged result to the route table
#[derive(Default)]
pub struct Reconciler {
    snapshots: BTreeMap<RouteSourceKind, Vec<Host>>,
    /// Hosts currently in the route table, by name
    applied: HashMap<String, (RouteSourceKind, Host)>,
    store_revision: Option<u64>,
    conflicts: Vec<Conflict>,
    invalid: Vec<InvalidRoute>,
}

impl Reconciler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the routes of the snapshot's source and applies the changes to the route table
    pub async fn apply(&mut self, snapshot: RouteSnapshot) -> ApplyReport {
        if snapshot.source == RouteSourceKind::Store {
            self.store_revision = snapshot.revision.or(self.store_revision);
        }
        self.snapshots.insert(snapshot.source, snapshot.hosts);

        let merged = merge(&self.snapshots);
        let mut report = ApplyReport::default();

        for (host, (source, definition)) in &merged.hosts {
            let is_new = match self.applied.get(host) {
                // Same definition from another source: nothing to rebuild
                Some((_, applied)) if applied == definition => {
                    self.applied
                        .insert(host.clone(), (*source, definition.clone()));
                    continue;
                }
                Some(_) => false,
                None => true,
            };

            let Some(container) = build_host_container(definition) else {
                // Keep serving the previous version of the host, if any
                continue;
            };

            if let Err(err) = load_host_certificate(definition).await {
                tracing::error!("failed to load the certificate for host {host}: {err}");
            }

            stores::insert_route(host.clone(), Arc::new(container));
            self.applied
                .insert(host.clone(), (*source, definition.clone()));
            if is_new {
                report.added.push(host.clone());
            } else {
                report.updated.push(host.clone());
            }
        }

        let removed_hosts = self
            .applied
            .keys()
            .filter(|host| !merged.hosts.contains_key(*host))
            .cloned()
            .collect::<Vec<_>>();
        for host in removed_hosts {
            stores::remove_route(&host);
            self.applied.remove(&host);
            report.removed.push(host);
        }

        self.log_changes(&merged, &report);
        for source in [
            RouteSourceKind::File,
            RouteSourceKind::Docker,
            RouteSourceKind::Kubernetes,
            RouteSourceKind::Store,
        ] {
            let hosts = self.applied.values().filter(|(s, _)| *s == source).count();
            crate::metrics::set_routes(&format!("{source:?}").to_lowercase(), hosts);
        }
        self.conflicts = merged.conflicts;
        self.invalid = merged.invalid;
        ROUTING_STATUS.store(Arc::new(self.status()));

        report
    }

    /// Status of the routes applied by this reconciler
    pub fn status(&self) -> RoutingStatus {
        let mut routes = self
            .applied
            .iter()
            .map(|(host, (source, _))| (host.clone(), *source))
            .collect::<Vec<_>>();
        routes.sort();

        RoutingStatus {
            routes,
            conflicts: self.conflicts.clone(),
            invalid: self.invalid.clone(),
            store_revision: self.store_revision,
        }
    }

    /// Logs route changes, and conflicts or invalid routes when they change
    fn log_changes(&self, merged: &MergeResult, report: &ApplyReport) {
        for host in &report.added {
            tracing::info!("added route for host {host}");
        }
        for host in &report.updated {
            tracing::info!("updated route for host {host}");
        }
        for host in &report.removed {
            tracing::info!("removed route for host {host}");
        }

        if merged.conflicts != self.conflicts {
            for conflict in &merged.conflicts {
                tracing::warn!(
                    "host {} is defined by several sources, using {:?} (also defined by {:?})",
                    conflict.host,
                    conflict.winner,
                    conflict.shadowed
                );
            }
        }
        if merged.invalid != self.invalid {
            for invalid in &merged.invalid {
                tracing::warn!(
                    "skipping invalid route for host {} from {:?}: {}",
                    invalid.host,
                    invalid.source,
                    invalid.error
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        routing::{test_utils::host, RouteSnapshot, RouteSourceKind},
        stores,
    };

    use super::{ApplyReport, Reconciler};

    fn upstream_ip(host: &str) -> Option<String> {
        stores::get_route_by_key(host).map(|c| c.locations[0].upstreams[0].ip.to_string())
    }

    #[tokio::test]
    async fn test_reconciler_adds_routes() {
        let mut reconciler = Reconciler::new();

        let report = reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-add.test", "10.0.0.1")],
            ))
            .await;

        assert_eq!(report.added, vec!["rec-add.test".to_string()]);
        assert_eq!(upstream_ip("rec-add.test"), Some("10.0.0.1".into()));
    }

    #[tokio::test]
    async fn test_reconciler_keeps_unchanged_routes() {
        let mut reconciler = Reconciler::new();
        let snapshot = RouteSnapshot::new(
            RouteSourceKind::File,
            vec![host("rec-same.test", "10.0.0.1")],
        );
        reconciler.apply(snapshot.clone()).await;
        let before = stores::get_route_by_key("rec-same.test").unwrap();

        let report = reconciler.apply(snapshot).await;

        assert_eq!(report, ApplyReport::default());
        let after = stores::get_route_by_key("rec-same.test").unwrap();
        assert!(Arc::ptr_eq(
            &before.locations[0].load_balancer,
            &after.locations[0].load_balancer
        ));
    }

    #[tokio::test]
    async fn test_reconciler_updates_changed_routes() {
        let mut reconciler = Reconciler::new();
        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-update.test", "10.0.0.1")],
            ))
            .await;

        let report = reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-update.test", "10.0.0.2")],
            ))
            .await;

        assert_eq!(report.updated, vec!["rec-update.test".to_string()]);
        assert_eq!(upstream_ip("rec-update.test"), Some("10.0.0.2".into()));
    }

    #[tokio::test]
    async fn test_reconciler_removes_routes() {
        let mut reconciler = Reconciler::new();
        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-remove.test", "10.0.0.1")],
            ))
            .await;

        let report = reconciler
            .apply(RouteSnapshot::new(RouteSourceKind::File, vec![]))
            .await;

        assert_eq!(report.removed, vec!["rec-remove.test".to_string()]);
        assert!(stores::get_route_by_key("rec-remove.test").is_none());
    }

    #[tokio::test]
    async fn test_reconciler_store_overrides_file_and_falls_back() {
        let mut reconciler = Reconciler::new();
        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-priority.test", "10.0.0.1")],
            ))
            .await;

        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::Store,
                vec![host("rec-priority.test", "10.0.0.2")],
            ))
            .await;
        assert_eq!(upstream_ip("rec-priority.test"), Some("10.0.0.2".into()));

        reconciler
            .apply(RouteSnapshot::new(RouteSourceKind::Store, vec![]))
            .await;
        assert_eq!(upstream_ip("rec-priority.test"), Some("10.0.0.1".into()));
    }

    #[tokio::test]
    async fn test_reconciler_uses_lowercase_hosts() {
        let mut reconciler = Reconciler::new();

        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("Rec-Case.Test", "10.0.0.1")],
            ))
            .await;

        assert_eq!(upstream_ip("rec-case.test"), Some("10.0.0.1".into()));
    }

    #[tokio::test]
    async fn test_reconciler_status() {
        let mut reconciler = Reconciler::new();
        reconciler
            .apply(RouteSnapshot::new(
                RouteSourceKind::File,
                vec![host("rec-status.test", "10.0.0.1")],
            ))
            .await;
        let mut store = RouteSnapshot::new(
            RouteSourceKind::Store,
            vec![host("rec-status.test", "10.0.0.2")],
        );
        store.revision = Some(7);

        reconciler.apply(store).await;
        let status = reconciler.status();

        assert!(status
            .routes
            .contains(&("rec-status.test".to_string(), RouteSourceKind::Store)));
        assert_eq!(status.conflicts.len(), 1);
        assert_eq!(status.store_revision, Some(7));
    }

    #[tokio::test]
    async fn test_reconciler_serves_locations_by_path() {
        let mut host = crate::routing::test_utils::host("rec-locations.test", "10.0.0.1");
        let mut session = host.locations[0].clone();
        session.path = "/v1/session".to_string();
        session.exact = false;
        session.upstreams[0].ip = "10.0.0.2".into();
        host.locations.push(session);

        let mut reconciler = Reconciler::new();
        reconciler
            .apply(RouteSnapshot::new(RouteSourceKind::File, vec![host]))
            .await;

        let container = stores::get_route_by_key("rec-locations.test").unwrap();
        let served = |path: &str| {
            container.location_for(path).unwrap().upstreams[0]
                .ip
                .to_string()
        };
        assert_eq!(served("/v1/session"), "10.0.0.2");
        assert_eq!(served("/"), "10.0.0.1");
    }
}
