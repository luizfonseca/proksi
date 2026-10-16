use crate::{config::host::Host, stores::host_container::HostContainer};

use super::location::build_location_container;

/// Builds what the proxy needs to serve a host: a container per location, chosen by path.
/// Returns `None` if a location can't be built (the previous version keeps being served).
pub fn build_host_container(host: &Host) -> Option<HostContainer> {
    let locations = host
        .locations
        .iter()
        .map(|location| {
            let container = build_location_container(&host.name, location)?;
            Some((location.patterns(), container))
        })
        .collect::<Option<Vec<_>>>()?;

    let mut container = HostContainer::new(locations, &host.tls);
    // Validated as `ip:port` when the host is merged
    container.listen = host
        .listen
        .iter()
        .filter_map(|address| address.parse().ok())
        .collect();
    Some(container)
}

#[cfg(test)]
mod tests {
    use crate::routing::test_utils::host;

    use super::build_host_container;

    #[test]
    fn test_build_host_container() {
        let mut host = host("a.test", "10.0.0.1");
        let mut session = host.locations[0].clone();
        session.path = "/v1/session".to_string();
        session.exact = false;
        session.upstreams[0].ip = "10.0.0.2".into();
        host.locations.push(session);
        host.tls.self_signed_on_failure = true;

        let container = build_host_container(&host).unwrap();

        assert_eq!(container.locations.len(), 2);
        assert!(container.self_signed_certificate);
        let served = |path: &str| {
            container.location_for(path).unwrap().upstreams[0]
                .ip
                .to_string()
        };
        assert_eq!(served("/v1/session/x"), "10.0.0.2");
        assert_eq!(served("/other"), "10.0.0.1");
    }
}
