//! Hosts and their locations (ADR 0003): the model every route source is converted into

use serde::{Deserialize, Serialize};

use super::RouteSslPath;

mod from_route;
mod location;
mod upstream_address;

pub use location::Location;
pub use upstream_address::parse_upstream_address;

/// A host: its name, TLS settings and the locations serving its requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    #[serde(default)]
    pub tls: HostTls,
    #[serde(default)]
    pub locations: Vec<Location>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HostTls {
    /// Certificate files, used instead of Let's Encrypt
    pub certificate: Option<RouteSslPath>,
    /// Use a self-signed certificate if no other certificate can be obtained
    #[serde(default)]
    pub self_signed_on_failure: bool,
}
