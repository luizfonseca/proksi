//! Validation of routes and hosts, whatever their source

mod common;
mod host;
mod route;

pub use host::validate_host;
pub use route::validate_route;
