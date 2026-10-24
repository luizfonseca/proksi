//! The `proksi.*` labels of a container or a service, and the route they describe

use std::{borrow::Cow, collections::HashMap};

use serde_json::json;

use crate::config::{RouteHeaderAdd, RouteHeaderRemove, RoutePlugin};

use super::ProksiDockerRoute;

/// What the `proksi.*` labels say
#[derive(Default)]
pub(super) struct Labels<'a> {
    pub enabled: bool,
    pub host: &'a str,
    pub port: &'a str,
    path_patterns: Vec<String>,
    header_add: Option<Vec<RouteHeaderAdd>>,
    header_remove: Option<Vec<RouteHeaderRemove>>,
    self_signed_on_failure: bool,
    oauth2: Oauth2Labels,
    request_id: bool,
    basic_auth_user: Option<String>,
    basic_auth_password: Option<String>,
}

/// `proksi.plugins.oauth2.*`
#[derive(Default)]
struct Oauth2Labels {
    provider: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    jwt_secret: Option<String>,
    validations: Option<serde_json::Value>,
}

impl<'a> Labels<'a> {
    pub fn read(labels: &'a HashMap<String, String>) -> Self {
        let mut read = Self::default();
        for (key, value) in labels.iter().filter(|(key, _)| key.starts_with("proksi.")) {
            read.set(key, value);
        }
        read
    }

    fn set(&mut self, key: &str, value: &'a String) {
        match key {
            "proksi.enabled" => self.enabled = value == "true",
            "proksi.host" => self.host = value,
            "proksi.port" => self.port = value,
            key if key.starts_with("proksi.match_with.path.pattern.") => {
                self.path_patterns.push(value.clone());
            }
            "proksi.headers.add" => {
                self.header_add = Some(serde_json::from_str(value).unwrap_or_default());
            }
            "proksi.headers.remove" => {
                self.header_remove = Some(serde_json::from_str(value).unwrap_or_default());
            }
            "proksi.ssl_certificate.self_signed_on_failure" => {
                self.self_signed_on_failure = value == "true";
            }
            "proksi.plugins.oauth2.provider" => self.oauth2.provider = Some(value.clone()),
            "proksi.plugins.oauth2.client_id" => self.oauth2.client_id = Some(value.clone()),
            "proksi.plugins.oauth2.client_secret" => {
                self.oauth2.client_secret = Some(value.clone());
            }
            "proksi.plugins.oauth2.jwt_secret" => self.oauth2.jwt_secret = Some(value.clone()),
            "proksi.plugins.oauth2.validations" => {
                self.oauth2.validations =
                    Some(serde_json::from_str(value).unwrap_or_else(|_| json!([])));
            }
            "proksi.plugins.request_id.enabled" => self.request_id = value == "true",
            "proksi.plugins.basic_auth.user" => self.basic_auth_user = Some(value.clone()),
            "proksi.plugins.basic_auth.password" => {
                self.basic_auth_password = Some(value.clone());
            }
            _ => {}
        }
    }

    /// The route of a container: its headers and certificate settings (its upstreams are
    /// added from its networks)
    pub fn container_route(self) -> ProksiDockerRoute {
        let mut route = ProksiDockerRoute::new(vec![], self.path_patterns);
        route.host_header_add = self.header_add;
        route.host_header_remove = self.header_remove;
        route.ssl_certificate_self_signed_on_failure = self.self_signed_on_failure;
        route
    }

    /// The route of a Swarm service: through the docker DNS of its tasks, with its plugins
    pub fn service_route(self, service: &str) -> ProksiDockerRoute {
        let upstream = format!("tasks.{service}:{}", self.port);
        let mut plugins = vec![];
        if let Some(plugin) = self.oauth2.plugin() {
            plugins.push(plugin);
        }
        if self.request_id {
            plugins.push(RoutePlugin {
                name: Cow::Borrowed("request_id"),
                config: None,
            });
        }
        if let (Some(user), Some(pass)) = (self.basic_auth_user, self.basic_auth_password) {
            let config = HashMap::from([
                (Cow::Borrowed("user"), json!(user)),
                (Cow::Borrowed("pass"), json!(pass)),
            ]);
            plugins.push(RoutePlugin {
                name: Cow::Borrowed("basic_auth"),
                config: Some(config),
            });
        }

        let mut route = ProksiDockerRoute::new(vec![upstream], self.path_patterns);
        route.host_header_add = self.header_add;
        route.host_header_remove = self.header_remove;
        route.ssl_certificate_self_signed_on_failure = self.self_signed_on_failure;
        route.plugins = Some(plugins);
        route
    }
}

impl Oauth2Labels {
    /// The `oauth2` plugin, when every one of its labels is set
    fn plugin(self) -> Option<RoutePlugin> {
        let (
            Some(provider),
            Some(client_id),
            Some(client_secret),
            Some(jwt_secret),
            Some(validations),
        ) = (
            self.provider,
            self.client_id,
            self.client_secret,
            self.jwt_secret,
            self.validations,
        )
        else {
            return None;
        };
        let config = HashMap::from([
            (Cow::Borrowed("client_id"), json!(client_id)),
            (Cow::Borrowed("provider"), json!(provider)),
            (Cow::Borrowed("client_secret"), json!(client_secret)),
            (Cow::Borrowed("jwt_secret"), json!(jwt_secret)),
            (Cow::Borrowed("validations"), json!(validations)),
        ]);
        Some(RoutePlugin {
            name: Cow::Borrowed("oauth2"),
            config: Some(config),
        })
    }
}
