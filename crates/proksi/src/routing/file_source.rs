use crate::config::Config;

use super::{RouteSnapshot, RouteSourceKind};

/// Routes defined in the configuration file
pub fn file_snapshot(config: &Config) -> RouteSnapshot {
    RouteSnapshot::new(RouteSourceKind::File, config.routes.clone())
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
        assert_eq!(snapshot.routes, config.routes);
        assert_eq!(snapshot.revision, None);
    }
}
