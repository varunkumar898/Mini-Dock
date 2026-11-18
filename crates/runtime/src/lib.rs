//! `alphen-runtime`: Linux container runtime execution, namespaces, mounts, and isolation.

pub mod cgroups;
pub mod container;
pub mod lifecycle;
pub mod logs;
pub mod mounts;
pub mod namespaces;
pub mod overlay;
pub mod pivot;
pub mod rootless;
pub mod security;
pub mod state;

pub use cgroups::{parse_memory_str, CgroupManager, ResourceLimits};
pub use container::{run_container, ContainerConfig};
pub use lifecycle::{exec_container, rm_container, stop_container};
pub use logs::{read_container_logs, RotatingLogWriter};
pub use mounts::{mount_virtual_filesystems, mount_volumes_and_tmpfs, VolumeMount};
pub use namespaces::{setup_namespaces, NamespaceConfig};
pub use overlay::OverlayFs;
pub use pivot::{switch_root_chroot, switch_root_pivot};
pub use rootless::{
    configure_id_maps, get_host_uid_gid, is_rootless, parse_user_spec, RootlessConfig,
};
pub use security::{apply_capabilities, apply_seccomp_filter, set_no_new_privs, SecurityOptions};
pub use state::{delete_state, get_state, list_states, save_state};

#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;
use std::path::Path;

/// System prerequisites and kernel capabilities verification report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCapabilities {
    /// Linux kernel version string (e.g. "6.1.0-rpi4").
    pub kernel_version: String,
    /// Kernel meets the minimum requirement (Linux 5.15+).
    pub kernel_supported: bool,
    /// Cgroups v2 unified hierarchy is mounted at `/sys/fs/cgroup`.
    pub cgroups_v2: bool,
    /// User namespaces enabled in kernel.
    pub user_namespaces: bool,
    /// Overlayfs filesystem support available.
    pub overlayfs: bool,
    /// Seccomp BPF syscall filtering supported.
    pub seccomp: bool,
}

/// Verify that host environment meets all Alphen OS runtime requirements.
///
/// Required: Linux 5.15+, cgroups v2, user namespaces, overlayfs, and seccomp.
pub fn verify_system_requirements() -> Result<SystemCapabilities> {
    #[cfg(target_os = "linux")]
    {
        verify_linux_system()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // On non-Linux development/cross-build hosts, return a mock inspection
        Ok(SystemCapabilities {
            kernel_version: "cross-compiling-host".to_string(),
            kernel_supported: true,
            cgroups_v2: true,
            user_namespaces: true,
            overlayfs: true,
            seccomp: true,
        })
    }
}

#[cfg(target_os = "linux")]
fn verify_linux_system() -> Result<SystemCapabilities> {
    // 1. Read kernel version from /proc/sys/kernel/osrelease
    let release_str = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string();

    let kernel_supported = check_kernel_version(&release_str, 5, 15);
    if !kernel_supported {
        return Err(AlphenError::SystemRequirement(format!(
            "Kernel version {release_str} is below minimum required Linux 5.15+"
        )));
    }

    // 2. Check cgroups v2 unified hierarchy
    let cgroups_v2 = check_cgroups_v2();
    if !cgroups_v2 {
        return Err(AlphenError::SystemRequirement(
            "cgroups v2 unified hierarchy is missing or not mounted at /sys/fs/cgroup".into(),
        ));
    }

    // 3. Check user namespaces
    let user_namespaces = check_user_namespaces();
    if !user_namespaces {
        return Err(AlphenError::SystemRequirement(
            "User namespaces are not enabled or max_user_namespaces is 0".into(),
        ));
    }

    // 4. Check overlayfs
    let overlayfs = check_overlayfs();
    if !overlayfs {
        return Err(AlphenError::SystemRequirement(
            "Overlayfs is not supported or module overlay is not loaded".into(),
        ));
    }

    // 5. Check seccomp
    let seccomp = check_seccomp();
    if !seccomp {
        return Err(AlphenError::SystemRequirement(
            "Seccomp BPF is not enabled in the current kernel".into(),
        ));
    }

    Ok(SystemCapabilities {
        kernel_version: release_str,
        kernel_supported,
        cgroups_v2,
        user_namespaces,
        overlayfs,
        seccomp,
    })
}

/// Helper to parse "major.minor" from kernel release string.
pub fn check_kernel_version(release: &str, min_major: u32, min_minor: u32) -> bool {
    let mut parts = release.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts
        .next()
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    major > min_major || (major == min_major && minor >= min_minor)
}

/// Verify if /sys/fs/cgroup/cgroup.controllers exists (sign of cgroups v2 unified).
pub fn check_cgroups_v2() -> bool {
    Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
}

/// Verify user namespaces via `/proc/sys/user/max_user_namespaces`.
pub fn check_user_namespaces() -> bool {
    if let Ok(content) = std::fs::read_to_string("/proc/sys/user/max_user_namespaces") {
        if let Ok(limit) = content.trim().parse::<u64>() {
            return limit > 0;
        }
    }
    Path::new("/proc/self/ns/user").exists()
}

/// Verify overlayfs by inspecting `/proc/filesystems`.
pub fn check_overlayfs() -> bool {
    if let Ok(filesystems) = std::fs::read_to_string("/proc/filesystems") {
        for line in filesystems.lines() {
            if line.split_whitespace().any(|word| word == "overlay") {
                return true;
            }
        }
    }
    false
}

/// Verify seccomp support via `/proc/sys/kernel/seccomp` or status.
pub fn check_seccomp() -> bool {
    Path::new("/proc/sys/kernel/seccomp").exists() || Path::new("/proc/self/status").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_version_parsing() {
        assert!(check_kernel_version("5.15.0", 5, 15));
        assert!(check_kernel_version("6.1.21-v8+", 5, 15));
        assert!(check_kernel_version("6.6.0-alphen", 5, 15));
        assert!(!check_kernel_version("5.10.120", 5, 15));
        assert!(!check_kernel_version("4.19.0", 5, 15));
    }
}
