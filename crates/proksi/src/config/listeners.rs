//! The addresses proksi listens on, worked out from its hosts

use std::collections::BTreeSet;

use super::{Config, StoreType};

/// Addresses to open, by protocol
#[derive(Debug, Default, PartialEq)]
pub struct Listeners {
    pub https: BTreeSet<String>,
    pub http: BTreeSet<String>,
}

/// The addresses the hosts of the configuration need:
/// - a host is served on its `listen` addresses, or on the default address
///   (`server.https_address`, or `server.http_address` with TLS disabled)
/// - hosts from `routes`, docker labels and the store use the default addresses
///   (they can appear while proksi is running, when ports can't be opened anymore)
/// - with any HTTPS address, the default HTTP address is opened too
///   (redirects to HTTPS and Let's Encrypt challenges, which only use port 80)
/// - without any host, the default addresses are opened, as before hosts could choose
pub fn listeners(config: &Config) -> Result<Listeners, String> {
    let default_https = config
        .server
        .https_address
        .as_deref()
        .unwrap_or("0.0.0.0:443");
    let default_http = config
        .server
        .http_address
        .as_deref()
        .unwrap_or("0.0.0.0:80");

    let mut listeners = Listeners::default();
    let hosts = config.hosts();
    for host in &hosts {
        let (target, default) = if host.tls.enabled {
            (&mut listeners.https, default_https)
        } else {
            (&mut listeners.http, default_http)
        };
        if host.listen.is_empty() {
            target.insert(default.to_string());
        } else {
            target.extend(host.listen.iter().cloned());
        }
    }

    let dynamic_hosts =
        config.docker.enabled.unwrap_or(false) || config.store.store_type != StoreType::Memory;
    if hosts.is_empty() || dynamic_hosts {
        listeners.https.insert(default_https.to_string());
    }
    if !listeners.https.is_empty() {
        listeners.http.insert(default_http.to_string());
    }

    if let Some(address) = listeners.https.intersection(&listeners.http).next() {
        return Err(format!(
            "address {address} is used both for HTTP and HTTPS hosts"
        ));
    }
    Ok(listeners)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        config::{blocks::HostBlock, hcl::HclFormat, Config},
        routing::test_utils::route,
    };

    use super::{listeners, Listeners};

    fn config(hcl: &str) -> Config {
        #[derive(serde::Deserialize)]
        struct Doc {
            host: std::collections::BTreeMap<String, HostBlock>,
        }
        let doc = <HclFormat as figment::providers::Format>::from_str::<Doc>(hcl).unwrap();
        let mut config = Config::default();
        config.host = doc.host;
        config
    }

    fn set(addresses: &[&str]) -> BTreeSet<String> {
        addresses.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn test_default_ports_without_hosts() {
        assert_eq!(
            listeners(&Config::default()),
            Ok(Listeners {
                https: set(&["0.0.0.0:443"]),
                http: set(&["0.0.0.0:80"]),
            })
        );
    }

    #[test]
    fn test_hosts_without_tls_only_need_http() {
        let config = config("host \"*\" {\n  tls = false\n}\n");
        assert_eq!(
            listeners(&config),
            Ok(Listeners {
                https: set(&[]),
                http: set(&["0.0.0.0:80"]),
            })
        );
    }

    #[test]
    fn test_hosts_listening_on_other_ports() {
        let config = config(
            "host \"*\" {\n  tls = false\n  listen = [\"0.0.0.0:8080\"]\n}\nhost \"a.test\" {\n  listen = [\"127.0.0.1:8443\"]\n}\n",
        );
        // Hosts with TLS also need the default HTTP port (redirects and ACME challenges)
        assert_eq!(
            listeners(&config),
            Ok(Listeners {
                https: set(&["127.0.0.1:8443"]),
                http: set(&["0.0.0.0:80", "0.0.0.0:8080"]),
            })
        );
    }

    #[test]
    fn test_routes_use_the_default_ports() {
        let mut config = config("host \"*\" {\n  tls = false\n  listen = [\"0.0.0.0:8080\"]\n}\n");
        config.routes = vec![route("a.test", "10.0.0.1")];
        assert_eq!(
            listeners(&config),
            Ok(Listeners {
                https: set(&["0.0.0.0:443"]),
                http: set(&["0.0.0.0:80", "0.0.0.0:8080"]),
            })
        );
    }

    #[test]
    fn test_docker_hosts_use_the_default_ports() {
        let mut config = config("host \"*\" {\n  tls = false\n  listen = [\"0.0.0.0:8080\"]\n}\n");
        config.docker.enabled = Some(true);
        assert_eq!(
            listeners(&config),
            Ok(Listeners {
                https: set(&["0.0.0.0:443"]),
                http: set(&["0.0.0.0:80", "0.0.0.0:8080"]),
            })
        );
    }

    #[test]
    fn test_address_used_for_http_and_https() {
        let config = config(
            "host \"*\" {\n  tls = false\n  listen = [\"0.0.0.0:8080\"]\n}\nhost \"a.test\" {\n  listen = [\"0.0.0.0:8080\"]\n}\n",
        );
        assert_eq!(
            listeners(&config),
            Err("address 0.0.0.0:8080 is used both for HTTP and HTTPS hosts".to_string())
        );
    }
}
