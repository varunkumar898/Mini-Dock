//! Linux cgroups v2 resource control (Memory, CPU, PIDs, I/O).
//!
//! Kernel documentation reference: Documentation/admin-guide/cgroup-v2.rst

use alphen_common::{AlphenError, Result};
use std::path::{Path, PathBuf};

/// Resource constraints applied to a container cgroup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResourceLimits {
    /// Maximum memory in bytes (`memory.max`).
    pub memory_bytes: Option<u64>,
    /// Maximum swap memory in bytes (`memory.swap.max`).
    pub memory_swap_bytes: Option<u64>,
    /// Fractional CPU quota (`cpu.max`), e.g. 1.5 cores.
    pub cpus: Option<f32>,
    /// CPU proportional weight (`cpu.weight`), 1 to 10000.
    pub cpu_shares: Option<u64>,
    /// Maximum task/process count (`pids.max`).
    pub pids_limit: Option<u32>,
    /// Device read bps limit string, e.g. "8:0 10485760".
    pub device_read_bps: Option<String>,
    /// Device write bps limit string, e.g. "8:0 10485760".
    pub device_write_bps: Option<String>,
}

/// Parse human-readable memory string (e.g. "64m", "512k", "1g", "1024b").
pub fn parse_memory_str(s: &str) -> Result<u64> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(AlphenError::Config("Memory string cannot be empty".into()));
    }

    let (num_part, unit) =
        if let Some(stripped) = s.strip_suffix("gb").or_else(|| s.strip_suffix('g')) {
            (stripped, 1024 * 1024 * 1024u64)
        } else if let Some(stripped) = s.strip_suffix("mb").or_else(|| s.strip_suffix('m')) {
            (stripped, 1024 * 1024u64)
        } else if let Some(stripped) = s.strip_suffix("kb").or_else(|| s.strip_suffix('k')) {
            (stripped, 1024u64)
        } else if let Some(stripped) = s.strip_suffix('b') {
            (stripped, 1u64)
        } else {
            (s.as_str(), 1u64)
        };

    let count: u64 = num_part
        .trim()
        .parse()
        .map_err(|_| AlphenError::Config(format!("Invalid memory specification: '{s}'")))?;

    Ok(count * unit)
}

/// Manages cgroups v2 hierarchy for a specific container under `/sys/fs/cgroup/alphen-box/<id>`.
pub struct CgroupManager {
    cgroup_path: PathBuf,
}

impl CgroupManager {
    /// Initialize manager for a container ID.
    pub fn new(container_id: &str) -> Self {
        let base = Path::new("/sys/fs/cgroup").join("alphen-box");
        let cgroup_path = base.join(container_id);
        Self { cgroup_path }
    }

    /// Path to the container's cgroup directory.
    pub fn path(&self) -> &Path {
        &self.cgroup_path
    }

    /// Create cgroup directory and enable subtree controllers.
    pub fn create(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.create_linux()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn create_linux(&self) -> Result<()> {
        let root_cgroup = Path::new("/sys/fs/cgroup");
        if !root_cgroup.exists() {
            return Err(AlphenError::Runtime(
                "cgroups v2 unified filesystem not mounted at /sys/fs/cgroup".into(),
            ));
        }

        // Enable controllers in root subtree_control if available
        let root_subtree = root_cgroup.join("cgroup.subtree_control");
        if root_subtree.exists() {
            let _ = std::fs::write(&root_subtree, "+cpu +memory +pids +io");
        }

        // Create alphen-box parent group
        let parent = self.cgroup_path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let parent_subtree = parent.join("cgroup.subtree_control");
        if parent_subtree.exists() {
            let _ = std::fs::write(&parent_subtree, "+cpu +memory +pids +io");
        }

        // Create container cgroup
        std::fs::create_dir_all(&self.cgroup_path)?;
        Ok(())
    }

    /// Apply resource constraints to the container cgroup.
    pub fn apply_limits(&self, limits: &ResourceLimits) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.apply_limits_linux(limits)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = limits;
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn apply_limits_linux(&self, limits: &ResourceLimits) -> Result<()> {
        // 1. Memory limit (memory.max)
        if let Some(mem) = limits.memory_bytes {
            let mem_file = self.cgroup_path.join("memory.max");
            std::fs::write(&mem_file, mem.to_string())
                .map_err(|e| AlphenError::Runtime(format!("Failed to set memory.max: {e}")))?;
        }

        // 2. Memory swap limit (memory.swap.max)
        if let Some(swap) = limits.memory_swap_bytes {
            let swap_file = self.cgroup_path.join("memory.swap.max");
            if swap_file.exists() {
                let _ = std::fs::write(&swap_file, swap.to_string());
            }
        }

        // 3. CPU quota and period (cpu.max)
        // Format: "$MAX $PERIOD" (default period: 100,000 microseconds = 100ms)
        if let Some(cpus) = limits.cpus {
            let period = 100_000u64;
            let quota = (cpus * period as f32).round() as u64;
            let cpu_max_file = self.cgroup_path.join("cpu.max");
            let payload = format!("{quota} {period}");
            std::fs::write(&cpu_max_file, payload)
                .map_err(|e| AlphenError::Runtime(format!("Failed to set cpu.max: {e}")))?;
        }

        // 4. CPU shares (cpu.weight, range 1-10000, default 100)
        // Docker shares default is 1024 (range 2-262144)
        if let Some(shares) = limits.cpu_shares {
            let weight = 1 + ((shares.saturating_sub(2)) * 9999) / 262142;
            let clamped = weight.clamp(1, 10000);
            let cpu_weight_file = self.cgroup_path.join("cpu.weight");
            if cpu_weight_file.exists() {
                let _ = std::fs::write(&cpu_weight_file, clamped.to_string());
            }
        }

        // 5. PIDs limit (pids.max)
        if let Some(pids) = limits.pids_limit {
            let pids_file = self.cgroup_path.join("pids.max");
            std::fs::write(&pids_file, pids.to_string())
                .map_err(|e| AlphenError::Runtime(format!("Failed to set pids.max: {e}")))?;
        }

        // 6. I/O throttling (io.max)
        if let Some(ref rbps) = limits.device_read_bps {
            let io_file = self.cgroup_path.join("io.max");
            if io_file.exists() {
                let payload = format!("{rbps} rbps={}", rbps);
                let _ = std::fs::write(&io_file, payload);
            }
        }
        if let Some(ref wbps) = limits.device_write_bps {
            let io_file = self.cgroup_path.join("io.max");
            if io_file.exists() {
                let payload = format!("{wbps} wbps={}", wbps);
                let _ = std::fs::write(&io_file, payload);
            }
        }

        Ok(())
    }

    /// Attach a process ID to this cgroup by writing to `cgroup.procs`.
    pub fn attach(&self, pid: u32) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let procs_file = self.cgroup_path.join("cgroup.procs");
            std::fs::write(&procs_file, pid.to_string()).map_err(|e| {
                AlphenError::Runtime(format!("Failed to attach PID {pid} to cgroup: {e}"))
            })?;
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
        }
        Ok(())
    }

    /// Remove the cgroup directory when container finishes.
    pub fn cleanup(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            if self.cgroup_path.exists() {
                // In cgroups v2, remove_dir (rmdir) succeeds once all attached tasks have exited
                if let Err(e) = std::fs::remove_dir(&self.cgroup_path) {
                    log::debug!("cgroup cleanup for {:?}: {e}", self.cgroup_path);
                }
            }
        }
        Ok(())
    }
}

impl Drop for CgroupManager {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_str() {
        assert_eq!(parse_memory_str("64m").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_memory_str("64MB").unwrap(), 64 * 1024 * 1024);
        assert_eq!(parse_memory_str("1g").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_memory_str("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_memory_str("2048b").unwrap(), 2048);
        assert_eq!(parse_memory_str("1024").unwrap(), 1024);
        assert!(parse_memory_str("invalid").is_err());
    }

    #[test]
    fn test_cgroup_path_construction() {
        let manager = CgroupManager::new("cont123");
        assert_eq!(
            manager.path(),
            Path::new("/sys/fs/cgroup/alphen-box/cont123")
        );
    }
}
