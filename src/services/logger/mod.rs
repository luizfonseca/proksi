use std::io::{self, Write};

use async_trait::async_trait;

use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing_subscriber::fmt::MakeWriter;

/// A `io::Write` implementation that sends logs to a background service
#[derive(Debug, Clone)]
pub struct StdoutWriter<'a> {
    chan: &'a UnboundedSender<Vec<u8>>,
    skip_log: bool,
}

impl io::Write for StdoutWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.skip_log {
            self.chan.send(buf.to_vec()).ok();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.skip_log {
            return Ok(());
        }

        io::stdout().flush()
    }
}

/// A naive implementation of a logger that delegate sending logs to a background channel
#[derive(Debug)]
pub struct ProxyLog {
    enabled: bool,
    chan: UnboundedSender<Vec<u8>>,
    access_logs: bool,
    error_logs: bool,
}

impl ProxyLog {
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn new(
        sender: UnboundedSender<Vec<u8>>,
        log_enabled: bool,
        access_logs: bool,
        error_logs: bool,
    ) -> Self {
        ProxyLog {
            // level,
            enabled: log_enabled,
            access_logs,
            error_logs,
            chan: sender,
        }
    }
}

/// impl from `tracing_subscriber::fmt::MakeWriter`
impl<'a> MakeWriter<'a> for ProxyLog {
    type Writer = StdoutWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        StdoutWriter {
            skip_log: false,
            chan: &self.chan,
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        // if access_logs are disabled, we run a logic to skip them

        let mut skip_log = false;

        if !self.error_logs {
            skip_log = *meta.level() == tracing::Level::ERROR;
        }

        if !self.access_logs {
            let access_log_tag = meta.fields().field("access_log");

            if access_log_tag.is_some() {
                skip_log = true;
            }
        }

        StdoutWriter {
            skip_log: skip_log || !self.enabled,
            chan: &self.chan,
        }
    }
}

/// A background service that receives logs from the main thread and writes them to stdout
/// TODO: implement log rotation/write to disk (or use an existing lightweight crate)
pub struct ProxyLoggerReceiver {
    receiver: UnboundedReceiver<Vec<u8>>,
}

impl ProxyLoggerReceiver {
    pub fn new(receiver: UnboundedReceiver<Vec<u8>>) -> Self {
        ProxyLoggerReceiver { receiver }
    }
}

#[async_trait]
impl Service for ProxyLoggerReceiver {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        // TODO: find a way to share the stdout lock
        while let Some(buf) = self.receiver.recv().await {
            io::stdout().write_all(&buf).unwrap();
        }
    }

    fn name(&self) -> &str {
        "logging_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
