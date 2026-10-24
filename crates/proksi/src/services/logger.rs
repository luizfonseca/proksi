use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use crate::logger::ProxyLoggerReceiver;

/// Writes the access and error logs, on a thread of its own (started in `main`)
pub struct LoggerService {
    receiver: ProxyLoggerReceiver,
}

impl LoggerService {
    pub fn new(receiver: ProxyLoggerReceiver) -> Self {
        Self { receiver }
    }
}

#[async_trait]
impl Service for LoggerService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        tracing::info!("starting logger service");
        self.receiver.run().await;
    }

    /// Listens on nothing: inherited sockets no service declares are closed after an upgrade
    fn listen_addresses(&self) -> Option<Vec<String>> {
        Some(vec![])
    }

    fn name(&self) -> &'static str {
        "logging_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
