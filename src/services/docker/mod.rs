use std::{
    borrow::Cow, collections::HashMap, hash::Hash, net::SocketAddr, str::FromStr, sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use bollard::{
    container::ListContainersOptions, service::ListServicesOptions, Docker, API_DEFAULT_VERSION,
};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::sync::broadcast::Sender;
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

#[derive(Debug, Default)]
pub struct ProksiDockerRoute {
    upstreams: Vec<String>,
    path_matchers: Vec<String>,
}

impl ProksiDockerRoute {
    pub fn new(upstreams: Vec<String>, path_matchers: Vec<String>) -> Self {
        Self {
            upstreams,
            path_matchers,
        }
    }
}

/// A service that will list all services in a Swarm OR containers through the Docker API
/// and update the route store with the new services.
/// This service will run in a separate thread.
pub struct LabelService {
    config: Arc<Config>,
    inner: Docker,
    sender: Sender<MsgProxy>,
}

impl LabelService {
    pub fn new(config: Arc<Config>, sender: Sender<MsgProxy>) -> Self {
        let endpoint = config.docker.endpoint.clone().unwrap_or_default();

        let docker = connect_to_docker(&endpoint);

        Self {
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
    async fn list_services<T>(
        &self,
        filters: HashMap<T, Vec<T>>,
    ) -> HashMap<String, ProksiDockerRoute>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let mut host_map = HashMap::<String, ProksiDockerRoute>::new();
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

        let services = services.unwrap();

        for service in &services {
            let service_id = service.id.as_ref().unwrap();
            let service_name = service.spec.as_ref().unwrap().name.as_ref();

            if service_name.is_none() {
                info!("Service {service_id:?} does not have a name");
                continue;
            }

            let service_name = service_name.unwrap();

            let falsy_string = String::from("false");
            let service_labels = service.spec.as_ref().unwrap().labels.as_ref().unwrap();
            let proksi_enabled = service_labels
                .get("proksi.enabled")
                .unwrap_or(&falsy_string);

            let empty_string = String::new();
            let proksi_host = service_labels.get("proksi.host").unwrap_or(&empty_string);
            let proksi_port = service_labels.get("proksi.port").unwrap_or(&empty_string);

            if proksi_enabled != "true" {
                info!(
                    "Service {service_name:?} does not have the label
                    proksi.enabled set to `true`"
                );
                continue;
            }

            if proksi_host.is_empty() || proksi_port.is_empty() {
                info!(
                    "Service {service_name:?} does not have the label
                    proksi.host set to a valid host or proksi.port set to a valid port"
                );
                continue;
            }

            // TODO offer an option to load balance directly to the container IPs
            // of the service instead of through the docker dns
            if !host_map.contains_key(proksi_host) {
                let mut routed = ProksiDockerRoute::default();
                routed
                    .upstreams
                    .push(format!("tasks.{service_name}:{proksi_port}"));
                host_map.insert(proksi_host.clone(), routed);
            }
        }

        host_map
    }

    /// Generate a list of containers based on the provided filters
    /// This will return a mapping between host <> ips for each container
    /// Does not work for docker in Swarm mode
    async fn list_containers<T>(
        &self,
        filters: HashMap<T, Vec<T>>,
    ) -> HashMap<String, ProksiDockerRoute>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let mut host_map = HashMap::<String, ProksiDockerRoute>::new();
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
            let container_names = &container.names;
            let container_labels = container.labels.as_ref().unwrap();

            let mut proxy_enabled = false;
            let mut proxy_host = "";
            let mut proxy_port = "";
            let mut match_with_path_patterns = vec![];

            // Map through extra labels
            for (k, v) in container_labels {
                if k.starts_with("proksi.") {
                    // direct values
                    match k.as_str() {
                        "proksi.enabled" => proxy_enabled = v == "true",
                        "proksi.host" => proxy_host = v,
                        "proksi.port" => proxy_port = v,
                        k if k.starts_with("proksi.match_with.path.pattern.") => {
                            match_with_path_patterns.push(v.clone());
                        }
                        _ => {}
                    }
                }
            }

            if !proxy_enabled {
                info!(
                    "Container {container_names:?} does not have the label
                    proksi.enabled set to `true`"
                );
                continue;
            }

            if proxy_port.is_empty() || proxy_host.is_empty() {
                info!(
                    "Container {container_names:?} does not have a
                  `proksi.port` label or a `proksi.host` label"
                );
                continue;
            }

            // Create a new entry in the host_map if it does not exist
            if !host_map.contains_key(proxy_host) {
                let routed = ProksiDockerRoute::new(vec![], match_with_path_patterns);
                host_map.insert(proxy_host.to_string(), routed);
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
                    debug!("Could not parse the ip address {ip_plus_port} of the container {container_names:?}");
                    continue;
                }

                host_map
                    .get_mut(proxy_host)
                    .unwrap()
                    .upstreams
                    .push(ip_plus_port);
            }
        }

        host_map
    }
}

#[async_trait]
impl Service for LabelService {
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
        interval.tick().await;
        loop {
            interval.tick().await;

            let hosts = match self.config.docker.mode {
                DockerServiceMode::Swarm => self.list_services(filters.clone()).await,
                DockerServiceMode::Container => self.list_containers(filters.clone()).await,
            };

            for (host, value) in hosts {
                // If no upstreams can be found, skip adding the route
                if value.upstreams.is_empty() {
                    continue;
                }

                let host_value: Cow<'static, str> = Cow::Owned(host.clone());

                // Notify the route discovery service of the new host
                self.sender
                    .send(MsgProxy::NewRoute(MsgRoute {
                        host: host_value,
                        upstreams: value.upstreams,
                        path_matchers: value.path_matchers,
                    }))
                    .ok();
            }
        }
    }

    fn name(&self) -> &'static str {
        "docker_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
