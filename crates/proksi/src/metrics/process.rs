//! The process (ADR 0008): CPU time and memory, read with each live stats sample (once a
//! second), and the container's limits (cgroups v2), read once

use std::sync::OnceLock;

use serde::Serialize;

/// CPU time and memory, now
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    /// User and system time, in seconds
    pub cpu_seconds: f64,
    /// Resident memory
    pub rss_bytes: u64,
    /// Memory the allocator has committed
    pub allocator_bytes: u64,
}

/// The container's limits, if any
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Limits {
    pub memory_bytes: Option<u64>,
    /// CPUs (`1.5`: one and a half)
    pub cpus: Option<f64>,
}

extern "C" {
    fn mi_process_info(
        elapsed_msecs: *mut usize,
        user_msecs: *mut usize,
        system_msecs: *mut usize,
        current_rss: *mut usize,
        peak_rss: *mut usize,
        current_commit: *mut usize,
        peak_commit: *mut usize,
        page_faults: *mut usize,
    );
}

/// The process' CPU time and memory, from mimalloc (the global allocator)
pub fn read() -> Reading {
    let (mut elapsed, mut user, mut system) = (0, 0, 0);
    let (mut rss, mut peak_rss, mut commit, mut peak_commit, mut faults) = (0, 0, 0, 0, 0);
    // SAFETY: mimalloc writes each value through its pointer, all valid for the call
    unsafe {
        mi_process_info(
            &raw mut elapsed,
            &raw mut user,
            &raw mut system,
            &raw mut rss,
            &raw mut peak_rss,
            &raw mut commit,
            &raw mut peak_commit,
            &raw mut faults,
        );
    }
    #[allow(clippy::cast_precision_loss)]
    let cpu_seconds = (user + system) as f64 / 1000.0;
    Reading {
        cpu_seconds,
        rss_bytes: rss as u64,
        allocator_bytes: commit as u64,
    }
}

/// The container's limits, read once (none outside a container, or without cgroups v2)
pub fn limits() -> Limits {
    static LIMITS: OnceLock<Limits> = OnceLock::new();
    *LIMITS.get_or_init(|| {
        let read = |file: &str| std::fs::read_to_string(format!("/sys/fs/cgroup/{file}")).ok();
        Limits {
            memory_bytes: read("memory.max").and_then(|value| memory_max(&value)),
            cpus: read("cpu.max").and_then(|value| cpu_max(&value)),
        }
    })
}

/// `memory.max`: bytes, or `max` (none)
fn memory_max(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

/// `cpu.max`: `<quota> <period>` in microseconds, or `max <period>` (none)
fn cpu_max(value: &str) -> Option<f64> {
    let mut parts = value.split_whitespace();
    let quota = parts.next()?.parse::<f64>().ok()?;
    let period = parts.next()?.parse::<f64>().ok()?;
    (period > 0.0).then(|| quota / period)
}

#[cfg(test)]
mod tests {
    use super::{cpu_max, memory_max, read};

    #[test]
    fn test_reading() {
        let reading = read();
        assert!(reading.rss_bytes > 0, "{reading:?}");
        assert!(reading.cpu_seconds >= 0.0);
    }

    #[test]
    fn test_cgroup_limits() {
        assert_eq!(memory_max("536870912\n"), Some(536_870_912));
        assert_eq!(memory_max("max\n"), None);
        assert_eq!(cpu_max("150000 100000\n"), Some(1.5));
        assert_eq!(cpu_max("max 100000\n"), None);
    }
}
