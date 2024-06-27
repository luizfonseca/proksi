use std::{os::unix::process::CommandExt, path::PathBuf, sync::Arc};

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
        if path.exists() {
            watcher
                .watch(path, notify::RecursiveMode::Recursive)
                .unwrap();
        }
    }
}

pub struct FileWatcherServiceHandler {}
impl EventHandler for FileWatcherServiceHandler {
    /// Handles configuration file changes and restarts the server
    fn handle_event(&mut self, notif: notify::Result<notify::Event>) {
        let Ok(_n) = notif else {
            tracing::error!("error handling event: {:?}", notif);
            return;
        };

        let Ok(cmd) = std::env::current_exe() else {
            return;
        };

        let current_pid = std::process::id();

        // remove the command path, take the rest
        let current_args = std::env::args().skip(1);

        // kill the process
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(current_pid.try_into().unwrap()),
            nix::sys::signal::Signal::SIGQUIT,
        )
        .unwrap();

        tracing::warn!("restarting server");

        // restart the process
        std::process::Command::new(cmd).args(current_args).exec();
    }
}

#[async_trait]
impl Service for FileWatcherService {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        if self.config.auto_reload.enabled.is_some_and(|v| !v) {
            // Nothing to do, auto reload is  disabled
            return;
        }

        // watch main config file:
        let path_buf = PathBuf::from(self.config.config_path.to_string());
        let config_file_yaml = path_buf.join("proksi.yaml");
        let config_file_hcl = path_buf.join("proksi.hcl");

        tracing::info!("starting config watcher service");

        let mut watcher = notify::poll::PollWatcher::new(
            FileWatcherServiceHandler {},
            notify::Config::default().with_manual_polling(),
        )
        .unwrap();

        Self::watch_file_or_dir(&mut watcher, &config_file_hcl);
        // Self::watch_file_or_dir(&mut watcher, &PathBuf::from("./dist/*.hcl"));
        Self::watch_file_or_dir(&mut watcher, &config_file_yaml);

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

    fn name(&self) -> &str {
        "config_watcher_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
