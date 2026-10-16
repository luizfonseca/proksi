use serde::{Deserialize, Serialize};

use crate::config::RouteSslPath;

/// `tls { ... }` in a host
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsBlock {
    /// Certificate files, used instead of Let's Encrypt
    pub certificate: Option<RouteSslPath>,
    /// Use a self-signed certificate if no other certificate can be obtained
    #[serde(default)]
    pub self_signed_on_failure: bool,
}
