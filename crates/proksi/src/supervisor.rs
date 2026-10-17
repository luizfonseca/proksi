//! Upgrades without dropping connections (Linux): proksi runs as a supervisor, which starts
//! the process serving requests (a worker) and replaces it when asked to.
//!
//! A new worker is started with `--upgrade`, and the previous one is sent `SIGQUIT`: it hands
//! its listening sockets over to the new worker (Pingora's graceful upgrade), stops accepting,
//! and stops once its connections are closed (`server.shutdown_timeout_secs`). The supervisor
//! stays, so a container's main process (PID 1) doesn't exit with the previous worker.
//!
//! - `SIGHUP`: upgrade, with the binary and configuration on disk (a worker sends it when
//!   settings other than hosts change). Asked again before the new worker is ready, the next
//!   upgrade waits for it.
//! - `SIGUSR1`: sent by a worker once it's ready (listening, its routes applied).
//! - `SIGTERM`, `SIGINT`: passed to the workers, which stop once their connections are closed.

use std::{
    ffi::OsString,
    hash::Hasher,
    path::Path,
    process::ExitStatus,
    sync::atomic::Ordering,
};

use nix::{sys::signal::Signal, unistd::Pid};
use tokio::{
    process::{Child, Command},
    signal::unix::{signal, SignalKind},
    task::JoinSet,
};

use crate::config::Config;

/// Set for workers: the supervisor's process id
const SUPERVISOR_ENV: &str = "PROKSI_SUPERVISOR_PID";

/// Set by a worker that runs on its own instead (see `detach`)
static DETACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this process is a worker started by a supervisor
pub fn is_worker() -> bool {
    std::env::var_os(SUPERVISOR_ENV).is_some() && !DETACHED.load(Ordering::Relaxed)
}

/// Whether to start a supervisor: decided before the configuration loads, so the supervisor
/// only holds what supervising needs. Not in a worker, nor in daemon mode asked for on the
/// command line or in `PROKSI_DAEMON`; `daemon = true` in a configuration file is found by the
/// worker, which then runs on its own (`detach`).
pub fn should_supervise() -> bool {
    !is_worker()
        && !daemon_requested(
            std::env::args_os().skip(1),
            std::env::var_os("PROKSI_DAEMON").as_deref(),
        )
}

/// `-d`, `--daemon` (or `-d` among other short flags, `-vd`), or `PROKSI_DAEMON=true`
fn daemon_requested(
    mut args: impl Iterator<Item = OsString>,
    env: Option<&std::ffi::OsStr>,
) -> bool {
    let env = env
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1");
    env || args.any(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        arg == "--daemon"
            || arg.strip_prefix('-').is_some_and(|flags| {
                !flags.starts_with('-')
                    && flags.contains('d')
                    && flags.chars().all(|flag| flag.is_ascii_alphabetic())
            })
    })
}

/// This worker runs on its own (`daemon = true` in the configuration file): its supervisor
/// stops with the process that daemonizes, and upgrades restart it instead
pub fn detach() {
    DETACHED.store(true, Ordering::Relaxed);
}

/// Asks the supervisor to replace this worker with a new one, with the configuration on disk.
/// Returns `false` without a supervisor.
pub fn request_upgrade() -> bool {
    signal_supervisor(Signal::SIGHUP)
}

/// Tells the supervisor this worker is ready: an upgrade waiting for it can start
pub fn notify_ready() {
    signal_supervisor(Signal::SIGUSR1);
}

fn signal_supervisor(signal: Signal) -> bool {
    if !is_worker() {
        return false;
    }
    let Some(pid) = std::env::var(SUPERVISOR_ENV)
        .ok()
        .and_then(|pid| pid.parse::<i32>().ok())
    else {
        return false;
    };
    nix::sys::signal::kill(Pid::from_raw(pid), signal).is_ok()
}

/// The socket the listening sockets are handed over through, the same for every worker of a
/// configuration (and different for proksi instances with other configurations)
pub fn upgrade_socket(config: &Config) -> String {
    // FNV-1a: stable across builds, for upgrades to a new binary
    let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
    hash.write(config.config_path.as_deref().unwrap_or_default().as_bytes());
    std::env::temp_dir()
        .join(format!("proksi-upgrade-{:016x}.sock", hash.finish()))
        .to_string_lossy()
        .into_owned()
}

struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// Runs the supervisor until every worker has stopped, returning the last one's exit code
pub fn run() -> Result<i32, anyhow::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(supervise())
}

async fn supervise() -> Result<i32, anyhow::Error> {
    let exe = std::env::current_exe()?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();

    let mut hangup = signal(SignalKind::hangup())?;
    let mut ready = signal(SignalKind::user_defined1())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    let mut workers = Workers::default();
    workers.start(&exe, &args)?;

    loop {
        tokio::select! {
            _ = hangup.recv() => {
                if workers.upgrading.is_some() {
                    log("upgrade requested while the new worker is starting: it follows once ready");
                    workers.pending = true;
                } else {
                    workers.upgrade(&exe, &args);
                }
            }
            _ = ready.recv() => {
                workers.upgrading = None;
                workers.upgrade_if_pending(&exe, &args);
            }
            _ = terminate.recv() => workers.signal(Signal::SIGTERM),
            _ = interrupt.recv() => workers.signal(Signal::SIGINT),
            exited = workers.exited() => {
                let Some(code) = exited else { continue };
                if workers.running.is_empty() {
                    return Ok(code);
                }
                workers.upgrade_if_pending(&exe, &args);
            }
        }
    }
}

#[derive(Default)]
struct Workers {
    /// Process ids of the running workers
    running: Vec<u32>,
    /// The worker serving new connections
    current: Option<u32>,
    /// A new worker that isn't ready yet
    upgrading: Option<u32>,
    /// An upgrade asked for while another was starting
    pending: bool,
    waiting: JoinSet<(u32, std::io::Result<ExitStatus>)>,
}

impl Workers {
    fn start(&mut self, exe: &Path, args: &[OsString]) -> Result<u32, anyhow::Error> {
        let mut child: Child = Command::new(exe)
            .args(args)
            .env(SUPERVISOR_ENV, std::process::id().to_string())
            .spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("the worker exited right away"))?;
        self.waiting.spawn(async move { (pid, child.wait().await) });
        self.running.push(pid);
        self.current = Some(pid);
        Ok(pid)
    }

    /// Starts a new worker, taking over the listening sockets of the current one
    fn upgrade(&mut self, exe: &Path, args: &[OsString]) {
        match self.start_upgrade(exe, args) {
            Ok(pid) => self.upgrading = Some(pid),
            Err(err) => log(&format!("could not upgrade: {err}")),
        }
    }

    /// Runs the upgrade asked for while another was starting, once it's done
    fn upgrade_if_pending(&mut self, exe: &Path, args: &[OsString]) {
        if self.pending && self.upgrading.is_none() {
            self.pending = false;
            self.upgrade(exe, args);
        }
    }

    fn start_upgrade(&mut self, exe: &Path, args: &[OsString]) -> Result<u32, anyhow::Error> {
        // Checked first: the current worker stops accepting once it has handed its sockets over
        let config = crate::config::load("/etc/proksi/configs")
            .map_err(|err| anyhow::anyhow!("invalid configuration, keeping the current one: {err}"))?;
        crate::config::listeners::listeners(&config).map_err(anyhow::Error::msg)?;

        let previous = self.current;
        let mut args = args.to_vec();
        if !args.iter().any(|arg| arg == "--upgrade" || arg == "-u") {
            args.push("--upgrade".into());
        }
        let pid = self.start(exe, &args)?;
        log(&format!("upgrading: started worker {pid}"));
        if let Some(previous) = previous {
            signal_worker(previous, Signal::SIGQUIT);
        }
        Ok(pid)
    }

    fn signal(&self, signal: Signal) {
        for pid in &self.running {
            signal_worker(*pid, signal);
        }
    }

    /// Waits for a worker to stop, returning its exit code (`None` without workers)
    async fn exited(&mut self) -> Option<i32> {
        if self.waiting.is_empty() {
            // Nothing to wait for: the other branches still run
            std::future::pending::<()>().await;
        }
        let (pid, status) = self.waiting.join_next().await?.ok()?;
        self.running.retain(|running| *running != pid);
        // A new worker that stopped before being ready
        if self.upgrading == Some(pid) {
            self.upgrading = None;
        }
        if self.current == Some(pid) {
            self.current = self.running.last().copied();
        }
        let code = status.ok().and_then(|status| status.code()).unwrap_or(1);
        log(&format!("worker {pid} stopped (exit code {code})"));
        Some(code)
    }
}

fn signal_worker(pid: u32, signal: Signal) {
    if let Ok(pid) = i32::try_from(pid) {
        let _ = nix::sys::signal::kill(Pid::from_raw(pid), signal);
    }
}

/// The supervisor doesn't set up proksi's logging: its few messages go to stderr
fn log(message: &str) {
    eprintln!("proksi supervisor: {message}");
}


#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::daemon_requested;

    fn requested(args: &[&str], env: Option<&str>) -> bool {
        daemon_requested(args.iter().map(OsString::from), env.map(std::ffi::OsStr::new))
    }

    #[test]
    fn test_daemon_requested_before_loading_the_configuration() {
        assert!(!requested(&[], None));
        assert!(!requested(&["-c", "/etc/proksi/"], None));
        // A value that happens to have a `d`
        assert!(!requested(&["-c", "/data/proksi.hcl"], None));
        assert!(!requested(&["-c/data/proksi.hcl"], None));
        assert!(!requested(&["--config", "-d.hcl"], None));

        assert!(requested(&["--daemon"], None));
        assert!(requested(&["-c", "/etc/proksi/", "-d"], None));
        assert!(requested(&["-dc", "/etc/proksi/"], None));
        assert!(requested(&[], Some("true")));
        assert!(requested(&[], Some("1")));
        assert!(!requested(&[], Some("false")));
    }
}
