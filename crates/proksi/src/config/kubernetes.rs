//! `kubernetes { ... }`: the ingress controller (ADR 0005)

use serde::{Deserialize, Serialize};

/// `kubernetes { ... }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubernetesConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The `IngressClass` proksi serves (`spec.ingressClassName` of its `Ingress`es)
    #[serde(default = "default_ingress_class")]
    pub ingress_class: String,
    /// Namespaces watched (empty: all of them)
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// `namespace/name` of proksi's `Service`, whose address is written to each `Ingress`'
    /// status
    #[serde(default)]
    pub publish_service: Option<String>,
}

fn default_ingress_class() -> String {
    "proksi".to_string()
}

impl Default for KubernetesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ingress_class: default_ingress_class(),
            namespaces: vec![],
            publish_service: None,
        }
    }
}
