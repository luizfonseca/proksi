//! Finding the host serving a request, and the hosts needing certificates

use std::sync::Arc;

use super::{get_route_by_key, get_routes, host_container::HostContainer};

/// Host serving requests for `name`: the host with that name, or `*` (any other name)
pub fn find_host(name: &str) -> Option<Arc<HostContainer>> {
    find_host_in(name, get_route_by_key)
}

/// Hosts served over HTTPS, with whether to fall back to a self-signed certificate
pub fn tls_hosts() -> Vec<(String, bool)> {
    tls_hosts_in(get_routes().iter())
}

fn find_host_in<T>(name: &str, get: impl Fn(&str) -> Option<T>) -> Option<T> {
    get(&name.to_lowercase()).or_else(|| get("*"))
}

fn tls_hosts_in<'a>(
    hosts: impl Iterator<Item = (&'a String, &'a Arc<HostContainer>)>,
) -> Vec<(String, bool)> {
    hosts
        .filter(|(_, host)| host.tls_enabled)
        .map(|(name, host)| (name.clone(), host.self_signed_certificate))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use crate::{config::host::HostTls, stores::host_container::HostContainer};

    use super::{find_host_in, tls_hosts_in};

    fn container(enabled: bool, self_signed_on_failure: bool) -> Arc<HostContainer> {
        Arc::new(HostContainer::new(
            vec![],
            &HostTls {
                enabled,
                certificate: None,
                self_signed_on_failure,
            },
        ))
    }

    #[test]
    fn test_named_host_wins_over_catch_all() {
        let named = container(true, false);
        let hosts = HashMap::from([
            ("a.test".to_string(), named.clone()),
            ("*".to_string(), container(false, false)),
        ]);
        let get = |name: &str| hosts.get(name).cloned();

        assert!(Arc::ptr_eq(&find_host_in("A.Test", get).unwrap(), &named));
    }

    #[test]
    fn test_unknown_host_falls_back_to_catch_all() {
        let catch_all = container(false, false);
        let hosts = HashMap::from([("*".to_string(), catch_all.clone())]);
        let get = |name: &str| hosts.get(name).cloned();

        assert!(Arc::ptr_eq(
            &find_host_in("other.test", get).unwrap(),
            &catch_all
        ));
    }

    #[test]
    fn test_unknown_host_without_catch_all() {
        let hosts: HashMap<String, Arc<HostContainer>> = HashMap::new();
        assert!(find_host_in("other.test", |name: &str| hosts.get(name).cloned()).is_none());
    }

    #[test]
    fn test_only_hosts_with_tls_need_certificates() {
        let hosts = [
            ("a.test".to_string(), container(true, true)),
            ("plain.test".to_string(), container(false, false)),
            ("*".to_string(), container(false, false)),
        ];

        let mut names = tls_hosts_in(hosts.iter().map(|(name, host)| (name, host)));
        names.sort();

        assert_eq!(names, vec![("a.test".to_string(), true)]);
    }
}
