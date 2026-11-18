//! Virtual filesystem mounting (/proc, /sys, /dev, /dev/pts, volumes, and tmpfs).

use alphen_common::{AlphenError, Result};
use std::path::{Path, PathBuf};

/// Bind mount specification: `HOST_PATH:CONTAINER_PATH[:ro|:rw]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMount {
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub read_only: bool,
}

impl VolumeMount {
    /// Parse `-v /host:/container[:ro]` string.
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.is_empty() || parts.len() > 3 {
            return Err(AlphenError::Config(format!(
                "Invalid volume mount format '{s}', expected /host:/container[:ro]"
            )));
        }

        let host_path = PathBuf::from(parts[0]);
        let container_path = if parts.len() >= 2 {
            PathBuf::from(parts[1])
        } else {
            PathBuf::from(parts[0])
        };

        let read_only = if parts.len() == 3 {
            parts[2].eq_ignore_ascii_case("ro")
        } else {
            false
        };

        Ok(Self {
            host_path,
            container_path,
            read_only,
        })
    }
}

/// Mount virtual filesystems (/proc, /sys, /dev, /dev/pts, /dev/shm) inside the container rootfs.
pub fn mount_virtual_filesystems(rootfs: &Path, read_only_root: bool) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        mount_virtual_filesystems_linux(rootfs, read_only_root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (rootfs, read_only_root);
        Ok(())
    }
}

/// Apply user bind mounts (-v) and tmpfs mounts into container rootfs.
pub fn mount_volumes_and_tmpfs(
    rootfs: &Path,
    volumes: &[VolumeMount],
    tmpfs_mounts: &[String],
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        mount_volumes_and_tmpfs_linux(rootfs, volumes, tmpfs_mounts)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (rootfs, volumes, tmpfs_mounts);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn mount_virtual_filesystems_linux(rootfs: &Path, read_only_root: bool) -> Result<()> {
    use std::ffi::CString;

    // 1. Mount /proc (man 5 proc, man 7 pid_namespaces)
    let proc_dir = rootfs.join("proc");
    std::fs::create_dir_all(&proc_dir)?;
    let c_proc_target = CString::new(proc_dir.to_string_lossy().as_bytes()).unwrap();
    let c_proc_type = CString::new("proc").unwrap();
    let c_proc_source = CString::new("proc").unwrap();

    // SAFETY: mounting proc filesystem into private mount namespace
    unsafe {
        if libc::mount(
            c_proc_source.as_ptr(),
            c_proc_target.as_ptr(),
            c_proc_type.as_ptr(),
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV,
            std::ptr::null(),
        ) != 0
        {
            return Err(AlphenError::Runtime(format!(
                "Mounting /proc failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    // 2. Mount /sys (man 5 sysfs) - mounted read-only for security
    let sys_dir = rootfs.join("sys");
    std::fs::create_dir_all(&sys_dir)?;
    let c_sys_target = CString::new(sys_dir.to_string_lossy().as_bytes()).unwrap();
    let c_sys_type = CString::new("sysfs").unwrap();
    let c_sys_source = CString::new("sysfs").unwrap();

    // SAFETY: mounting sysfs filesystem into private mount namespace
    unsafe {
        if libc::mount(
            c_sys_source.as_ptr(),
            c_sys_target.as_ptr(),
            c_sys_type.as_ptr(),
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV | libc::MS_RDONLY,
            std::ptr::null(),
        ) != 0
        {
            log::warn!(
                "Mounting /sys read-only failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    // 3. Mount /dev minimal tmpfs (man 4 null, zero, urandom, tty)
    let dev_dir = rootfs.join("dev");
    std::fs::create_dir_all(&dev_dir)?;
    let c_dev_target = CString::new(dev_dir.to_string_lossy().as_bytes()).unwrap();
    let c_tmpfs = CString::new("tmpfs").unwrap();
    let c_dev_opts = CString::new("mode=755").unwrap();

    // SAFETY: mounting minimal dev tmpfs
    unsafe {
        if libc::mount(
            c_tmpfs.as_ptr(),
            c_dev_target.as_ptr(),
            c_tmpfs.as_ptr(),
            libc::MS_NOSUID | libc::MS_STRICTATIME,
            c_dev_opts.as_ptr() as *const libc::c_void,
        ) != 0
        {
            return Err(AlphenError::Runtime(format!(
                "Mounting /dev tmpfs failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    // 4. Bind-mount standard device nodes: null, zero, full, random, urandom, tty
    let dev_nodes = ["null", "zero", "full", "random", "urandom", "tty"];
    for node in dev_nodes {
        let host_node = format!("/dev/{node}");
        let target_node = dev_dir.join(node);
        if Path::new(&host_node).exists() {
            let _ = std::fs::File::create(&target_node);
            let c_src = CString::new(host_node.as_bytes()).unwrap();
            let c_dst = CString::new(target_node.to_string_lossy().as_bytes()).unwrap();
            // SAFETY: bind mounting device node from host
            unsafe {
                libc::mount(
                    c_src.as_ptr(),
                    c_dst.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                );
            }
        }
    }

    // 5. Mount /dev/pts (man 4 ptmx, devpts)
    let pts_dir = dev_dir.join("pts");
    std::fs::create_dir_all(&pts_dir)?;
    let c_pts_target = CString::new(pts_dir.to_string_lossy().as_bytes()).unwrap();
    let c_pts_type = CString::new("devpts").unwrap();
    let c_pts_opts = CString::new("newinstance,ptmxmode=0666,mode=0620").unwrap();

    // SAFETY: mounting devpts with newinstance for pty isolation
    unsafe {
        libc::mount(
            c_pts_type.as_ptr(),
            c_pts_target.as_ptr(),
            c_pts_type.as_ptr(),
            libc::MS_NOSUID | libc::MS_NOEXEC,
            c_pts_opts.as_ptr() as *const libc::c_void,
        );
    }

    // 6. Mount /dev/shm (shared memory tmpfs)
    let shm_dir = dev_dir.join("shm");
    std::fs::create_dir_all(&shm_dir)?;
    let c_shm_target = CString::new(shm_dir.to_string_lossy().as_bytes()).unwrap();
    let c_shm_opts = CString::new("mode=1777").unwrap();

    // SAFETY: mounting dev shm tmpfs
    unsafe {
        libc::mount(
            c_tmpfs.as_ptr(),
            c_shm_target.as_ptr(),
            c_tmpfs.as_ptr(),
            libc::MS_NOSUID | libc::MS_NODEV,
            c_shm_opts.as_ptr() as *const libc::c_void,
        );
    }

    // 7. Read-only rootfs remount if requested
    if read_only_root {
        let c_root_target = CString::new(rootfs.to_string_lossy().as_bytes()).unwrap();
        // SAFETY: remounting container rootfs as read-only
        unsafe {
            libc::mount(
                std::ptr::null(),
                c_root_target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
                std::ptr::null(),
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn mount_volumes_and_tmpfs_linux(
    rootfs: &Path,
    volumes: &[VolumeMount],
    tmpfs_mounts: &[String],
) -> Result<()> {
    use std::ffi::CString;

    // Apply bind mounts
    for vol in volumes {
        let rel_container = vol
            .container_path
            .strip_prefix("/")
            .unwrap_or(&vol.container_path);
        let target_in_rootfs = rootfs.join(rel_container);

        if vol.host_path.is_dir() {
            std::fs::create_dir_all(&target_in_rootfs)?;
        } else {
            if let Some(parent) = target_in_rootfs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::File::create(&target_in_rootfs);
        }

        let c_src = CString::new(vol.host_path.to_string_lossy().as_bytes()).unwrap();
        let c_dst = CString::new(target_in_rootfs.to_string_lossy().as_bytes()).unwrap();

        // SAFETY: bind mounting host directory or file into container rootfs
        unsafe {
            let ret = libc::mount(
                c_src.as_ptr(),
                c_dst.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND | libc::MS_REC,
                std::ptr::null(),
            );
            if ret != 0 {
                return Err(AlphenError::Runtime(format!(
                    "Bind mount {:?} to {:?} failed: {}",
                    vol.host_path,
                    target_in_rootfs,
                    std::io::Error::last_os_error()
                )));
            }

            if vol.read_only {
                libc::mount(
                    std::ptr::null(),
                    c_dst.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
                    std::ptr::null(),
                );
            }
        }
    }

    // Apply tmpfs mounts
    for tmpfs_path in tmpfs_mounts {
        let rel_path = tmpfs_path.trim_start_matches('/');
        let target_in_rootfs = rootfs.join(rel_path);
        std::fs::create_dir_all(&target_in_rootfs)?;

        let c_target = CString::new(target_in_rootfs.to_string_lossy().as_bytes()).unwrap();
        let c_tmpfs = CString::new("tmpfs").unwrap();

        // SAFETY: mounting tmpfs on requested container directory
        unsafe {
            let ret = libc::mount(
                c_tmpfs.as_ptr(),
                c_target.as_ptr(),
                c_tmpfs.as_ptr(),
                libc::MS_NOSUID | libc::MS_NODEV,
                std::ptr::null(),
            );
            if ret != 0 {
                return Err(AlphenError::Runtime(format!(
                    "tmpfs mount on {tmpfs_path} failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_mount_parse() {
        let v1 = VolumeMount::parse("/host/data:/app/data").unwrap();
        assert_eq!(v1.host_path, PathBuf::from("/host/data"));
        assert_eq!(v1.container_path, PathBuf::from("/app/data"));
        assert!(!v1.read_only);

        let v2 = VolumeMount::parse("/var/log:/container/log:ro").unwrap();
        assert_eq!(v2.host_path, PathBuf::from("/var/log"));
        assert_eq!(v2.container_path, PathBuf::from("/container/log"));
        assert!(v2.read_only);
    }
}
