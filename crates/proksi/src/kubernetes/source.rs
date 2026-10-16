//! Watches the cluster and sends its hosts to the reconciler (ADR 0005), as the kubernetes
//! route source of ADR 0001

use std::{collections::HashMap, fmt::Debug, sync::Arc, time::Duration};

use futures_util::StreamExt;
use k8s_openapi::{
    api::{
        core::v1::{Secret, Service},
        discovery::v1::EndpointSlice,
        networking::v1::{Ingress, IngressClass},
    },
    NamespaceResourceScope,
};
use kube::{
    api::{Patch, PatchParams},
    runtime::{
        events::{Event, EventType, Recorder, Reporter},
        reflector, watcher, WatchStreamExt,
    },
    Api, Client, Resource,
};
use serde::de::DeserializeOwned;
use tokio::{sync::Notify, task::JoinHandle};

use crate::{
    config::kubernetes::KubernetesConfig,
    routing::{RouteSnapshot, RouteSourceKind, SnapshotSender},
};

use super::{
    certificates::parse,
    convert::{hosts, of_class, Cluster, Converted, HostCertificate, Invalid, AUTH_LABEL},
    status,
};

/// Changes often come together (a deployment rolling out): they're applied once settled
const SETTLE: Duration = Duration::from_millis(500);
/// How often the status of `Ingress`es is checked, besides changes
const STATUS_INTERVAL: Duration = Duration::from_secs(30);
/// Held by the replica writing the status of `Ingress`es (ADR 0002)
const STATUS_LEASE: &str = "kubernetes:status";

/// Watches the cluster, forever (reconnecting after errors)
pub async fn run(config: KubernetesConfig, sender: SnapshotSender) {
    tracing::info!(
        "watching Ingresses of class {:?} in {}",
        config.ingress_class,
        if config.namespaces.is_empty() {
            "all namespaces".to_string()
        } else {
            format!("namespaces {:?}", config.namespaces)
        }
    );
    loop {
        if let Err(err) = watch(&config, &sender).await {
            tracing::warn!("kubernetes: {err}, retrying in 10 seconds");
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn watch(config: &KubernetesConfig, sender: &SnapshotSender) -> Result<(), String> {
    // In the cluster, the pod's service account; outside, the kubeconfig
    let client = Client::try_default()
        .await
        .map_err(|err| format!("failed to connect to the cluster: {err}"))?;
    let changed = Arc::new(Notify::new());

    let all = watcher::Config::default();
    let ingresses = Watched::<Ingress>::namespaced(&client, &config.namespaces, &all, &changed);
    let services = Watched::<Service>::namespaced(&client, &config.namespaces, &all, &changed);
    let endpoint_slices =
        Watched::<EndpointSlice>::namespaced(&client, &config.namespaces, &all, &changed);
    // Only TLS secrets (`tls.crt` and `tls.key`)
    let tls = watcher::Config::default().fields("type=kubernetes.io/tls");
    let secrets = Watched::<Secret>::namespaced(&client, &config.namespaces, &tls, &changed);
    // And the ones labelled for `basic_auth` and `oauth2` settings
    let auth = watcher::Config::default().labels(AUTH_LABEL);
    let auth_secrets = Watched::<Secret>::namespaced(&client, &config.namespaces, &auth, &changed);
    let ingress_classes = Watched::<IngressClass>::cluster(&client, &changed);

    // Hosts are only sent once everything was listed, so none is removed by a partial view
    ingresses.ready().await?;
    services.ready().await?;
    endpoint_slices.ready().await?;
    secrets.ready().await?;
    auth_secrets.ready().await?;
    ingress_classes.ready().await?;

    let recorder = Recorder::new(
        client.clone(),
        Reporter {
            controller: "proksi".to_string(),
            instance: Some(crate::stores::hostname()),
        },
    );
    let mut reported: Vec<Invalid> = vec![];
    let mut certificates = HashMap::new();
    loop {
        let cluster = Cluster {
            ingresses: ingresses.state(),
            ingress_classes: ingress_classes.state(),
            services: services.state(),
            endpoint_slices: endpoint_slices.state(),
            secrets: [secrets.state(), auth_secrets.state()].concat(),
        };
        let Converted {
            hosts,
            certificates: wanted,
            invalid,
        } = hosts(&cluster, &config.ingress_class);
        if invalid != reported {
            for new in invalid.iter().filter(|invalid| !reported.contains(invalid)) {
                tracing::warn!("ingress {}: {}", new.ingress, new.reason);
                record(&recorder, &cluster, new).await;
            }
            reported = invalid;
        }

        // Before the hosts, so they're served with their certificate right away
        apply_certificates(&wanted, &mut certificates).await;

        sender
            .send(RouteSnapshot::new(RouteSourceKind::Kubernetes, hosts))
            .await
            .map_err(|_| "the routing service stopped".to_string())?;

        if let Some(publish) = &config.publish_service {
            let served = of_class(&cluster, &config.ingress_class);
            if let Err(err) = publish_status(&client, publish, &served).await {
                tracing::warn!("kubernetes: failed to update the status of Ingresses: {err}");
            }
        }

        // Also now and then, for a load balancer's address assigned later
        tokio::select! {
            () = changed.notified() => {}
            () = tokio::time::sleep(STATUS_INTERVAL) => {}
        }
        tokio::time::sleep(SETTLE).await;
    }
}

/// Writes the addresses of proksi's `Service` (`namespace/name`) to the status of the
/// `Ingress`es that don't have them, from one replica only (the one holding the lease)
async fn publish_status(
    client: &Client,
    publish: &str,
    ingresses: &[&Arc<Ingress>],
) -> Result<(), String> {
    let store = crate::stores::global::get_store();
    let leader = store
        .try_lock(STATUS_LEASE, crate::stores::replica_id(), STATUS_INTERVAL * 3)
        .await
        .unwrap_or(true);
    if !leader {
        return Ok(());
    }

    let (namespace, name) = publish
        .split_once('/')
        .ok_or_else(|| format!("publish_service must be namespace/name, not {publish:?}"))?;
    let service = Api::<Service>::namespaced(client.clone(), namespace)
        .get(name)
        .await
        .map_err(|err| format!("failed to read service {publish}: {err}"))?;
    let addresses = status::addresses(&service);

    let ingresses = ingresses.iter().map(|ingress| &***ingress).collect::<Vec<&Ingress>>();
    for (namespace, name) in status::outdated(&ingresses, &addresses) {
        let patch = serde_json::json!({ "status": { "loadBalancer": { "ingress": addresses } } });
        Api::<Ingress>::namespaced(client.clone(), &namespace)
            .patch_status(&name, &PatchParams::default(), &Patch::Merge(&patch))
            .await
            .map_err(|err| format!("{namespace}/{name}: {err}"))?;
    }
    Ok(())
}

/// A warning `Event` on an `Ingress` that couldn't be served (`kubectl describe ingress`)
async fn record(recorder: &Recorder, cluster: &Cluster, invalid: &Invalid) {
    let Some(ingress) = cluster.ingresses.iter().find(|ingress| {
        let id = format!(
            "{}/{}",
            ingress.metadata.namespace.as_deref().unwrap_or_default(),
            ingress.metadata.name.as_deref().unwrap_or_default()
        );
        id == invalid.ingress
    }) else {
        return;
    };
    let event = Event {
        type_: EventType::Warning,
        reason: "Invalid".to_string(),
        note: Some(invalid.reason.clone()),
        action: "Serving".to_string(),
        secondary: None,
    };
    if let Err(err) = recorder.publish(&event, &ingress.object_ref(&())).await {
        tracing::debug!("kubernetes: failed to record an event: {err}");
    }
}

/// Puts the certificates of TLS secrets in the store (shared with the other replicas), when
/// their secret changed. `applied`: the secret and version of each host's certificate.
async fn apply_certificates(
    certificates: &[HostCertificate],
    applied: &mut HashMap<String, (String, Option<String>)>,
) {
    let store = crate::stores::global::get_store();
    for certificate in certificates {
        let host = certificate.host.to_lowercase();
        let version = (certificate.secret.clone(), certificate.version.clone());
        if applied.get(&host) == Some(&version) {
            continue;
        }
        match parse(&certificate.certificate, &certificate.key) {
            Ok(parsed) => {
                if let Err(err) = store.set_certificate(&host, parsed).await {
                    tracing::warn!("failed to store the certificate for {host}: {err}");
                    continue;
                }
                tracing::info!("certificate for {host} from secret {}", certificate.secret);
            }
            Err(err) => tracing::warn!("secret {} (for {host}): {err}", certificate.secret),
        }
        // Not retried until the secret changes
        applied.insert(host, version);
    }
}

/// The objects of a kind, kept up to date by watching them
struct Watched<K: Resource + 'static>
where
    K::DynamicType: std::hash::Hash + Eq + Clone,
{
    stores: Vec<reflector::Store<K>>,
    tasks: Vec<JoinHandle<()>>,
}

impl<K> Watched<K>
where
    K: Resource + Clone + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Default + std::hash::Hash + Eq + Clone,
{
    /// In every namespace, or only in `namespaces`
    fn namespaced(
        client: &Client,
        namespaces: &[String],
        config: &watcher::Config,
        changed: &Arc<Notify>,
    ) -> Self
    where
        K: Resource<Scope = NamespaceResourceScope>,
    {
        let apis = if namespaces.is_empty() {
            vec![Api::<K>::all(client.clone())]
        } else {
            namespaces
                .iter()
                .map(|namespace| Api::<K>::namespaced(client.clone(), namespace))
                .collect()
        };
        Self::watch(apis, config, changed)
    }

    /// Cluster-scoped objects
    fn cluster(client: &Client, changed: &Arc<Notify>) -> Self {
        let config = watcher::Config::default();
        Self::watch(vec![Api::<K>::all(client.clone())], &config, changed)
    }

    fn watch(apis: Vec<Api<K>>, config: &watcher::Config, changed: &Arc<Notify>) -> Self {
        let mut stores = vec![];
        let mut tasks = vec![];
        for api in apis {
            let (store, writer) = reflector::store();
            let changed = changed.clone();
            let stream = reflector(writer, watcher(api, config.clone())).default_backoff();
            tasks.push(tokio::spawn(stream.for_each(move |event| {
                if let Err(err) = &event {
                    tracing::warn!("kubernetes: watching failed, retrying: {err}");
                }
                changed.notify_one();
                std::future::ready(())
            })));
            stores.push(store);
        }
        Self { stores, tasks }
    }

    async fn ready(&self) -> Result<(), String> {
        for store in &self.stores {
            store
                .wait_until_ready()
                .await
                .map_err(|err| format!("watching stopped: {err}"))?;
        }
        Ok(())
    }

    fn state(&self) -> Vec<Arc<K>> {
        self.stores
            .iter()
            .flat_map(reflector::Store::state)
            .collect()
    }
}

impl<K: Resource + 'static> Drop for Watched<K>
where
    K::DynamicType: std::hash::Hash + Eq + Clone,
{
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
