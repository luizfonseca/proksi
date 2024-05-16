use async_trait::async_trait;
use pingora::{server::ShutdownWatch, services::background::BackgroundService};

use crate::config::LogLevel;

pub struct ProxyLogger(pub LogLevel);

#[async_trait]
impl BackgroundService for ProxyLogger {
    async fn start(&self, _shutdown: ShutdownWatch) {
        tracing_subscriber::fmt()
            .with_max_level(&self.0)
            .with_writer(std::io::stdout)
            .init()
    }
}
