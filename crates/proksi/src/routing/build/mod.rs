//! Building what the proxy needs to serve hosts (load balancers, matchers, certificates)

mod certificate;
mod host;
mod load_balancer;
mod location;

pub use certificate::load_host_certificate;
pub use host::build_host_container;
pub use load_balancer::{load_balancer, upstream_index};
