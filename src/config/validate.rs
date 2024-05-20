use anyhow::anyhow;

use super::Config;

/// given a Config struct, validate the values to ensure
/// That we program won't panic when we try to use them
pub fn validate_config(config: &Config) -> Result<(), anyhow::Error> {
    // Validate if worker threads is greater than 0
    if config.worker_threads.unwrap() == 0 {
        return Err(anyhow!("Worker threads must be greater than 0"));
    }

    // Validate that the docker interval secs is greater than 0
    if config.docker.interval_secs.unwrap() == 0 {
        return Err(anyhow!("docker.interval_secs must be greater than 0"));
    }

    // validate that the lets encrypt email does not contain @example or is empty
    if config.lets_encrypt.email.contains("@example") || config.lets_encrypt.email.is_empty() {
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
        // Validate the route's upstreams
        for (upstream_index, upstream) in route.upstreams.iter().enumerate() {
            // Validate the upstream's address
            if upstream.ip.is_empty() {
                return Err(anyhow!(
                    "routes{}.upstreams{}.id cannot be empty",
                    route_index,
                    upstream_index
                ));
            }

            if upstream.port <= 0 {
                return Err(anyhow!(
                    "routes{}.upstreams{}.port must be greater than 0",
                    route_index,
                    upstream_index
                ));
            }
        }
    }

    Ok(())
}
