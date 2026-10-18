//! Finding the host serving a request, and the hosts needing certificates

use std::sync::Arc;

use super::{get_route_by_key, get_routes, host_container::HostContainer};

/// Host serving a request for `name`: the host with that name, or `*` if there's none.
/// `None` if that host doesn't `serves` the request (e.g. on the port it came in on).
pub fn find_host(
    name: &str,
    serves: impl Fn(&Arc<HostContainer>) -> bool,
) -> Option<Arc<HostContainer>> {
    find_host_in(name, get_route_by_key, serves)
}

/// Hosts served over HTTPS, with whether to fall back to a self-signed certificate
pub fn tls_hosts() -> Vec<(String, bool)> {
    tls_hosts_in(get_routes().iter())
}

fn find_host_in<T>(
    name: &str,
    get: impl Fn(&str) -> Option<T>,
    serves: impl Fn(&T) -> bool,
) -> Option<T> {
    // Names are lowercase already, as clients send them
    let lowercase;
    let name = if name.chars().any(char::is_uppercase) {
        lowercase = name.to_lowercase();
        &lowercase
    } else {
        name
    };
    get(name)
        .or_else(|| get("*"))
        .filter(|host| serves(host))
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

        assert!(Arc::ptr_eq(
            &find_host_in("A.Test", get, |_| true).unwrap(),
            &named
        ));
    }

    #[test]
    fn test_unknown_host_falls_back_to_catch_all() {
        let catch_all = container(false, false);
        let hosts = HashMap::from([("*".to_string(), catch_all.clone())]);
        let get = |name: &str| hosts.get(name).cloned();

        assert!(Arc::ptr_eq(
            &find_host_in("other.test", get, |_| true).unwrap(),
            &catch_all
        ));
    }

    #[test]
    fn test_unknown_host_without_catch_all() {
        let hosts: HashMap<String, Arc<HostContainer>> = HashMap::new();
        assert!(find_host_in(
            "other.test",
            |name: &str| hosts.get(name).cloned(),
            |_| true
        )
        .is_none());
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

    #[test]
    fn test_catch_all_only_serves_names_without_a_host() {
        let hosts = HashMap::from([
            ("a.test".to_string(), container(true, false)),
            ("*".to_string(), container(false, false)),
        ]);
        let get = |name: &str| hosts.get(name).cloned();
        let plain_http = |host: &Arc<HostContainer>| !host.tls_enabled;

        // e.g. a request on the HTTP port for a host with TLS: not served (it's redirected),
        // even though `*` would serve it
        assert!(find_host_in("a.test", get, plain_http).is_none());
        assert!(find_host_in("b.test", get, plain_http).is_some());
    }
}
