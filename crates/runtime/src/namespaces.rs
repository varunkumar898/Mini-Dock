//! Linux kernel namespace isolation (PID, Mount, UTS, IPC, Network, User).

#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;

/// Linux namespace creation flags and parameters.
#[derive(Debug, Clone)]
pub struct NamespaceConfig {
    /// Create independent PID namespace (man 7 pid_namespaces).
    pub new_pid: bool,
    /// Create independent Mount namespace (man 7 mount_namespaces).
    pub new_mount: bool,
    /// Create independent UTS namespace for hostname isolation (man 7 uts_namespaces).
    pub new_uts: bool,
    /// Create independent IPC namespace (man 7 ipc_namespaces).
    pub new_ipc: bool,
    /// Create independent Network namespace (man 7 network_namespaces).
    pub new_net: bool,
    /// Hostname to configure in the UTS namespace.
    pub hostname: String,
}

impl Default for NamespaceConfig {
    fn default() -> Self {
        Self {
            new_pid: true,
            new_mount: true,
            new_uts: true,
            new_ipc: true,
            new_net: false, // Network namespace managed by alphen-net in M7
            hostname: "alphen-container".to_string(),
        }
    }
}

/// Unshare specified namespaces for the calling process before forking the container init process.
pub fn setup_namespaces(config: &NamespaceConfig) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        setup_namespaces_linux(config)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn setup_namespaces_linux(config: &NamespaceConfig) -> Result<()> {
    use std::ffi::CString;

    let mut unshare_flags: libc::c_int = 0;

    if config.new_pid {
        unshare_flags |= libc::CLONE_NEWPID;
    }
    if config.new_mount {
        unshare_flags |= libc::CLONE_NEWNS;
    }
    if config.new_uts {
        unshare_flags |= libc::CLONE_NEWUTS;
    }
    if config.new_ipc {
        unshare_flags |= libc::CLONE_NEWIPC;
    }
    if config.new_net {
        unshare_flags |= libc::CLONE_NEWNET;
    }

    // SAFETY: unsharing namespaces via libc::unshare(2)
    unsafe {
        if libc::unshare(unshare_flags) != 0 {
            return Err(AlphenError::Runtime(format!(
                "unshare(0x{unshare_flags:x}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        // Set private mount propagation to avoid leaking mounts back to the host
        if config.new_mount {
            let c_slash = CString::new("/").unwrap();
            if libc::mount(
                std::ptr::null(),
                c_slash.as_ptr(),
                std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE,
                std::ptr::null(),
            ) != 0
            {
                return Err(AlphenError::Runtime(format!(
                    "Setting mount propagation to MS_PRIVATE failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
        }

        // Set hostname inside the new UTS namespace (man 2 sethostname)
        if config.new_uts {
            let c_host = CString::new(config.hostname.as_bytes()).unwrap();
            let host_len = config.hostname.len();
            if libc::sethostname(c_host.as_ptr(), host_len) != 0 {
                log::warn!(
                    "sethostname({:?}) failed: {}",
                    config.hostname,
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_config_defaults() {
        let cfg = NamespaceConfig::default();
        assert!(cfg.new_pid);
        assert!(cfg.new_mount);
        assert!(cfg.new_uts);
        assert!(cfg.new_ipc);
        assert_eq!(cfg.hostname, "alphen-container");
    }
}
