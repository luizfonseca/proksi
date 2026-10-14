use std::{
    borrow::Cow, collections::HashMap, hash::Hash, net::SocketAddr, str::FromStr, sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use bollard::{
    container::ListContainersOptions, service::ListServicesOptions, Docker, API_DEFAULT_VERSION,
};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use routes::DockerRoutes;
use serde_json::json;
use tracing::{debug, info};

use crate::{
    config::{Config, DockerServiceMode, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin},
    routing::{RouteSnapshot, RouteSourceKind, SnapshotSender},
};

pub mod routes;

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

/// A service that will list all services in a Swarm OR containers through the Docker API
/// and update the route store with the new services.
/// This service will run in a separate thread.
pub struct LabelService {
    config: Arc<Config>,
    inner: Option<Docker>,
    sender: SnapshotSender,
    routes: DockerRoutes,
}

impl LabelService {
    pub fn new(config: Arc<Config>, sender: SnapshotSender) -> Self {
        let endpoint = config.docker.endpoint.clone().unwrap_or_default();

        let docker = match connect_to_docker(&endpoint) {
            Ok(docker) => Some(docker),
            Err(e) => {
                if config.docker.enabled.unwrap_or(false) {
                    tracing::error!("could not connect to the docker daemon: {e}");
                }
                None
            }
        };

        Self {
            config,
            sender,
            routes: DockerRoutes::default(),
            inner: docker,
        }
    }

    /// Generate a list of services based on the provided filters
    /// This will returns a mapping between host <> ips for each service
    /// Only works for docker in Swarm mode.
    #[allow(clippy::too_many_lines)]
    async fn list_services<T>(
        &self,
        filters: HashMap<T, Vec<T>>,
    ) -> HashMap<String, ProksiDockerRoute>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let mut host_map = HashMap::<String, ProksiDockerRoute>::new();
        let Some(docker) = &self.inner else {
            return host_map;
        };

        let services = match docker
            .list_services(Some(ListServicesOptions {
                filters,
                status: true,
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

            let mut proxy_enabled = false;
            let mut proxy_host = "";
            let mut proxy_port = "";
            let mut match_with_path_patterns = vec![];
            let mut route_header_add: Option<Vec<RouteHeaderAdd>> = None;
            let mut route_header_remove: Option<Vec<RouteHeaderRemove>> = None;

            // TEMP: Oauth2 plugin
            let mut oauth2_provider: Option<String> = None;
            let mut oauth2_client_id: Option<String> = None;
            let mut oauth2_client_secret: Option<String> = None;
            let mut oauth2_jwt_secret: Option<String> = None;
            let mut oauth2_validations: Option<serde_json::Value> = None;
            let mut ssl_certificate_self_signed_on_failure = false;
            let mut docker_request_id = false;
            let mut basic_auth_user = None;
            let mut basic_auth_password = None;

            // Map through extra labels
            for (k, v) in service_labels {
                if k.starts_with("proksi.") {
                    // direct values
                    // TODO refactor to be reused for both services and containers
                    match k.as_str() {
                        "proksi.enabled" => proxy_enabled = v == "true",
                        "proksi.host" => proxy_host = v,
                        "proksi.port" => proxy_port = v,
                        k if k.starts_with("proksi.match_with.path.pattern.") => {
                            match_with_path_patterns.push(v.clone());
                        }
                        "proksi.headers.add" => {
                            let deser: Vec<RouteHeaderAdd> =
                                serde_json::from_str(v).unwrap_or(vec![]);

                            route_header_add = Some(deser);
                        }
                        "proksi.headers.remove" => {
                            let deser: Vec<RouteHeaderRemove> =
                                serde_json::from_str(v).unwrap_or(vec![]);

                            route_header_remove = Some(deser);
                        }
                        "proksi.ssl_certificate.self_signed_on_failure" => {
                            ssl_certificate_self_signed_on_failure = v == "true";
                        }
                        "proksi.plugins.oauth2.provider" => oauth2_provider = Some(v.clone()),
                        "proksi.plugins.oauth2.client_id" => oauth2_client_id = Some(v.clone()),
                        "proksi.plugins.oauth2.client_secret" => {
                            oauth2_client_secret = Some(v.clone());
                        }
                        "proksi.plugins.oauth2.jwt_secret" => oauth2_jwt_secret = Some(v.clone()),
                        "proksi.plugins.oauth2.validations" => {
                            oauth2_validations =
                                Some(serde_json::from_str(v).unwrap_or_else(|_| json!([])));
                        }
                        "proksi.plugins.request_id.enabled" => {
                            docker_request_id = v == "true";
                        }
                        "proksi.plugins.basic_auth.user" => basic_auth_user = Some(v.clone()),
                        "proksi.plugins.basic_auth.password" => {
                            basic_auth_password = Some(v.clone());
                        }

                        _ => {}
                    }
                }
            }

            if !proxy_enabled {
                info!(
                    "Service {service_name:?} does not have the label
                    proksi.enabled set to `true`"
                );
                continue;
            }

            if proxy_host.is_empty() || proxy_port.is_empty() {
                info!(
                    "Service {service_name:?} does not have the label
                    proksi.host set to a valid host or proksi.port set to a valid port"
                );
                continue;
            }

            // TODO offer an option to load balance directly to the container IPs
            // of the service instead of through the docker dns
            if !host_map.contains_key(proxy_host) {
                let mut routed = ProksiDockerRoute::default();
                routed
                    .upstreams
                    .push(format!("tasks.{service_name}:{proxy_port}"));
                routed.path_matchers = match_with_path_patterns;
                routed.host_header_add = route_header_add;
                routed.host_header_remove = route_header_remove;
                routed.ssl_certificate_self_signed_on_failure =
                    ssl_certificate_self_signed_on_failure;

                // This part is optional
                let mut plugins: Vec<RoutePlugin> = vec![];
                if let Some(plugin) = Self::get_oauth2_plugin(
                    oauth2_provider,
                    oauth2_client_id,
                    oauth2_client_secret,
                    oauth2_jwt_secret,
                    oauth2_validations,
                ) {
                    plugins.push(plugin);
                }

                if docker_request_id {
                    plugins.push(RoutePlugin {
                        name: Cow::Borrowed("request_id"),
                        config: None,
                    });
                }

                if let (Some(user), Some(pass)) = (basic_auth_user, basic_auth_password) {
                    let mut map = HashMap::new();
                    map.insert(Cow::Borrowed("user"), json!(user));
                    map.insert(Cow::Borrowed("pass"), json!(pass));

                    plugins.push(RoutePlugin {
                        name: Cow::Borrowed("basic_auth"),
                        config: Some(map),
                    });
                }

                routed.plugins = Some(plugins);
                host_map.insert(proxy_host.to_string(), routed);
            }
        }

        host_map
    }

    /// Generate a list of containers based on the provided filters
    /// This will return a mapping between host <> ips for each container
    /// Does not work for docker in Swarm mode
    #[allow(clippy::too_many_lines)]
    async fn list_containers<T>(
        &self,
        filters: HashMap<T, Vec<T>>,
    ) -> HashMap<String, ProksiDockerRoute>
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let mut host_map = HashMap::<String, ProksiDockerRoute>::new();
        let Some(docker) = &self.inner else {
            return host_map;
        };

        let containers = match docker
            .list_containers(Some(ListContainersOptions {
                all: false,
                limit: Some(1000),
                filters,
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

            let mut proxy_enabled = false;
            let mut proxy_host = "";
            let mut proxy_port = "";
            let mut match_with_path_patterns = vec![];
            let mut route_header_add: Option<Vec<RouteHeaderAdd>> = None;
            let mut route_header_remove: Option<Vec<RouteHeaderRemove>> = None;
            let mut ssl_certificate_self_signed_on_failure = false;

            // Map through extra labels
            for (k, v) in container_labels {
                if k.starts_with("proksi.") {
                    // direct values
                    match k.as_str() {
                        "proksi.enabled" => proxy_enabled = v == "true",
                        "proksi.host" => proxy_host = v,
                        "proksi.port" => proxy_port = v,
                        "proksi.headers.add" => {
                            let deser: Vec<RouteHeaderAdd> =
                                serde_json::from_str(v).unwrap_or(vec![]);

                            route_header_add = Some(deser);
                        }
                        "proksi.headers.remove" => {
                            let deser: Vec<RouteHeaderRemove> =
                                serde_json::from_str(v).unwrap_or(vec![]);

                            route_header_remove = Some(deser);
                        }
                        "proksi.ssl_certificate.self_signed_on_failure" => {
                            ssl_certificate_self_signed_on_failure = v == "true";
                        }
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
                let mut routed = ProksiDockerRoute::new(vec![], match_with_path_patterns);
                routed.host_header_add = route_header_add;
                routed.host_header_remove = route_header_remove;
                routed.ssl_certificate_self_signed_on_failure =
                    ssl_certificate_self_signed_on_failure;
                host_map.insert(proxy_host.to_string(), routed);
            }

            // map container endpoints
            let Some(networks) = container
                .network_settings
                .as_ref()
                .and_then(|settings| settings.networks.as_ref())
            else {
                info!("Container {container_names:?} does not have networks");
                continue;
            };

            for network in networks.values() {
                let Some(ip_on_network) = network.ip_address.as_ref() else {
                    continue;
                };
                let ip_plus_port = format!("{ip_on_network}:{proxy_port}");

                let socket_addr = SocketAddr::from_str(&ip_plus_port);

                // skip values from networks that Proksi does not have access to
                if ip_on_network.is_empty() || socket_addr.is_err() {
                    debug!("Could not parse the ip address {ip_plus_port} of the container {container_names:?}");
                    continue;
                }

                if let Some(route) = host_map.get_mut(proxy_host) {
                    route.upstreams.push(ip_plus_port);
                }
            }
        }

        host_map
    }

    // Parses the oauth2 configuration and returns a RoutePlugin
    fn get_oauth2_plugin(
        provider: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        jwt_secret: Option<String>,
        validations: Option<serde_json::Value>,
    ) -> Option<RoutePlugin> {
        let (
            Some(provider),
            Some(client_id),
            Some(client_secret),
            Some(jwt_secret),
            Some(validations),
        ) = (provider, client_id, client_secret, jwt_secret, validations)
        else {
            return None;
        };

        let mut plugin_hashmap = HashMap::new();
        plugin_hashmap.insert(Cow::Borrowed("client_id"), json!(client_id));

        plugin_hashmap.insert(Cow::Borrowed("provider"), json!(provider));

        plugin_hashmap.insert(Cow::Borrowed("client_secret"), json!(client_secret));
        plugin_hashmap.insert(Cow::Borrowed("jwt_secret"), json!(jwt_secret));
        plugin_hashmap.insert(Cow::Borrowed("validations"), json!(validations));

        Some(RoutePlugin {
            name: Cow::Borrowed("oauth2"),
            config: Some(plugin_hashmap),
        })
    }

    // By default every container or service should have these 3 labels
    // So that Proksi can route the appropriate traffic
    async fn get_routes_from_docker(&self) -> HashMap<String, ProksiDockerRoute> {
        let mut filters = HashMap::new();
        filters.insert(
            "label",
            vec!["proksi.enabled=true", "proksi.host", "proksi.port"],
        );

        match self.config.docker.mode {
            DockerServiceMode::Swarm => self.list_services(filters).await,
            DockerServiceMode::Container => self.list_containers(filters).await,
        }
    }
}

#[async_trait]
impl Service for LabelService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        mut _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.config.docker.enabled.is_some_and(|v| !v) {
            // Nothing to do, docker is disabled
            return;
        }

        info!(service = "docker", "Started Docker service");

        let mut interval = tokio::time::interval(Duration::from_secs(
            self.config.docker.interval_secs.unwrap_or(15),
        ));

        interval.tick().await;
        loop {
            interval.tick().await;
            let found = self.get_routes_from_docker().await;
            let routes = self.routes.update(found);
            let snapshot = RouteSnapshot::new(RouteSourceKind::Docker, routes);
            if self.sender.send(snapshot).await.is_err() {
                tracing::error!("failed to send the routes found in docker");
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
