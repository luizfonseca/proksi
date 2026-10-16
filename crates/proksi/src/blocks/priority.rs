//! `priority = <0-100>`, which any block can set

use crate::config::RoutePlugin;

use super::Kind;

/// The priority of a block: the one it sets, otherwise the default of its kind
pub(super) fn priority(kind: Kind, set: Option<u64>) -> Result<u8, String> {
    match set {
        None => Ok(kind.default_priority()),
        Some(priority) => u8::try_from(priority)
            .ok()
            .filter(|priority| *priority <= 100)
            .ok_or_else(|| format!("{}: priority must be between 0 and 100", kind.name())),
    }
}

/// The priority a plugin sets in its settings (`priority = 10`)
pub(super) fn plugin_priority(kind: Kind, plugin: &RoutePlugin) -> Result<u8, String> {
    let set = plugin
        .config
        .as_ref()
        .and_then(|config| config.get("priority"))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| format!("{}: priority must be between 0 and 100", kind.name()))
        })
        .transpose()?;
    priority(kind, set)
}

/// A setting a plugin requires, as a string
pub(super) fn required<'a>(
    kind: Kind,
    plugin: &'a RoutePlugin,
    key: &str,
) -> Result<&'a str, String> {
    plugin
        .config
        .as_ref()
        .and_then(|config| config.get(key))
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{}: `{key}` is required", kind.name()))
}
