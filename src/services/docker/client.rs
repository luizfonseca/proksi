use std::{
    borrow::Cow, collections::HashMap, hash::Hash, net::SocketAddr, str::FromStr, sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use bollard::{
    container::ListContainersOptions, service::ListServicesOptions, Docker, API_DEFAULT_VERSION,
};
use crossbeam_channel::Sender;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tracing::{debug, info};

use crate::{
    config::{Config, DockerServiceMode},
    MsgProxy, MsgRoute,
};

/// Based on the provided endpoint, returns the correct Docker client
fn connect_to_docker(endpoint: &str) -> Result<Docker, bollard::errors::Error> {
    if endpoint.starts_with("unix:///") {
        return Docker::connect_with_unix(endpoint, 120, API_DEFAULT_VERSION);
    }
    if endpoint.starts_with("tcp://") || endpoint.starts_with("http") {
        return Docker::connect_with_http(endpoint, 120, API_DEFAULT_VERSION);
    }

    Docker::connect_with_local_defaults()
}

pub struct DockerService {
    config: Arc<Config>,
    inner: Docker,
    sender: Sender<MsgProxy>,
}

impl DockerService {
    pub fn new(config: Arc<Config>, sender: Sender<MsgProxy>) -> Self {
        let endpoint = config.docker.endpoint.clone().unwrap_or_default();

        let docker = connect_to_docker(&endpoint);

        DockerService {
            config,
            sender,
            inner: docker
                .map_err(|e| anyhow!("could not connect to the docker daemon: {e}"))
                .unwrap(),
        }
    }

    /// Generate a list of services based on the provided filters
    /// This will returns a mapping between host <> ips for each service
    /// Only works for docker in Swarm mode.
    async fn list_services<T>(&self, filters: HashMap<T, Vec<T>>) -> HashMap<String, Vec<String>>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let host_map = HashMap::<String, Vec<String>>::new();
        let services = self
            .inner
            .list_services(Some(ListServicesOptions {
                filters,
                status: true,
            }))
            .await;

        if services.is_err() {
            info!("Could not list services {:?}", services.err().unwrap());
            return host_map;
        }

        for service in services.unwrap() {
            println!("Found service {}", service.id.unwrap());
        }

        host_map
    }

    /// Generate a list of containers based on the provided filters
    /// This will returns a mapping between host <> ips for each container
    /// Does not work for docker in Swarm mode
    async fn list_containers<T>(&self, filters: HashMap<T, Vec<T>>) -> HashMap<String, Vec<String>>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let mut host_map = HashMap::<String, Vec<String>>::new();
        let containers = self
            .inner
            .list_containers(Some(ListContainersOptions {
                all: false,
                limit: Some(1000),
                filters,
                size: false,
            }))
            .await;

        if containers.is_err() {
            info!("Could not list containers {:?}", containers.err().unwrap());
            return host_map;
        }

        let containers = containers.unwrap();

        for container in &containers {
            // Get specified container labels
            let container_labels = container.labels.as_ref().unwrap();
            let default_bool = String::from("false");
            let proxy_enabled = container_labels
                .get("proksi.enabled")
                .unwrap_or(&default_bool);
            let proxy_host = container_labels.get("proksi.host");
            let proxy_port = container_labels.get("proksi.port");

            if proxy_enabled != "true" {
                info!("Container does not have the label proksi.enabled set to `true`");
                continue;
            }

            if proxy_port.is_none() || proxy_host.is_none() {
                info!("Container does not have a `proksi.port` label or a `proksi.host` label");
                continue;
            }

            let proxy_port = proxy_port.unwrap();
            let proxy_host = proxy_host.unwrap();

            // Create a new entry in the host_map if it does not exist
            if !host_map.contains_key(proxy_host) {
                host_map.insert(proxy_host.clone(), vec![]);
            }

            // map container endpoints
            let network_settings = &container.network_settings.as_ref().unwrap();
            let networks = network_settings.networks.as_ref().unwrap();

            for network in networks.values() {
                let ip_on_network = network.ip_address.as_ref().unwrap();
                let ip_plus_port = format!("{ip_on_network}:{proxy_port}");

                let socket_addr = SocketAddr::from_str(&ip_plus_port);

                // skip values from networks that Proksi does not have access to
                if ip_on_network.is_empty() || socket_addr.is_err() {
                    debug!(
                        "Could not parse the ip address of the container: {}",
                        ip_plus_port
                    );
                    continue;
                }

                host_map.get_mut(proxy_host).unwrap().push(ip_plus_port);
            }
        }

        host_map
    }
}

#[async_trait]
impl Service for DockerService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, mut _shutdown: ShutdownWatch) {
        info!(service = "docker", "Started Docker service");

        // By default every container or service should have these 3 labels
        // So that Proksi can route the appropriate traffic
        let mut filters = HashMap::new();
        filters.insert(
            "label",
            vec!["proksi.enabled=true", "proksi.host", "proksi.port"],
        );

        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;

            let hosts = match self.config.docker.mode {
                DockerServiceMode::Swarm => self.list_services(filters.clone()).await,
                DockerServiceMode::Standalone => self.list_containers(filters.clone()).await,
            };

            for (host, ips) in hosts {
                if ips.is_empty() {
                    continue;
                }

                let host_value: Cow<'static, str> = Cow::Owned(host.clone());

                // Notify the route store of the new host
                self.sender
                    .try_send(MsgProxy::NewRoute(MsgRoute {
                        host: host_value,
                        upstreams: ips,
                    }))
                    .ok();
            }
        }
    }

    fn name(&self) -> &'static str {
        "DockerService"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
