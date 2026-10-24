use std::{collections::HashMap, hash::Hash, net::SocketAddr, str::FromStr};

use bollard::{
    query_parameters::{ListContainersOptions, ListServicesOptions},
    Docker, API_DEFAULT_VERSION,
};
use labels::Labels;
use tracing::{debug, info};

use crate::config::{Config, DockerServiceMode, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin};

mod labels;
pub mod routes;

/// Based on the provided endpoint, returns the correct Docker client
/// Docker's filters, as its API takes them
fn owned<T: Into<String>>(filters: HashMap<T, Vec<T>>) -> HashMap<String, Vec<String>> {
    filters
        .into_iter()
        .map(|(key, values)| (key.into(), values.into_iter().map(Into::into).collect()))
        .collect()
}

fn connect_to_docker(endpoint: &str) -> Result<Docker, bollard::errors::Error> {
    if endpoint.starts_with("unix:///") {
        return Docker::connect_with_unix(endpoint, 120, API_DEFAULT_VERSION);
    }
    if endpoint.starts_with("tcp://") || endpoint.starts_with("http") {
        return Docker::connect_with_http(endpoint, 120, API_DEFAULT_VERSION);
    }

    Docker::connect_with_local_defaults()
}

/// A container's address on a network, with `port`: none on networks Proksi can't reach
fn network_address(
    network: &bollard::models::EndpointSettings,
    port: &str,
    container: Option<&[String]>,
) -> Option<String> {
    let ip = network.ip_address.as_ref()?;
    let address = format!("{ip}:{port}");
    if ip.is_empty() || SocketAddr::from_str(&address).is_err() {
        debug!("Could not parse the ip address {address} of the container {container:?}");
        return None;
    }
    Some(address)
}

#[derive(Debug, Default)]
pub struct ProksiDockerRoute {
    pub(crate) upstreams: Vec<String>,
    pub(crate) path_matchers: Vec<String>,

    pub(crate) host_header_add: Option<Vec<RouteHeaderAdd>>,
    pub(crate) host_header_remove: Option<Vec<RouteHeaderRemove>>,
    pub(crate) ssl_certificate_self_signed_on_failure: bool,
    pub(crate) plugins: Option<Vec<RoutePlugin>>,
}

impl ProksiDockerRoute {
    pub fn new(upstreams: Vec<String>, path_matchers: Vec<String>) -> Self {
        Self {
            upstreams,
            path_matchers,
            host_header_add: None,
            host_header_remove: None,
            ssl_certificate_self_signed_on_failure: false,
            plugins: None,
        }
    }
}

/// The Docker client for `config.docker.endpoint`: none when it can't connect
pub fn connect(config: &Config) -> Option<Docker> {
    let endpoint = config.docker.endpoint.clone().unwrap_or_default();
    match connect_to_docker(&endpoint) {
        Ok(docker) => Some(docker),
        Err(e) => {
            if config.docker.enabled.unwrap_or(false) {
                tracing::error!("could not connect to the docker daemon: {e}");
            }
            None
        }
    }
}

// By default every container or service should have these 3 labels
// So that Proksi can route the appropriate traffic
pub async fn routes(docker: &Docker, mode: &DockerServiceMode) -> HashMap<String, ProksiDockerRoute> {
    let mut filters = HashMap::new();
    filters.insert(
        "label",
        vec!["proksi.enabled=true", "proksi.host", "proksi.port"],
    );

    match mode {
        DockerServiceMode::Swarm => list_services(docker, filters).await,
        DockerServiceMode::Container => list_containers(docker, filters).await,
    }
}

/// Generate a list of services based on the provided filters
/// This will returns a mapping between host <> ips for each service
/// Only works for docker in Swarm mode.
#[allow(clippy::too_many_lines)]
async fn list_services<T>(
    docker: &Docker,
    filters: HashMap<T, Vec<T>>,
) -> HashMap<String, ProksiDockerRoute>
where
    T: Into<String> + Hash + serde::ser::Serialize + Eq,
{
    let mut host_map = HashMap::<String, ProksiDockerRoute>::new();

    let services = match docker
        .list_services(Some(ListServicesOptions {
            filters: Some(owned(filters)),
            status: Some(true),
        }))
        .await
    {
        Ok(services) => services,
        Err(err) => {
            info!("Could not list services {err:?}");
            return host_map;
        }
    };

    for service in services {
        let service_id = service.id.unwrap_or_default();
        let Some(service_spec) = service.spec.clone() else {
            info!("Service {service_id:?} does not have a spec");
            continue;
        };

        let Some(service_name) = service_spec.name.as_ref() else {
            info!("Service {service_id:?} does not have a name");
            continue;
        };

        let Some(service_labels) = service_spec.labels.as_ref() else {
            info!("Service {service_id:?} does not have labels");
            continue;
        };

        let labels = Labels::read(service_labels);
        if !labels.enabled {
            info!(
                "Service {service_name:?} does not have the label
                proksi.enabled set to `true`"
            );
            continue;
        }

        if labels.host.is_empty() || labels.port.is_empty() {
            info!(
                "Service {service_name:?} does not have the label
                proksi.host set to a valid host or proksi.port set to a valid port"
            );
            continue;
        }

        // TODO offer an option to load balance directly to the container IPs
        // of the service instead of through the docker dns
        let host = labels.host.to_string();
        host_map
            .entry(host)
            .or_insert_with(|| labels.service_route(service_name));
    }

    host_map
}

/// Generate a list of containers based on the provided filters
/// This will return a mapping between host <> ips for each container
/// Does not work for docker in Swarm mode
#[allow(clippy::too_many_lines)]
async fn list_containers<T>(
    docker: &Docker,
    filters: HashMap<T, Vec<T>>,
) -> HashMap<String, ProksiDockerRoute>
where
    T: Into<String> + Hash + serde::ser::Serialize + Eq,
{
    let mut host_map = HashMap::<String, ProksiDockerRoute>::new();

    let containers = match docker
        .list_containers(Some(ListContainersOptions {
            all: false,
            limit: Some(1000),
            filters: Some(owned(filters)),
            size: false,
        }))
        .await
    {
        Ok(containers) => containers,
        Err(err) => {
            info!("Could not list containers {err:?}");
            return host_map;
        }
    };

    for container in containers {
        // Get specified container labels
        let container_names = &container.names;

        let Some(container_labels) = container.labels.as_ref() else {
            info!("Container {container_names:?} does not have labels");
            continue;
        };

        let labels = Labels::read(container_labels);
        if !labels.enabled {
            info!(
                "Container {container_names:?} does not have the label
                proksi.enabled set to `true`"
            );
            continue;
        }

        if labels.port.is_empty() || labels.host.is_empty() {
            info!(
                "Container {container_names:?} does not have a
              `proksi.port` label or a `proksi.host` label"
            );
            continue;
        }
        let (host, port) = (labels.host, labels.port);

        // Create a new entry in the host_map if it does not exist
        let route = host_map
            .entry(host.to_string())
            .or_insert_with(|| labels.container_route());

        // map container endpoints
        let Some(networks) = container
            .network_settings
            .as_ref()
            .and_then(|settings| settings.networks.as_ref())
        else {
            info!("Container {container_names:?} does not have networks");
            continue;
        };
        let addresses = networks
            .values()
            .filter_map(|network| network_address(network, port, container_names.as_deref()));
        route.upstreams.extend(addresses);
    }

    host_map
}
