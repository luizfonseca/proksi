use crate::config::Config;

use super::{RouteSnapshot, RouteSourceKind};

/// Hosts defined in the configuration file (`host` blocks and `routes`)
pub fn file_snapshot(config: &Config) -> RouteSnapshot {
    RouteSnapshot::new(RouteSourceKind::File, config.hosts())
}

#[cfg(test)]
mod tests {
    use crate::{
        config::Config,
        routing::{test_utils::route, RouteSourceKind},
    };

    use super::file_snapshot;

    #[test]
    fn test_file_snapshot() {
        let mut config = Config::default();
        config.routes = vec![route("a.test", "10.0.0.1"), route("b.test", "10.0.0.2")];

        let snapshot = file_snapshot(&config);

        assert_eq!(snapshot.source, RouteSourceKind::File);
        assert_eq!(snapshot.hosts, config.hosts());
        assert_eq!(snapshot.hosts.len(), 2);
        assert_eq!(snapshot.revision, None);
    }
}
