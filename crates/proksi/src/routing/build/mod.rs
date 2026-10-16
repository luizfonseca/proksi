//! Building what the proxy needs to serve hosts (load balancers, matchers, certificates)

mod certificate;
mod host;
mod location;

pub use certificate::load_host_certificate;
pub use host::build_host_container;
