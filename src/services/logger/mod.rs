use std::io::{self, Write};

use async_trait::async_trait;
use bytes::Bytes;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing_subscriber::fmt::MakeWriter;

/// A `io::Write` implementation that sends logs to a background service
#[derive(Debug, Clone)]
pub struct StdoutWriter(UnboundedSender<Bytes>);

impl io::Write for StdoutWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(()) = self.0.send(Bytes::copy_from_slice(buf)) {
            return Ok(buf.len());
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}

/// A naive implementation of a logger that delegate sending logs to a background channel
#[derive(Debug)]
pub struct ProxyLog {
    stdout: StdoutWriter,
}

impl ProxyLog {
    pub fn new(sender: &UnboundedSender<Bytes>) -> Self {
        ProxyLog {
            // level,
            stdout: StdoutWriter(sender.clone()),
        }
    }
}

/// impl from `tracing_subscriber::fmt::MakeWriter`
impl<'a> MakeWriter<'a> for ProxyLog {
    type Writer = StdoutWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.stdout.clone()
    }
}

/// A background service that receives logs from the main thread and writes them to stdout
/// TODO: implement log rotation/write to disk (or use an existing lightweight crate)
pub struct ProxyLoggerReceiver {
    receiver: UnboundedReceiver<Bytes>,
}

impl ProxyLoggerReceiver {
    pub fn new(receiver: UnboundedReceiver<Bytes>) -> Self {
        ProxyLoggerReceiver { receiver }
    }
}

#[async_trait]
impl Service for ProxyLoggerReceiver {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        loop {
            if let Some(buf) = self.receiver.recv().await {
                // TODO: flush/rotate logs to disk
                io::stdout().write_all(&buf).unwrap();
            }
        }
    }

    fn name(&self) -> &str {
        "ProxyLogger"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
