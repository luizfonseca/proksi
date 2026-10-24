use std::collections::BTreeMap;

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::{config::notify::NotifyBlock, notify};

/// Sends `notify` events as they're raised (see `notify::run`)
pub struct NotifyService;

impl NotifyService {
    /// Starts with the configuration's `notify` blocks
    pub fn new(blocks: BTreeMap<String, NotifyBlock>) -> Self {
        notify::configure(blocks);
        Self
    }
}

#[async_trait]
impl Service for NotifyService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        notify::run().await;
    }

    fn name(&self) -> &'static str {
        "notify"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
