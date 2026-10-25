//! The process (ADR 0008): CPU time and memory, read with each live stats sample (once a
//! second), and the container's limits (cgroups v2), read once

use std::{ffi::c_long, sync::OnceLock};

use nix::{
    sys::{
        resource::{getrusage, UsageWho},
        time::{TimeVal, TimeValLike},
    },
    unistd::{sysconf, SysconfVar},
};
use serde::Serialize;

/// CPU time and memory, now
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    /// User and system time, in seconds
    pub cpu_seconds: f64,
    /// Resident memory
    pub rss_bytes: u64,
}

/// The container's limits, if any
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Limits {
    pub memory_bytes: Option<u64>,
    /// CPUs (`1.5`: one and a half)
    pub cpus: Option<f64>,
}

/// The process' CPU time, and its resident memory (on Linux; elsewhere, the most it had)
pub fn read() -> Reading {
    let Ok(usage) = getrusage(UsageWho::RUSAGE_SELF) else {
        return Reading::default();
    };
    #[allow(clippy::cast_precision_loss)]
    let seconds = |time: TimeVal| time.num_microseconds() as f64 / 1e6;
    Reading {
        cpu_seconds: seconds(usage.user_time()) + seconds(usage.system_time()),
        rss_bytes: rss_bytes().unwrap_or_else(|| peak_rss_bytes(usage.max_rss())),
    }
}

/// Resident memory now, from `/proc/self/statm`
fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let page = sysconf(SysconfVar::PAGE_SIZE).ok().flatten()?;
    resident(&statm, u64::try_from(page).ok()?)
}

/// `statm`'s resident pages (its second field), in bytes
fn resident(statm: &str, page: u64) -> Option<u64> {
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    pages.checked_mul(page)
}

/// `getrusage`'s peak resident memory, in bytes: macOS counts bytes, others KiB
fn peak_rss_bytes(max_rss: c_long) -> u64 {
    let max_rss = u64::try_from(max_rss).unwrap_or(0);
    if cfg!(target_os = "macos") {
        max_rss
    } else {
        max_rss.saturating_mul(1024)
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
    use super::{cpu_max, memory_max, read, resident};

    #[test]
    fn test_reading() {
        let reading = read();
        assert!(reading.rss_bytes > 0, "{reading:?}");
        assert!(reading.cpu_seconds >= 0.0);
    }

    #[test]
    fn test_resident_memory_from_statm() {
        // size resident shared text lib data dt, in pages
        assert_eq!(resident("12345 678 90 1 0 2 0\n", 4096), Some(678 * 4096));
        assert_eq!(resident("", 4096), None);
    }

    #[test]
    fn test_cgroup_limits() {
        assert_eq!(memory_max("536870912\n"), Some(536_870_912));
        assert_eq!(memory_max("max\n"), None);
        assert_eq!(cpu_max("150000 100000\n"), Some(1.5));
        assert_eq!(cpu_max("max 100000\n"), None);
    }
}
