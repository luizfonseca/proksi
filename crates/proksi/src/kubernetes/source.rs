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
    runtime::{reflector, watcher, WatchStreamExt},
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
    convert::{hosts, Cluster, Converted, HostCertificate, Invalid},
};

/// Changes often come together (a deployment rolling out): they're applied once settled
const SETTLE: Duration = Duration::from_millis(500);

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
    let ingress_classes = Watched::<IngressClass>::cluster(&client, &changed);

    // Hosts are only sent once everything was listed, so none is removed by a partial view
    ingresses.ready().await?;
    services.ready().await?;
    endpoint_slices.ready().await?;
    secrets.ready().await?;
    ingress_classes.ready().await?;

    let mut reported: Vec<Invalid> = vec![];
    let mut certificates = HashMap::new();
    loop {
        let cluster = Cluster {
            ingresses: ingresses.state(),
            ingress_classes: ingress_classes.state(),
            services: services.state(),
            endpoint_slices: endpoint_slices.state(),
            secrets: secrets.state(),
        };
        let Converted {
            hosts,
            certificates: wanted,
            invalid,
        } = hosts(&cluster, &config.ingress_class);
        if invalid != reported {
            for invalid in &invalid {
                tracing::warn!("ingress {}: {}", invalid.ingress, invalid.reason);
            }
            reported = invalid;
        }

        // Before the hosts, so they're served with their certificate right away
        apply_certificates(&wanted, &mut certificates).await;

        sender
            .send(RouteSnapshot::new(RouteSourceKind::Kubernetes, hosts))
            .await
            .map_err(|_| "the routing service stopped".to_string())?;

        changed.notified().await;
        tokio::time::sleep(SETTLE).await;
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
