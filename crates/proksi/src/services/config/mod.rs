use std::{
    os::unix::process::CommandExt,
    path::{self, Path},
    sync::Arc,
};

use async_trait::async_trait;
use clap::Parser;
use notify::{EventHandler, Watcher};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::{
    config::{load_from_path, Config},
    routing::SnapshotSender,
};

use reload::{config_files, is_config_file, reload_action, ReloadAction};

pub mod reload;

pub struct FileWatcherService {
    config: Arc<Config>,
    sender: SnapshotSender,
}

impl FileWatcherService {
    pub fn new(config: Arc<Config>, sender: SnapshotSender) -> Self {
        Self { config, sender }
    }

    /// Watchs a file or directory for changes
    /// If the file or directory does not exist, it will be ignored
    pub fn watch_file_or_dir(watcher: &mut notify::poll::PollWatcher, path: &std::path::Path) {
        // if the path is not absolute, make it absolute
        let Ok(absolute_path) = path::absolute(path) else {
            tracing::error!("could not get absolute path, auto_reload will not work");
            return;
        };

        tracing::info!("auto_reload path: {:?}", absolute_path);

        if absolute_path.exists() {
            if let Err(err) = watcher.watch(&absolute_path, notify::RecursiveMode::Recursive) {
                tracing::error!("failed to watch {absolute_path:?}: {err}");
            }
        } else {
            tracing::debug!("file or directory does not exist: {:?}", absolute_path);
        }
    }
}

pub struct FileWatcherServiceHandler {
    /// Configuration proksi is running with
    config: Arc<Config>,
    config_path: String,
    sender: SnapshotSender,
}

impl EventHandler for FileWatcherServiceHandler {
    /// Handles configuration file changes: applies route changes, or restarts the
    /// server when other settings changed
    fn handle_event(&mut self, notif: notify::Result<notify::Event>) {
        let Ok(n) = notif else {
            tracing::error!("error handling auto_reload event: {:?}", notif);
            return;
        };

        if !n.paths.iter().any(|path| is_config_file(path)) {
            tracing::info!("no configuration file changed, skipping {:?}", n.paths);
            return;
        }

        let cli = Config::try_parse().unwrap_or_default();
        let new_config = load_from_path(&self.config_path, &cli, false).map_err(|e| e.to_string());

        match reload_action(&self.config, new_config) {
            ReloadAction::Reject(err) => {
                tracing::error!(
                    "the configuration file changed but is invalid, keeping the current configuration: {err}"
                );
            }
            ReloadAction::ApplyRoutes(snapshot) => {
                if self.sender.try_send(snapshot).is_err() {
                    tracing::error!("failed to apply the routes from the configuration file");
                } else {
                    tracing::info!("applying route changes from the configuration file");
                }
            }
            ReloadAction::Restart => restart(),
        }
    }
}

/// Watcher for configuration files, polled manually.
/// Contents are compared too: modification times are only compared to the second, so
/// changes made within the same second would be missed otherwise.
fn config_watcher<H: EventHandler>(handler: H) -> notify::Result<notify::poll::PollWatcher> {
    notify::poll::PollWatcher::new(
        handler,
        notify::Config::default()
            .with_manual_polling()
            .with_compare_contents(true),
    )
}

/// Restarts proksi with the same arguments: replaced by a new worker without dropping
/// connections when supervised (Linux), started again otherwise
fn restart() {
    #[cfg(target_os = "linux")]
    if crate::supervisor::request_upgrade() {
        tracing::warn!("settings changed: upgrading to a new worker");
        return;
    }

    let Ok(cmd) = std::env::current_exe() else {
        return;
    };

    let current_pid = std::process::id();

    // remove the command path, take the rest
    let current_args = std::env::args().skip(1);

    // restart the process
    let _ = std::process::Command::new(cmd).args(current_args).exec();

    tracing::warn!("restarting Proksi server");

    // kill existing process
    let Ok(current_pid) = current_pid.try_into() else {
        tracing::error!("invalid process id: {current_pid}");
        return;
    };
    if let Err(err) = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(current_pid),
        nix::sys::signal::Signal::SIGQUIT,
    ) {
        tracing::error!("failed to stop the current process: {err}");
    }
}

#[async_trait]
impl Service for FileWatcherService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.config.auto_reload.enabled.is_some_and(|v| !v) {
            // Nothing to do, auto reload is  disabled
            return;
        }

        // watch main config file:
        let Some(config_path) = &self.config.config_path else {
            // No config path provided, nothing to watch
            tracing::info!("No config path provided, config watcher service not started");
            return;
        };
        tracing::info!("starting config watcher service");

        let handler = FileWatcherServiceHandler {
            config: self.config.clone(),
            config_path: config_path.to_string(),
            sender: self.sender.clone(),
        };
        let Ok(mut watcher) = config_watcher(handler) else {
            tracing::error!("could not create config watcher, auto_reload will not work");
            return;
        };

        for config_file in config_files(Path::new(config_path.as_ref())) {
            Self::watch_file_or_dir(&mut watcher, &config_file);
        }

        // Watch for paths in the config
        for watch_path in &self.config.auto_reload.paths {
            Self::watch_file_or_dir(&mut watcher, watch_path);
        }

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            self.config.auto_reload.interval_secs.unwrap_or(60),
        ));
        interval.tick().await;

        loop {
            interval.tick().await;
            if watcher.poll().is_ok() {
                tracing::debug!("config watcher service tick");
            }
        }
    }

    fn name(&self) -> &'static str {
        "config_watcher_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use notify::Watcher;

    use super::config_watcher;

    #[test]
    fn test_config_watcher_detects_changes_within_the_same_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proksi.hcl");
        std::fs::write(&path, "host = \"a.test\"").unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

        let (sender, receiver) = mpsc::channel();
        let mut watcher = config_watcher(move |event: notify::Result<notify::Event>| {
            sender.send(event.is_ok()).ok();
        })
        .unwrap();
        watcher
            .watch(&path, notify::RecursiveMode::Recursive)
            .unwrap();

        // Same size, and the same modification time (as when written within the same second)
        std::fs::write(&path, "host = \"b.test\"").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        watcher.poll().unwrap();

        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)), Ok(true));
    }
}
