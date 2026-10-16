//! The addresses proksi listens on, worked out from its hosts

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{SocketAddr, ToSocketAddrs},
};

use super::{
    admin::AdminConfig,
    host::Host,
    tcp::{TcpRoute, TcpTls},
    Config, StoreType,
};

/// Addresses to open, by protocol
#[derive(Debug, Default, PartialEq)]
pub struct Listeners {
    pub https: BTreeSet<String>,
    pub http: BTreeSet<String>,
    /// `tcp` blocks, each on its own address (except blocks of different hosts with TLS)
    pub tcp: Vec<TcpRoute>,
    /// The admin API (`admin { ... }`), if enabled
    pub admin: Option<SocketAddr>,
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
    listeners.tcp = tcp_routes(config, &hosts, &listeners)?;
    listeners.admin = config
        .admin
        .as_ref()
        .map(|admin| admin_address(admin, &listeners))
        .transpose()?;
    Ok(listeners)
}

/// The admin API's address, on a port of its own
fn admin_address(admin: &AdminConfig, listeners: &Listeners) -> Result<SocketAddr, String> {
    let address = admin.address()?;
    // Also an address covering it (`0.0.0.0`, `[::]`) on the same port
    let overlaps = |other: &str| {
        other.parse::<SocketAddr>().is_ok_and(|other| {
            other.port() == address.port()
                && (other.ip() == address.ip()
                    || other.ip().is_unspecified()
                    || address.ip().is_unspecified())
        })
    };
    let used = listeners.https.iter().chain(&listeners.http).any(|a| overlaps(a))
        || listeners.tcp.iter().any(|route| overlaps(&route.listen));
    if used {
        return Err(format!(
            "admin: address {address} is also used by hosts or tcp blocks"
        ));
    }
    Ok(address)
}

/// The `tcp` blocks, each on an address of its own: blocks of different hosts with TLS can
/// share one, also with HTTPS hosts (the host is chosen by SNI), but not with HTTP hosts
fn tcp_routes(
    config: &Config,
    hosts: &[Host],
    http_and_https: &Listeners,
) -> Result<Vec<TcpRoute>, String> {
    let routes = config.tcp_routes()?;

    let mut by_address: BTreeMap<&str, Vec<&TcpRoute>> = BTreeMap::new();
    for route in &routes {
        let label = route.label();
        let address = route.listen.as_str();
        if address.parse::<SocketAddr>().is_err() {
            return Err(format!(
                "{label}: `listen` must be an ip:port address, not {address:?}"
            ));
        }
        if route.upstreams.is_empty() {
            return Err(format!("{label}: needs at least one upstream"));
        }
        if http_and_https.http.contains(address) {
            return Err(format!(
                "{label}: address {address} is also used by HTTP hosts"
            ));
        }
        let https = http_and_https.https.contains(address);
        if https {
            if route.tls == TcpTls::None {
                return Err(format!(
                    "{label}: address {address} is also used by HTTPS hosts (only tcp blocks in a host with TLS can share it, chosen by SNI)"
                ));
            }
            // The host's connections go to the block, unless it is chosen by its protocols
            let has_locations = hosts
                .iter()
                .any(|host| Some(&host.name) == route.host.as_ref() && !host.locations.is_empty());
            if has_locations && route.tls == TcpTls::Passthrough {
                return Err(format!(
                    "{label}: the host also has locations on {address} (TLS passthrough is chosen by name only, so the host is either HTTPS or TCP there)"
                ));
            }
            if has_locations && route.alpn.is_empty() {
                return Err(format!(
                    "{label}: the host also has locations on {address}, so the block needs `alpn` to tell them apart"
                ));
            }
        }

        let sharing = by_address.entry(address).or_default();
        for other in sharing.iter() {
            let tls = |route: &TcpRoute| route.tls != TcpTls::None;
            if !tls(other) || !tls(route) || other.host == route.host {
                return Err(format!(
                    "{} and {label} can't share address {address} (only tcp blocks of different hosts with TLS can, chosen by SNI)",
                    other.label()
                ));
            }
            // Telling them apart takes reading the name before the handshake, which proksi
            // does on HTTPS addresses only
            if other.tls != route.tls && !https {
                return Err(format!(
                    "{} and {label} can't share address {address} (TLS passthrough and TLS terminated by proksi only share the HTTPS address)",
                    other.label()
                ));
            }
        }
        sharing.push(route);
    }
    Ok(routes)
}

/// The default HTTPS and HTTP addresses (`server.https_address`, `server.http_address`),
/// where hosts without `listen` addresses are served
pub fn default_addresses(config: &Config) -> Result<(SocketAddr, SocketAddr), String> {
    let resolve = |name: &str, address: Option<&str>, default: &str| {
        let address = address.unwrap_or(default);
        address
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next())
            .ok_or_else(|| format!("{name} is not a valid address: {address:?}"))
    };
    Ok((
        resolve(
            "server.https_address",
            config.server.https_address.as_deref(),
            "0.0.0.0:443",
        )?,
        resolve(
            "server.http_address",
            config.server.http_address.as_deref(),
            "0.0.0.0:80",
        )?,
    ))
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
            #[serde(default)]
            host: std::collections::BTreeMap<String, HostBlock>,
            #[serde(default)]
            tcp: std::collections::BTreeMap<String, crate::config::blocks::TcpBlock>,
        }
        let doc = <HclFormat as figment::providers::Format>::from_str::<Doc>(hcl).unwrap();
        let mut config = Config::default();
        config.host = doc.host;
        config.tcp = doc.tcp;
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
                tcp: vec![],
                admin: None,
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
                tcp: vec![],
                admin: None,
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
                tcp: vec![],
                admin: None,
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
                tcp: vec![],
                admin: None,
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
                tcp: vec![],
                admin: None,
            })
        );
    }

    const TCP: &str = r#"
tcp "game" {
  listen = "0.0.0.0:7777"
  upstream "game:7777" {}
}

host "a.test" {
  tcp "db" {
    listen = "0.0.0.0:5433"
    upstream "db:5432" {}
  }
}

host "b.test" {
  tcp "db" {
    listen = "0.0.0.0:5433"
    upstream "db-b:5432" {}
  }
}
"#;

    #[test]
    fn test_tcp_addresses() {
        let listeners = listeners(&config(TCP)).unwrap();

        assert_eq!(listeners.https, set(&["0.0.0.0:443"]));
        assert_eq!(listeners.http, set(&["0.0.0.0:80"]));
        // Blocks of different hosts with TLS share an address (chosen by SNI)
        assert_eq!(
            listeners
                .tcp
                .iter()
                .map(|route| (route.label(), route.listen.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("tcp \"game\"".to_string(), "0.0.0.0:7777"),
                ("tcp \"db\" in host \"a.test\"".to_string(), "0.0.0.0:5433"),
                ("tcp \"db\" in host \"b.test\"".to_string(), "0.0.0.0:5433"),
            ]
        );
    }

    #[test]
    fn test_tcp_on_the_https_address() {
        let config = config(
            "host \"app.test\" {\n  location \"/\" {\n    upstream \"app:80\" {}\n  }\n}\nhost \"game.test\" {\n  tcp \"game\" {\n    listen = \"0.0.0.0:443\"\n    upstream \"game:7777\" {}\n  }\n}\n",
        );
        let listeners = listeners(&config).unwrap();

        // HTTPS and TCP on one address, chosen by SNI
        assert_eq!(listeners.https, set(&["0.0.0.0:443"]));
        assert_eq!(listeners.tcp.len(), 1);
        assert_eq!(listeners.tcp[0].listen, "0.0.0.0:443");
    }

    #[test]
    fn test_tcp_and_locations_of_a_host_chosen_by_alpn() {
        let config = config(
            "host \"a.test\" {\n  location \"/\" {\n    upstream \"app:80\" {}\n  }\n  tcp \"game\" {\n    listen = \"0.0.0.0:443\"\n    alpn = [\"game/1\"]\n    upstream \"game:7777\" {}\n  }\n}\n",
        );

        assert_eq!(listeners(&config).unwrap().tcp.len(), 1);
    }

    #[test]
    fn test_admin_address() {
        let mut with_admin = config("host \"a.test\" {}\n");
        with_admin.admin = Some(crate::config::admin::AdminConfig {
            listen: "127.0.0.1:7555".to_string(),
            token: None,
            metrics: true,
        });
        assert_eq!(
            listeners(&with_admin).unwrap().admin,
            Some("127.0.0.1:7555".parse().unwrap())
        );

        for address in ["0.0.0.0:80", "0.0.0.0:443", "0.0.0.0:7777"] {
            let mut config = config(
                "tcp \"game\" {\n  listen = \"0.0.0.0:7777\"\n  upstream \"game:7777\" {}\n}\nhost \"a.test\" {}\n",
            );
            config.admin = Some(crate::config::admin::AdminConfig {
                listen: address.to_string(),
                token: Some("secret".to_string()),
                metrics: true,
            });
            assert_eq!(
                listeners(&config),
                Err(format!("admin: address {address} is also used by hosts or tcp blocks")),
            );
        }
    }

    #[test]
    fn test_tls_passthrough_shares_addresses() {
        let config = config(
            "host \"app.test\" {\n  location \"/\" {\n    upstream \"app:80\" {}\n  }\n}\nhost \"a.test\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:443\"\n    tls = \"passthrough\"\n    upstream \"a:443\" {}\n  }\n  tcp \"b\" {\n    listen = \"0.0.0.0:8443\"\n    tls = \"passthrough\"\n    upstream \"a:443\" {}\n  }\n}\nhost \"b.test\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:443\"\n    upstream \"b:7777\" {}\n  }\n  tcp \"b\" {\n    listen = \"0.0.0.0:8443\"\n    tls = \"passthrough\"\n    upstream \"b:443\" {}\n  }\n}\n",
        );

        // With HTTPS hosts and TLS terminated by proksi on 443, with another host on 8443
        assert_eq!(listeners(&config).unwrap().tcp.len(), 4);
    }

    #[test]
    fn test_invalid_tcp_addresses() {
        for (hcl, expected) in [
            (
                "tcp \"a\" {\n  listen = \"7777\"\n  upstream \"b:1\" {}\n}\n",
                "tcp \"a\": `listen` must be an ip:port address, not \"7777\"",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:7777\"\n}\n",
                "tcp \"a\": needs at least one upstream",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:443\"\n  upstream \"b:1\" {}\n}\nhost \"h\" {}\n",
                "tcp \"a\": address 0.0.0.0:443 is also used by HTTPS hosts (only tcp blocks in a host with TLS can share it, chosen by SNI)",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:80\"\n  upstream \"b:1\" {}\n}\nhost \"h\" {}\n",
                "tcp \"a\": address 0.0.0.0:80 is also used by HTTP hosts",
            ),
            (
                "host \"h\" {\n  location \"/\" {\n    upstream \"b:1\" {}\n  }\n  tcp \"a\" {\n    listen = \"0.0.0.0:443\"\n    tls = \"passthrough\"\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": the host also has locations on 0.0.0.0:443 (TLS passthrough is chosen by name only, so the host is either HTTPS or TCP there)",
            ),
            (
                "host \"h\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:7\"\n    tls = \"passthrough\"\n    upstream \"b:1\" {}\n  }\n}\nhost \"i\" {\n  tcp \"a\" {\n    listen = \"0.0.0.0:7\"\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\" and tcp \"a\" in host \"i\" can't share address 0.0.0.0:7 (TLS passthrough and TLS terminated by proksi only share the HTTPS address)",
            ),
            (
                "host \"h\" {\n  location \"/\" {\n    upstream \"b:1\" {}\n  }\n  tcp \"a\" {\n    listen = \"0.0.0.0:443\"\n    upstream \"b:1\" {}\n  }\n}\n",
                "tcp \"a\" in host \"h\": the host also has locations on 0.0.0.0:443, so the block needs `alpn` to tell them apart",
            ),
            (
                "tcp \"a\" {\n  listen = \"0.0.0.0:7\"\n  upstream \"b:1\" {}\n}\ntcp \"b\" {\n  listen = \"0.0.0.0:7\"\n  upstream \"b:1\" {}\n}\n",
                "tcp \"a\" and tcp \"b\" can't share address 0.0.0.0:7 (only tcp blocks of different hosts with TLS can, chosen by SNI)",
            ),

        ] {
            assert_eq!(listeners(&config(hcl)).err().as_deref(), Some(expected), "{hcl}");
        }
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

    #[test]
    fn test_default_addresses() {
        let (https, http) = super::default_addresses(&Config::default()).unwrap();
        assert_eq!(https, "0.0.0.0:443".parse().unwrap());
        assert_eq!(http, "0.0.0.0:80".parse().unwrap());

        let mut config = Config::default();
        config.server.https_address = Some("localhost:8443".into());
        let (https, _) = super::default_addresses(&config).unwrap();
        assert_eq!(https.port(), 8443);

        config.server.http_address = Some("not an address".into());
        assert!(super::default_addresses(&config).is_err());
    }
}
