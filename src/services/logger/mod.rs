use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use async_trait::async_trait;

use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};

use rotation::Rotation;
use tokio::{
    io::AsyncWriteExt,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
use tracing_subscriber::fmt::MakeWriter;

use crate::config::{Config, LogRotation};

mod rotation;

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
            skip_log = meta.level() == &tracing::Level::ERROR;
        }

        if !self.access_logs {
            skip_log = meta.fields().field("access_log").is_some();
        }

        StdoutWriter {
            skip_log: skip_log || !self.enabled,
            chan: &self.chan,
        }
    }
}

/// Common ENUM that implements the AsyncWrite trait
pub enum LogWriter {
    Stdout(tokio::io::Stdout),
    File(tokio::fs::File),
}

impl tokio::io::AsyncWrite for LogWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            LogWriter::Stdout(w) => Pin::new(w).poll_write(cx, buf),
            LogWriter::File(w) => Pin::new(w).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            LogWriter::Stdout(w) => Pin::new(w).poll_flush(cx),
            LogWriter::File(w) => Pin::new(w).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            LogWriter::Stdout(w) => Pin::new(w).poll_shutdown(cx),
            LogWriter::File(w) => Pin::new(w).poll_shutdown(cx),
        }
    }
}

/// A background service that receives logs from the main thread and writes them to stdout
/// TODO: implement log rotation/write to disk (or use an existing lightweight crate)
pub struct ProxyLoggerReceiver {
    receiver: UnboundedReceiver<Vec<u8>>,
    config: Arc<Config>,
    bufwriter: tokio::io::BufWriter<LogWriter>,
    suffix: String,
    state: Inner,
    rotation: Rotation,
}

pub struct Inner {
    next_date: AtomicUsize,
}

impl ProxyLoggerReceiver {
    pub fn new(receiver: UnboundedReceiver<Vec<u8>>, config: Arc<Config>) -> Self {
        ProxyLoggerReceiver {
            receiver,
            config,
            bufwriter: tokio::io::BufWriter::with_capacity(
                1024,
                LogWriter::Stdout(tokio::io::stdout()),
            ),
            suffix: String::new(),
            state: Inner {
                next_date: AtomicUsize::new(0),
            },
            rotation: Rotation(LogRotation::Never),
        }
    }

    async fn file_buf_writer(&mut self, date: time::OffsetDateTime) {
        self.suffix = match self.rotation {
            Rotation::NEVER => "".to_string(),
            _ => format!(".{}", date.format(&self.rotation.date_format()).unwrap()),
        };

        let path = &self.config.logging.path.as_ref().unwrap();
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.join(format!("proksi{}.log", self.suffix)))
            .await
            .unwrap();

        self.bufwriter = Self::new_buf_writer(LogWriter::File(file));

        if let Some(next_date) = self.rotation.next_date(&date) {
            self.state
                .next_date
                .swap(next_date.unix_timestamp() as usize, Ordering::Relaxed);
        }
    }

    fn new_buf_writer(writer: LogWriter) -> tokio::io::BufWriter<LogWriter> {
        tokio::io::BufWriter::with_capacity(1024, writer)
    }

    async fn prepare_buf_writer(&mut self) {
        if self.config.logging.path.is_some() {
            self.rotation = Rotation(self.config.logging.clone().rotation);
            self.file_buf_writer(time::OffsetDateTime::now_utc()).await;
        }

        // Default is stdout, do nothing
        return;
    }

    /// If this method returns `Some`, we should roll to a new log file.
    /// Otherwise, if this returns we should not rotate the log file.
    fn should_rollover(&self, date: time::OffsetDateTime) -> Option<usize> {
        let next_date = self.state.next_date.load(Ordering::Acquire);
        // if the next date is 0, this appender *never* rotates log files.
        if next_date == 0 {
            return None;
        }

        if date.unix_timestamp() as usize >= next_date {
            return Some(next_date);
        }

        None
    }

    async fn handle_log_rotation(&mut self) {
        if self.rotation == Rotation::NEVER {
            return;
        }

        if let Some(next_date) = self.should_rollover(time::OffsetDateTime::now_utc()) {
            let date = time::OffsetDateTime::from_unix_timestamp(next_date as i64).unwrap();
            self.bufwriter.flush().await.ok();
            self.file_buf_writer(date).await;
        }
    }
}

#[async_trait]
impl Service for ProxyLoggerReceiver {
    async fn start_service(&mut self, _fds: Option<ListenFds>, _shutdown: ShutdownWatch) {
        tracing::info!("starting logger service");
        self.prepare_buf_writer().await;

        while let Some(buf) = self.receiver.recv().await {
            self.bufwriter.write(&buf).await.unwrap();

            self.handle_log_rotation().await;
        }
    }

    fn name(&self) -> &str {
        "logging_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
