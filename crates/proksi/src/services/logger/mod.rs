use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};

use async_trait::async_trait;

use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use rotation::Rotation;
use tokio::sync::mpsc::{error::TrySendError, Receiver, Sender};
use tracing_subscriber::fmt::MakeWriter;

use crate::config::Config;

pub mod handshake_error_filter;
mod rotation;

/// Maximum number of log lines waiting to be written.
/// When the queue is full (e.g. the output can't keep up), new lines are dropped
/// so that memory usage stays bounded.
pub const LOG_QUEUE_CAPACITY: usize = 100_000;

/// Size of the buffer used to write logs to stdout or a file
const WRITE_BUFFER_SIZE: usize = 64 * 1024;

/// Maximum amount of queued logs written at once
const MAX_BATCH_SIZE: usize = 1024 * 1024;

/// Creates the queue between `ProxyLog` and `ProxyLoggerReceiver`
pub fn log_queue() -> (Sender<Vec<u8>>, Receiver<Vec<u8>>) {
    tokio::sync::mpsc::channel(LOG_QUEUE_CAPACITY)
}

/// A `io::Write` implementation that sends logs to a background service
#[derive(Debug, Clone)]
pub struct StdoutWriter<'a> {
    chan: &'a Sender<Vec<u8>>,
    dropped: &'a AtomicU64,
    skip_log: bool,
}

impl io::Write for StdoutWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.skip_log {
            if let Err(TrySendError::Full(_)) = self.chan.try_send(buf.to_vec()) {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A naive implementation of a logger that delegate sending logs to a background channel
#[derive(Debug)]
pub struct ProxyLog {
    enabled: bool,
    chan: Sender<Vec<u8>>,
    dropped: Arc<AtomicU64>,
    access_logs: bool,
    error_logs: bool,
}

impl ProxyLog {
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn new(
        sender: Sender<Vec<u8>>,
        dropped: Arc<AtomicU64>,
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
            dropped,
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
            dropped: &self.dropped,
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        let skip_error_log = !self.error_logs && meta.level() == &tracing::Level::ERROR;
        let skip_access_log = !self.access_logs && meta.fields().field("access_log").is_some();

        StdoutWriter {
            skip_log: skip_error_log || skip_access_log || !self.enabled,
            chan: &self.chan,
            dropped: &self.dropped,
        }
    }
}

/// Where logs are written to.
///
/// Writes are blocking: the logger runs in its own thread, and blocking writes
/// avoid going through tokio's blocking pool (which can panic during shutdown).
pub enum LogWriter {
    Stdout(io::Stdout),
    File(std::fs::File),
}

impl io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            LogWriter::Stdout(w) => w.write(buf),
            LogWriter::File(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            LogWriter::Stdout(w) => w.flush(),
            LogWriter::File(w) => w.flush(),
        }
    }
}

/// A background service that receives logs from the main thread and writes them to stdout
pub struct ProxyLoggerReceiver {
    receiver: Receiver<Vec<u8>>,
    dropped: Arc<AtomicU64>,
    config: Arc<Config>,
    bufwriter: io::BufWriter<LogWriter>,
    suffix: String,
    state: Inner,
    rotation: Rotation,
}

// Inner state for the LoggerReceiver
pub struct Inner {
    next_date: AtomicI64,
}

impl ProxyLoggerReceiver {
    pub fn new(
        receiver: Receiver<Vec<u8>>,
        dropped: Arc<AtomicU64>,
        config: &Arc<Config>,
    ) -> Self {
        ProxyLoggerReceiver {
            receiver,
            dropped,
            config: config.clone(),
            bufwriter: Self::new_buf_writer(LogWriter::Stdout(io::stdout())),
            suffix: String::new(),
            state: Inner {
                next_date: AtomicI64::new(0),
            },
            rotation: Rotation(crate::config::LogRotation::Never),
        }
    }

    /// Writes all queued logs, waiting for new ones when the queue is empty.
    /// Returns when all senders are dropped.
    async fn write_logs(&mut self) {
        let mut batch = Vec::with_capacity(WRITE_BUFFER_SIZE);
        while let Some(buf) = self.receiver.recv().await {
            batch.extend_from_slice(&buf);
            // Write everything that is already queued at once
            while batch.len() < MAX_BATCH_SIZE {
                let Ok(buf) = self.receiver.try_recv() else {
                    break;
                };
                batch.extend_from_slice(&buf);
            }

            self.bufwriter.write_all(&batch).ok();
            batch.clear();
            if self.receiver.is_empty() {
                self.bufwriter.flush().ok();
            }

            let dropped = self.dropped.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                tracing::warn!("dropped {dropped} log lines, logging could not keep up");
            }

            self.handle_log_rotation();
        }
        self.bufwriter.flush().ok();
    }

    /// Based on the defined rotation strategy, create a new file for the logs
    /// and set the suffix for the file name
    /// If the rotation strategy is `NEVER`, the suffix is empty
    /// If the rotation strategy is `DAILY`, the suffix is the date in the format defined in the config
    /// The state of the rotation is updated with the next date to rotate
    fn file_buf_writer(&mut self, date: time::OffsetDateTime) {
        self.suffix = if self.rotation == Rotation::NEVER {
            String::new()
        } else {
            let Ok(formatted_date) = date.format(self.rotation.date_format()) else {
                tracing::error!("Failed to format date for log file");
                return;
            };
            format!(".{formatted_date}")
        };

        let Some(log_path) = self.config.logging.path.as_ref() else {
            tracing::error!("No path configured for log file");
            return;
        };

        let mut op = std::fs::OpenOptions::new();
        let open_options = op.create(true).append(true);
        let Ok(path) = std::path::absolute(log_path) else {
            tracing::error!("Failed to get absolute path for log file");
            return;
        };
        let Ok(file) = open_options.open(path.join(format!("proksi{}.log", self.suffix))) else {
            tracing::error!("Failed to open log file");
            return;
        };

        self.bufwriter = Self::new_buf_writer(LogWriter::File(file));

        if let Some(next_date) = self.rotation.next_date(&date) {
            self.state
                .next_date
                .swap(next_date.unix_timestamp(), Ordering::Relaxed);
        }
    }

    /// Creates a new `BufWriter` for stdout or a log file
    fn new_buf_writer(writer: LogWriter) -> io::BufWriter<LogWriter> {
        io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, writer)
    }

    /// Prepares the `BufWriter` for the next log file
    fn prepare_buf_writer(&mut self) {
        if self.config.logging.path.is_some() {
            self.rotation = Rotation(self.config.logging.clone().rotation);
            self.file_buf_writer(time::OffsetDateTime::now_utc());
        }

        // Default is stdout, do nothing
    }

    /// If this method returns `Some`, we should roll to a new log file.
    /// Otherwise, if this returns we should not rotate the log file.
    fn should_rollover(&self, date: time::OffsetDateTime) -> Option<i64> {
        let next_date = self.state.next_date.load(Ordering::Acquire);
        // if the next date is 0, this appender *never* rotates log files.
        if next_date == 0 {
            return None;
        }

        if date.unix_timestamp() >= next_date {
            return Some(next_date);
        }

        None
    }

    /// Handles log rotation if the rotation strategy is not `NEVER`
    /// Also flushes the buffer writer and creates a new file if the rollover date is reached
    fn handle_log_rotation(&mut self) {
        if self.rotation == Rotation::NEVER {
            return;
        }

        // If the rotation strategy is `NEVER`, the suffix is empty
        if let Some(next_date) = self.should_rollover(time::OffsetDateTime::now_utc()) {
            let Ok(date) = time::OffsetDateTime::from_unix_timestamp(next_date) else {
                tracing::error!("Invalid log rotation date: {next_date}");
                return;
            };
            self.bufwriter.flush().ok();
            self.file_buf_writer(date);
        }
    }
}

#[async_trait]
impl Service for ProxyLoggerReceiver {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        tracing::info!("starting logger service");
        self.prepare_buf_writer();
        self.write_logs().await;
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

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::StdoutWriter;

    #[test]
    fn test_writer_drops_logs_when_queue_is_full() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let dropped = AtomicU64::new(0);
        let mut writer = StdoutWriter {
            chan: &sender,
            dropped: &dropped,
            skip_log: false,
        };

        for i in 0..5 {
            writer.write_all(format!("line {i}").as_bytes()).unwrap();
        }

        assert_eq!(receiver.try_recv().unwrap(), b"line 0");
        assert_eq!(receiver.try_recv().unwrap(), b"line 1");
        assert!(receiver.try_recv().is_err());
        assert_eq!(dropped.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_writer_skips_logs() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let dropped = AtomicU64::new(0);
        let mut writer = StdoutWriter {
            chan: &sender,
            dropped: &dropped,
            skip_log: true,
        };

        writer.write_all(b"line").unwrap();

        assert!(receiver.try_recv().is_err());
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }
}
