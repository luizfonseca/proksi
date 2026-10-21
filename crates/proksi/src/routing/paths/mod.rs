//! Choosing a host's location from the request path

mod matcher;
mod normalize;

pub use matcher::{compile_regex, compile_regexes, LocationMatcher, MAX_REGEXES};
pub(crate) use normalize::normalize_path;
