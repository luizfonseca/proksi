//! Settings that still work but will be removed in a future major version

use super::Config;

/// Warnings for the deprecated settings used in the configuration
pub fn deprecation_warnings(config: &Config) -> Vec<&'static str> {
    let mut warnings = vec![];
    if !config.routes.is_empty() {
        warnings.push(
            "`routes = [...]` is deprecated and will be removed in a future major version, use `host` blocks instead (https://docs.proksi.info/#/configuration/hcl/)",
        );
    }
    warnings
}

#[cfg(test)]
mod tests {
    use crate::{config::Config, routing::test_utils::route};

    use super::deprecation_warnings;

    #[test]
    fn test_no_deprecated_settings() {
        assert!(deprecation_warnings(&Config::default()).is_empty());
    }

    #[test]
    fn test_routes_are_deprecated() {
        let mut config = Config::default();
        config.routes = vec![route("a.test", "10.0.0.1")];

        let warnings = deprecation_warnings(&config);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("`routes = [...]` is deprecated"));
    }
}
