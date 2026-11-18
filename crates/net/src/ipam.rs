//! IPAM (IP Address Management) for `alphen-box` bridge networks.

use alphen_common::{AlphenError, Result};
use alphen_core::default_networks_dir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::net::Ipv4Addr;
use std::path::PathBuf;

/// Default bridge subnet: 10.88.0.0/16.
pub const DEFAULT_BRIDGE_SUBNET: &str = "10.88.0.0/16";
/// Default bridge gateway IP: 10.88.0.1.
pub const DEFAULT_BRIDGE_GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 88, 0, 1);

/// Persistent IPAM lease registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpamState {
    /// CIDR representation of the subnet.
    pub subnet: String,
    /// Gateway IP address.
    pub gateway: String,
    /// Map of container ID to assigned IPv4 address string.
    pub leases: BTreeMap<String, String>,
}

impl Default for IpamState {
    fn default() -> Self {
        Self {
            subnet: DEFAULT_BRIDGE_SUBNET.to_string(),
            gateway: DEFAULT_BRIDGE_GATEWAY.to_string(),
            leases: BTreeMap::new(),
        }
    }
}

/// IP Address Manager for allocating and tracking container addresses.
pub struct IpamManager {
    dir: PathBuf,
}

impl IpamManager {
    /// Create manager with standard default networks directory.
    pub fn new_default() -> Self {
        Self {
            dir: default_networks_dir(),
        }
    }

    /// Create manager with custom storage directory (e.g. for testing).
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn state_file(&self) -> PathBuf {
        self.dir.join("ipam.json")
    }

    fn lock_file(&self) -> PathBuf {
        self.dir.join(".ipam.lock")
    }

    /// Acquire file lock, load state, perform mutation, write atomically, and unlock.
    fn with_locked_state<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut IpamState) -> Result<R>,
    {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir).map_err(|e| {
                AlphenError::Runtime(format!(
                    "Failed to create network directory {}: {e}",
                    self.dir.display()
                ))
            })?;
        }

        let lock_path = self.lock_file();
        let _lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| AlphenError::Runtime(format!("Failed to open IPAM lock file: {e}")))?;

        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::flock(_lock.as_raw_fd(), libc::LOCK_EX);
        }

        let mut state = self.load_state()?;
        let res = f(&mut state)?;
        self.save_state(&state)?;

        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::flock(_lock.as_raw_fd(), libc::LOCK_UN);
        }

        Ok(res)
    }

    fn load_state(&self) -> Result<IpamState> {
        let path = self.state_file();
        if !path.exists() {
            return Ok(IpamState::default());
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| AlphenError::Runtime(format!("Failed to read IPAM state: {e}")))?;
        serde_json::from_str(&content)
            .map_err(|e| AlphenError::Runtime(format!("Corrupt IPAM state JSON: {e}")))
    }

    fn save_state(&self, state: &IpamState) -> Result<()> {
        let path = self.state_file();
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| AlphenError::Runtime(format!("Failed to serialize IPAM state: {e}")))?;
        let tmp = self.dir.join(".ipam.json.tmp");
        fs::write(&tmp, content)
            .map_err(|e| AlphenError::Runtime(format!("Failed to write IPAM state: {e}")))?;
        fs::rename(&tmp, &path)
            .map_err(|e| AlphenError::Runtime(format!("Failed to commit IPAM state: {e}")))?;
        Ok(())
    }

    /// Allocate the next available IPv4 address in 10.88.0.0/16 for container.
    pub fn allocate_ip(&self, container_id: &str) -> Result<Ipv4Addr> {
        self.with_locked_state(|state| {
            if let Some(existing) = state.leases.get(container_id) {
                if let Ok(ip) = existing.parse::<Ipv4Addr>() {
                    return Ok(ip);
                }
            }

            let used_ips: HashSet<Ipv4Addr> = state
                .leases
                .values()
                .filter_map(|s| s.parse::<Ipv4Addr>().ok())
                .collect();

            // Range: 10.88.0.2 to 10.88.255.254
            for third in 0..=255u8 {
                for fourth in 2..=254u8 {
                    let candidate = Ipv4Addr::new(10, 88, third, fourth);
                    if candidate != DEFAULT_BRIDGE_GATEWAY && !used_ips.contains(&candidate) {
                        state
                            .leases
                            .insert(container_id.to_string(), candidate.to_string());
                        log::info!("Allocated IP {candidate} to container {container_id}");
                        return Ok(candidate);
                    }
                }
            }

            Err(AlphenError::Runtime(
                "IPAM pool 10.88.0.0/16 exhausted (no addresses available)".to_string(),
            ))
        })
    }

    /// Release allocated IP address when container terminates.
    pub fn release_ip(&self, container_id: &str) -> Result<Option<Ipv4Addr>> {
        self.with_locked_state(|state| {
            if let Some(ip_str) = state.leases.remove(container_id) {
                log::info!("Released IP {ip_str} from container {container_id}");
                return Ok(ip_str.parse::<Ipv4Addr>().ok());
            }
            Ok(None)
        })
    }

    /// Query assigned IP for a container if allocated.
    pub fn get_ip(&self, container_id: &str) -> Result<Option<Ipv4Addr>> {
        let state = self.load_state()?;
        if let Some(ip_str) = state.leases.get(container_id) {
            Ok(ip_str.parse::<Ipv4Addr>().ok())
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipam_allocation_and_release() {
        let temp_dir = std::env::temp_dir().join(format!(
            "alphen_test_ipam_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let manager = IpamManager::new(temp_dir.clone());

        let ip1 = manager.allocate_ip("c1").unwrap();
        let ip2 = manager.allocate_ip("c2").unwrap();

        assert_eq!(ip1, Ipv4Addr::new(10, 88, 0, 2));
        assert_eq!(ip2, Ipv4Addr::new(10, 88, 0, 3));

        // Idempotency for same container
        let ip1_repeat = manager.allocate_ip("c1").unwrap();
        assert_eq!(ip1, ip1_repeat);

        // Release c1 and re-allocate
        manager.release_ip("c1").unwrap();
        let ip3 = manager.allocate_ip("c3").unwrap();
        // Should reuse lowest available (10.88.0.2)
        assert_eq!(ip3, Ipv4Addr::new(10, 88, 0, 2));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
