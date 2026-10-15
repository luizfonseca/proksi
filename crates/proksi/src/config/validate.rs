use anyhow::anyhow;

use super::Config;
use crate::routing::validate::validate_route;

/// given a Config struct, validate the values to ensure
/// That we program won't panic when we try to use them
pub fn check_config(config: &Config) -> Result<(), anyhow::Error> {
    // Validate if worker threads is greater than 0
    if config.worker_threads.is_some_and(|v| v == 0) {
        return Err(anyhow!("Worker threads must be greater than 0"));
    }

    if config.store.sync_interval_secs == 0 {
        return Err(anyhow!("store.sync_interval_secs must be greater than 0"));
    }

    // Validate that the docker interval secs is greater than 0
    if config.docker.interval_secs.is_some_and(|v| v == 0) {
        return Err(anyhow!("docker.interval_secs must be greater than 0"));
    }

    // validate that the lets encrypt email does not contain @example or is empty, but only if lets_encrypt is enabled
    if config.lets_encrypt.enabled.unwrap_or(false)
        && (config.lets_encrypt.email.contains("@example") || config.lets_encrypt.email.is_empty())
    {
        return Err(anyhow!(
            "lets_encrypt.email cannot be empty or an email from @example.com (the default value)"
        ));
    }

    // Validate that the lets_encrypt pathbuf is not an empty string
    if config.paths.lets_encrypt.as_os_str() == "" {
        return Err(anyhow!("paths.lets_encrypt cannot be empty"));
    }

    // Validate the routes
    for (route_index, route) in config.routes.iter().enumerate() {
        validate_route(route).map_err(|err| anyhow!("routes[{route_index}]: {err}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::routing::test_utils::route;

    use super::check_config;

    #[test]
    fn test_check_config_store_sync_interval() {
        let mut config = Config::default();
        config.store.sync_interval_secs = 0;

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "store.sync_interval_secs must be greater than 0"
        );
    }

    #[test]
    fn test_check_config_validates_routes() {
        let mut config = Config::default();
        let mut invalid = route("example.com", "10.0.0.1");
        invalid.upstreams[0].port = 0;
        config.routes = vec![route("valid.com", "10.0.0.1"), invalid];

        let err = check_config(&config).unwrap_err();

        assert_eq!(
            err.to_string(),
            "routes[1]: upstreams[0].port must be greater than 0"
        );
    }
}
