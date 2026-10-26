//! The status of `Ingress`es (ADR 0005): the addresses they're served on, from proksi's own
//! `Service` (`publish_service`)

use k8s_openapi::api::{
    core::v1::Service,
    networking::v1::{Ingress, IngressLoadBalancerIngress},
};

/// The addresses of proksi's `Service`: its load balancer's, its external IPs, or (for services
/// without a load balancer) its cluster IP. None while a load balancer is being provisioned.
pub fn addresses(service: &Service) -> Vec<IngressLoadBalancerIngress> {
    let balancer = service
        .status
        .as_ref()
        .and_then(|status| status.load_balancer.as_ref())
        .and_then(|balancer| balancer.ingress.as_ref())
        .map(|ingress| {
            ingress
                .iter()
                .map(|address| IngressLoadBalancerIngress {
                    ip: address.ip.clone(),
                    hostname: address.hostname.clone(),
                    ..IngressLoadBalancerIngress::default()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !balancer.is_empty() {
        return balancer;
    }

    let Some(spec) = &service.spec else {
        return vec![];
    };
    let ip = |ip: &String| IngressLoadBalancerIngress {
        ip: Some(ip.clone()),
        ..IngressLoadBalancerIngress::default()
    };
    if let Some(external) = spec.external_ips.as_ref().filter(|ips| !ips.is_empty()) {
        return external.iter().map(ip).collect();
    }
    if spec.type_.as_deref() == Some("LoadBalancer") {
        return vec![];
    }
    spec.cluster_ip
        .iter()
        .filter(|cluster_ip| !cluster_ip.is_empty() && *cluster_ip != "None")
        .map(ip)
        .collect()
}

/// `namespace/name` of the `Ingress`es whose status doesn't have `addresses` yet
pub fn outdated(
    ingresses: &[&Ingress],
    addresses: &[IngressLoadBalancerIngress],
) -> Vec<(String, String)> {
    if addresses.is_empty() {
        return vec![];
    }
    ingresses
        .iter()
        .filter(|ingress| {
            let current = ingress
                .status
                .as_ref()
                .and_then(|status| status.load_balancer.as_ref())
                .and_then(|balancer| balancer.ingress.as_deref())
                .unwrap_or_default();
            current != addresses
        })
        .map(|ingress| {
            (
                ingress.metadata.namespace.clone().unwrap_or_default(),
                ingress.metadata.name.clone().unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::{core::v1::Service, networking::v1::Ingress};
    use serde_json::{json, Value};

    use super::{addresses, outdated};

    fn service(value: Value) -> Service {
        serde_json::from_value(value).unwrap()
    }

    fn published(service: &Service) -> Vec<(Option<String>, Option<String>)> {
        addresses(service)
            .into_iter()
            .map(|address| (address.ip, address.hostname))
            .collect()
    }

    #[test]
    fn test_addresses() {
        let balanced = service(json!({
            "spec": { "type": "LoadBalancer", "clusterIP": "10.96.0.5" },
            "status": { "loadBalancer": { "ingress": [{ "ip": "203.0.113.5" }, { "hostname": "lb.example.com" }] } },
        }));
        assert_eq!(
            published(&balanced),
            vec![
                (Some("203.0.113.5".to_string()), None),
                (None, Some("lb.example.com".to_string()))
            ]
        );

        // A load balancer being provisioned: nothing yet
        let pending =
            service(json!({ "spec": { "type": "LoadBalancer", "clusterIP": "10.96.0.5" } }));
        assert_eq!(published(&pending), vec![]);

        let external = service(json!({
            "spec": { "type": "NodePort", "clusterIP": "10.96.0.5", "externalIPs": ["198.51.100.1"] },
        }));
        assert_eq!(
            published(&external),
            vec![(Some("198.51.100.1".to_string()), None)]
        );

        let internal =
            service(json!({ "spec": { "type": "ClusterIP", "clusterIP": "10.96.0.5" } }));
        assert_eq!(
            published(&internal),
            vec![(Some("10.96.0.5".to_string()), None)]
        );
    }

    #[test]
    fn test_outdated() {
        let ingress = |name: &str, ips: &[&str]| -> Ingress {
            serde_json::from_value(json!({
                "metadata": { "name": name, "namespace": "default" },
                "status": { "loadBalancer": { "ingress": ips.iter().map(|ip| json!({ "ip": ip })).collect::<Vec<_>>() } },
            }))
            .unwrap()
        };
        let current = ingress("current", &["203.0.113.5"]);
        let old = ingress("old", &["203.0.113.9"]);
        let new = ingress("new", &[]);
        let wanted = addresses(&service(json!({
            "status": { "loadBalancer": { "ingress": [{ "ip": "203.0.113.5" }] } },
        })));

        assert_eq!(
            outdated(&[&current, &old, &new], &wanted),
            vec![
                ("default".to_string(), "old".to_string()),
                ("default".to_string(), "new".to_string())
            ]
        );
        // Nothing to publish (a load balancer being provisioned): nothing changes
        assert_eq!(outdated(&[&old], &[]), vec![]);
    }
}
