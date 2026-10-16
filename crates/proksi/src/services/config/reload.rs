//! Decides what to do when the configuration file changes

use std::path::{Path, PathBuf};

use crate::{
    config::{validate::check_config, Config},
    routing::{file_source::file_snapshot, RouteSnapshot},
};

/// What to do with a new version of the configuration file
#[derive(Debug)]
pub enum ReloadAction {
    /// The new configuration is invalid: keep the current one
    Reject(String),
    /// Only routes changed: apply them without restarting
    ApplyRoutes(RouteSnapshot),
    /// Other settings changed: restart with the new configuration
    Restart,
}

pub fn reload_action(current: &Config, new: Result<Config, String>) -> ReloadAction {
    let new = match new {
        Ok(new) => new,
        Err(err) => return ReloadAction::Reject(err),
    };
    if let Err(err) = check_config(&new) {
        return ReloadAction::Reject(err.to_string());
    }

    if settings(current) == settings(&new) {
        ReloadAction::ApplyRoutes(file_snapshot(&new))
    } else {
        ReloadAction::Restart
    }
}

/// Everything in the configuration except the routes and hosts
fn settings(config: &Config) -> Option<serde_json::Value> {
    let mut value = serde_json::to_value(config).ok()?;
    if let Some(settings) = value.as_object_mut() {
        settings.remove("routes");
        settings.remove("host");
    }
    Some(value)
}

/// Configuration files to watch for a configuration path (a file or a directory)
pub fn config_files(config_path: &Path) -> Vec<PathBuf> {
    if config_path.is_file() {
        return vec![config_path.to_path_buf()];
    }

    CONFIG_FILE_NAMES
        .iter()
        .map(|name| config_path.join(name))
        .collect()
}

/// Whether a changed file can be a configuration file
pub fn is_config_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| CONFIG_EXTENSIONS.contains(&ext))
}

const CONFIG_FILE_NAMES: [&str; 3] = ["proksi.hcl", "proksi.yml", "proksi.yaml"];
const CONFIG_EXTENSIONS: [&str; 3] = ["hcl", "yml", "yaml"];

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::{
        config::{Config, LogLevel},
        routing::{test_utils::route, RouteSourceKind},
    };

    use super::{config_files, is_config_file, reload_action, ReloadAction};

    fn config_with_routes(hosts: &[&str]) -> Config {
        let mut config = Config::default();
        config.routes = hosts.iter().map(|h| route(h, "10.0.0.1")).collect();
        config
    }

    #[test]
    fn test_routes_only_change_applies_routes() {
        let current = config_with_routes(&["a.test"]);
        let new = config_with_routes(&["a.test", "b.test"]);

        let ReloadAction::ApplyRoutes(snapshot) = reload_action(&current, Ok(new)) else {
            panic!("expected routes to be applied");
        };

        assert_eq!(snapshot.source, RouteSourceKind::File);
        assert_eq!(snapshot.routes.len(), 2);
    }

    #[test]
    fn test_settings_change_restarts() {
        let current = config_with_routes(&["a.test"]);
        let mut new = config_with_routes(&["a.test"]);
        new.logging.level = LogLevel::Debug;

        assert!(matches!(
            reload_action(&current, Ok(new)),
            ReloadAction::Restart
        ));
    }

    #[test]
    fn test_unreadable_config_is_rejected() {
        let current = config_with_routes(&["a.test"]);

        let action = reload_action(&current, Err("invalid hcl".to_string()));

        assert!(matches!(action, ReloadAction::Reject(err) if err == "invalid hcl"));
    }

    #[test]
    fn test_invalid_config_is_rejected() {
        let current = config_with_routes(&["a.test"]);
        let mut new = config_with_routes(&["a.test"]);
        new.routes[0].upstreams[0].port = 0;

        let action = reload_action(&current, Ok(new));

        assert!(
            matches!(&action, ReloadAction::Reject(err) if err.contains("port must be greater than 0")),
            "{action:?}"
        );
    }

    #[test]
    fn test_config_files_for_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.hcl");
        std::fs::write(&file, "").unwrap();

        assert_eq!(config_files(&file), vec![file]);
    }

    #[test]
    fn test_config_files_for_a_directory() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            config_files(dir.path()),
            vec![
                dir.path().join("proksi.hcl"),
                dir.path().join("proksi.yml"),
                dir.path().join("proksi.yaml"),
            ]
        );
    }

    #[test]
    fn test_is_config_file() {
        assert!(is_config_file(Path::new("/etc/proksi/proksi.hcl")));
        assert!(is_config_file(Path::new("proksi.yml")));
        assert!(is_config_file(&PathBuf::from("routes.yaml")));
        assert!(!is_config_file(Path::new("proksi.hcl.swp")));
        assert!(!is_config_file(Path::new("README.md")));
    }

    #[test]
    fn test_host_block_change_applies_routes() {
        let current = Config::default();
        let mut new = Config::default();
        new.host.insert(
            "a.test".to_string(),
            crate::config::blocks::HostBlock::default(),
        );

        assert!(matches!(
            reload_action(&current, Ok(new)),
            ReloadAction::ApplyRoutes(_)
        ));
    }
}
