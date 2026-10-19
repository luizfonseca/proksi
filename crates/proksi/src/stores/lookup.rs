//! Finding the host serving a request, and the hosts needing certificates

use std::sync::Arc;

use crate::config::host::Challenge;

use super::{get_route_by_key, get_routes, host_container::HostContainer};

/// Host serving a request for `name`: the host with that name, the wildcard host of its parent
/// (`*.example.com` for `a.example.com`), or `*` if there's neither (ADR 0009).
/// `None` if that host doesn't `serves` the request (e.g. on the port it came in on).
pub fn find_host(
    name: &str,
    serves: impl Fn(&Arc<HostContainer>) -> bool,
) -> Option<Arc<HostContainer>> {
    find_host_in(name, get_route_by_key, serves)
}

/// The host with that name, or the wildcard host of its parent, without the catch-all `*`
pub fn named_host(name: &str) -> Option<Arc<HostContainer>> {
    named_host_in(name, get_route_by_key)
}

/// The name the certificate for `name` (a TLS server name) is stored under: the name of the
/// host serving it, so a wildcard host's for names it serves
pub fn certificate_name(name: &str) -> std::borrow::Cow<'_, str> {
    certificate_name_in(name, get_route_by_key)
}

/// A host served over HTTPS, needing a certificate
#[derive(Debug, Clone, PartialEq)]
pub struct TlsHost {
    pub name: String,
    /// Fall back to a self-signed certificate
    pub self_signed_on_failure: bool,
    pub challenge: Challenge,
}

/// Hosts served over HTTPS
pub fn tls_hosts() -> Vec<TlsHost> {
    tls_hosts_in(get_routes().iter())
}

fn find_host_in<T>(
    name: &str,
    get: impl Fn(&str) -> Option<T>,
    serves: impl Fn(&T) -> bool,
) -> Option<T> {
    named_host_in(name, &get)
        .or_else(|| get("*"))
        .filter(|host| serves(host))
}

fn named_host_in<T>(name: &str, get: impl Fn(&str) -> Option<T>) -> Option<T> {
    // Names are lowercase already, as clients send them
    let lowercase;
    let name = if name.chars().any(char::is_uppercase) {
        lowercase = name.to_lowercase();
        &lowercase
    } else {
        name
    };
    get(name).or_else(|| with_wildcard_parent(name, |wildcard| get(wildcard)))
}

fn certificate_name_in<T>(
    name: &str,
    get: impl Fn(&str) -> Option<T>,
) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    let name: Cow<'_, str> = if name.chars().any(char::is_uppercase) {
        Cow::Owned(name.to_lowercase())
    } else {
        Cow::Borrowed(name)
    };
    if get(&name).is_some() {
        return name;
    }
    with_wildcard_parent(&name, |wildcard| {
        get(wildcard).map(|_| wildcard.to_string())
    })
    .map_or(name, Cow::Owned)
}

/// Calls `f` with `*.<parent>` for `name` (`*.example.com` for `a.example.com`), built on the
/// stack. `None` for names without a parent of two labels or more.
fn with_wildcard_parent<T>(name: &str, f: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    // The longest DNS name is 253 characters
    const MAX: usize = 255;
    let (_, parent) = name.split_once('.')?;
    if !parent.contains('.') || parent.len() + 2 > MAX {
        return None;
    }
    let mut buffer = [0u8; MAX];
    let wildcard = buffer.get_mut(..parent.len() + 2)?;
    let (prefix, rest) = wildcard.split_at_mut(2);
    prefix.copy_from_slice(b"*.");
    rest.copy_from_slice(parent.as_bytes());
    f(std::str::from_utf8(wildcard).ok()?)
}

fn tls_hosts_in<'a>(
    hosts: impl Iterator<Item = (&'a String, &'a Arc<HostContainer>)>,
) -> Vec<TlsHost> {
    hosts
        .filter(|(_, host)| host.tls_enabled)
        .map(|(name, host)| TlsHost {
            name: name.clone(),
            self_signed_on_failure: host.self_signed_certificate,
            challenge: Challenge::of_host(host.challenge, name),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use crate::{config::host::HostTls, stores::host_container::HostContainer};

    use crate::config::host::Challenge;

    use super::{certificate_name_in, find_host_in, named_host_in, tls_hosts_in, TlsHost};

    fn container(enabled: bool, self_signed_on_failure: bool) -> Arc<HostContainer> {
        Arc::new(HostContainer::new(
            vec![],
            &HostTls {
                enabled,
                certificate: None,
                self_signed_on_failure,
                challenge: None,
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
    fn test_wildcard_host_serves_one_more_label() {
        let wildcard = container(true, false);
        let catch_all = container(false, false);
        let hosts = HashMap::from([
            ("*.example.test".to_string(), wildcard.clone()),
            ("*".to_string(), catch_all.clone()),
        ]);
        let get = |name: &str| hosts.get(name).cloned();
        let found = |name: &str| find_host_in(name, get, |_| true).unwrap();

        assert!(Arc::ptr_eq(&found("a.example.test"), &wildcard));
        assert!(Arc::ptr_eq(&found("B.Example.Test"), &wildcard));
        // Not the parent itself, nor two more labels
        assert!(Arc::ptr_eq(&found("example.test"), &catch_all));
        assert!(Arc::ptr_eq(&found("a.b.example.test"), &catch_all));
    }

    #[test]
    fn test_exact_host_wins_over_wildcard_host() {
        let exact = container(true, false);
        let hosts = HashMap::from([
            ("a.example.test".to_string(), exact.clone()),
            ("*.example.test".to_string(), container(true, false)),
        ]);
        let get = |name: &str| hosts.get(name).cloned();

        assert!(Arc::ptr_eq(
            &find_host_in("a.example.test", get, |_| true).unwrap(),
            &exact
        ));
    }

    #[test]
    fn test_named_host_ignores_catch_all() {
        let wildcard = container(true, false);
        let hosts = HashMap::from([
            ("*.example.test".to_string(), wildcard.clone()),
            ("*".to_string(), container(false, false)),
        ]);
        let get = |name: &str| hosts.get(name).cloned();

        assert!(Arc::ptr_eq(
            &named_host_in("a.example.test", get).unwrap(),
            &wildcard
        ));
        assert!(named_host_in("other.test", get).is_none());
    }

    #[test]
    fn test_certificate_of_the_host_serving_a_name() {
        let hosts = HashMap::from([
            ("a.example.test".to_string(), container(true, false)),
            ("*.example.test".to_string(), container(true, false)),
        ]);
        let get = |name: &str| hosts.get(name).cloned();

        assert_eq!(certificate_name_in("a.example.test", get), "a.example.test");
        assert_eq!(certificate_name_in("b.example.test", get), "*.example.test");
        assert_eq!(certificate_name_in("B.Example.Test", get), "*.example.test");
        // Names no host serves keep their own (nothing is found for them)
        assert_eq!(certificate_name_in("other.test", get), "other.test");
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
            ("*.a.test".to_string(), container(true, false)),
            ("plain.test".to_string(), container(false, false)),
            ("*".to_string(), container(false, false)),
        ];

        let mut hosts = tls_hosts_in(hosts.iter().map(|(name, host)| (name, host)));
        hosts.sort_by(|a, b| a.name.cmp(&b.name));

        // Wildcard hosts are checked through DNS (ADR 0009)
        assert_eq!(
            hosts,
            vec![
                TlsHost {
                    name: "*.a.test".to_string(),
                    self_signed_on_failure: false,
                    challenge: Challenge::Dns01,
                },
                TlsHost {
                    name: "a.test".to_string(),
                    self_signed_on_failure: true,
                    challenge: Challenge::Http01,
                },
            ]
        );
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
