//! Choosing a host's location from the request path

mod matcher;
mod normalize;

pub use matcher::LocationMatcher;
pub(crate) use normalize::normalize_path;
