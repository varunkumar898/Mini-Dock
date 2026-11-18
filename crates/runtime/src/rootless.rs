//! Rootless container execution using unprivileged Linux user namespaces (`CLONE_NEWUSER`).
//!
//! Enables unprivileged users to execute containers without `sudo`, setuid helpers,
//! or root daemon privileges.
//!
//! References:
//! - `user_namespaces(7)`: Linux user namespace mechanics
//! - `subuid(5)`, `subgid(5)`: Subordinate user and group IDs

use alphen_common::{AlphenError, Result};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;

/// Check if the calling process is running as an unprivileged user (non-root).
pub fn is_rootless() -> bool {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: geteuid is always safe to call
        unsafe { libc::geteuid() != 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Retrieve current process effective UID and GID.
pub fn get_host_uid_gid() -> (u32, u32) {
    #[cfg(target_os = "linux")]
    {
        unsafe { (libc::geteuid() as u32, libc::getegid() as u32) }
    }
    #[cfg(not(target_os = "linux"))]
    {
        (1000, 1000)
    }
}

/// Parse user and group specification (e.g. "1000:1000", "0:0", or single "1000").
pub fn parse_user_spec(spec: &str) -> Result<(u32, u32)> {
    if let Some((u, g)) = spec.split_once(':') {
        let uid = u
            .parse::<u32>()
            .map_err(|_| AlphenError::Config(format!("Invalid UID '{u}' in user spec '{spec}'")))?;
        let gid = g
            .parse::<u32>()
            .map_err(|_| AlphenError::Config(format!("Invalid GID '{g}' in user spec '{spec}'")))?;
        Ok((uid, gid))
    } else {
        let uid = spec
            .parse::<u32>()
            .map_err(|_| AlphenError::Config(format!("Invalid UID '{spec}'")))?;
        Ok((uid, uid))
    }
}

/// Configure UID and GID mapping for a target process in a user namespace.
///
/// Uses single-mapping (`0 <host_uid> 1` and `0 <host_gid> 1`) with `deny` written to
/// `setgroups`. This allows 100% unprivileged execution without requiring setuid
/// `newuidmap` or `newgidmap` binaries.
pub fn configure_id_maps(pid: u32, host_uid: u32, host_gid: u32) -> Result<()> {
    let proc_dir = if pid == 0 {
        "/proc/self".to_string()
    } else {
        format!("/proc/{pid}")
    };

    let setgroups_path = format!("{proc_dir}/setgroups");
    let uid_map_path = format!("{proc_dir}/uid_map");
    let gid_map_path = format!("{proc_dir}/gid_map");

    #[cfg(target_os = "linux")]
    {
        // 1. Write "deny" to setgroups (mandatory for unprivileged gid_map write since Linux 3.19)
        if Path::new(&setgroups_path).exists() {
            if let Err(e) = fs::write(&setgroups_path, "deny\n") {
                log::warn!("Could not write to {setgroups_path}: {e}");
            }
        }

        // 2. Map container UID 0 to host UID
        let uid_map = format!("0 {host_uid} 1\n");
        fs::write(&uid_map_path, uid_map)
            .map_err(|e| AlphenError::Runtime(format!("Failed to write {uid_map_path}: {e}")))?;

        // 3. Map container GID 0 to host GID
        let gid_map = format!("0 {host_gid} 1\n");
        fs::write(&gid_map_path, gid_map)
            .map_err(|e| AlphenError::Runtime(format!("Failed to write {gid_map_path}: {e}")))?;

        log::debug!(
            "Configured user namespace ID mappings for PID {pid}: UID {host_uid}->0, GID {host_gid}->0"
        );
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            setgroups_path,
            uid_map_path,
            gid_map_path,
            host_uid,
            host_gid,
        );
        log::debug!("Simulating user namespace mapping on non-Linux platform");
    }

    Ok(())
}

/// Rootless mode configuration options.
#[derive(Debug, Clone, Default)]
pub struct RootlessConfig {
    /// Whether rootless mode is active.
    pub enabled: bool,
    /// Host UID mapped to container root.
    pub host_uid: u32,
    /// Host GID mapped to container root.
    pub host_gid: u32,
    /// Target container internal user/group.
    pub container_user: Option<(u32, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_spec() {
        assert_eq!(parse_user_spec("1000:1000").unwrap(), (1000, 1000));
        assert_eq!(parse_user_spec("0:0").unwrap(), (0, 0));
        assert_eq!(parse_user_spec("1001").unwrap(), (1001, 1001));
        assert!(parse_user_spec("invalid").is_err());
        assert!(parse_user_spec("1000:bad").is_err());
    }

    #[test]
    fn test_host_uid_gid_defaults() {
        let (uid, gid) = get_host_uid_gid();
        assert!(uid < u32::MAX);
        assert!(gid < u32::MAX);
    }

    #[test]
    fn test_configure_id_maps_contract() {
        let res = configure_id_maps(0, 1000, 1000);
        assert!(res.is_ok());
    }
}
