use std::{
    os::unix::process::CommandExt,
    path::{self, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use notify::{EventHandler, Watcher};
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::config::Config;

pub struct FileWatcherService {
    config: Arc<Config>,
}

impl FileWatcherService {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
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
            watcher
                .watch(&absolute_path, notify::RecursiveMode::Recursive)
                .unwrap();
        } else {
            tracing::debug!("file or directory does not exist: {:?}", absolute_path);
        }
    }
}

pub struct FileWatcherServiceHandler {}
impl EventHandler for FileWatcherServiceHandler {
    /// Handles configuration file changes and restarts the server
    fn handle_event(&mut self, notif: notify::Result<notify::Event>) {
        let Ok(n) = notif else {
            tracing::error!("error handling auto_reload event: {:?}", notif);
            return;
        };

        // If no .hcl can be found, skip
        if !n
            .paths
            .iter()
            .any(|v| v.extension().is_some_and(|v| v == "hcl"))
        {
            tracing::info!("no .hcl file found, skipping {:?}", n.paths);
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
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(current_pid.try_into().unwrap()),
            nix::sys::signal::Signal::SIGQUIT,
        )
        .unwrap();
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
        if let Some(config_path) = &self.config.config_path {
            let path_buf = PathBuf::from(config_path.to_string());
            let config_file_yaml = path_buf.join("proksi.yaml");
            let config_file_hcl = path_buf.join("proksi.hcl");

            tracing::info!("starting config watcher service");

            let mut watcher = notify::poll::PollWatcher::new(
                FileWatcherServiceHandler {},
                notify::Config::default().with_manual_polling(),
            )
            .unwrap();

            Self::watch_file_or_dir(&mut watcher, &config_file_hcl);
            Self::watch_file_or_dir(&mut watcher, &config_file_yaml);

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
        } else {
            // No config path provided, nothing to watch
            tracing::info!("No config path provided, config watcher service not started");
        }
    }

    fn name(&self) -> &'static str {
        "config_watcher_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
