//! OverlayFS management for container layered storage.

#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;
use std::path::{Path, PathBuf};

/// Manages overlayfs lower, upper, work, and merged directories for a container.
pub struct OverlayFs {
    /// Container root storage directory (`/var/lib/alphen-box/containers/<id>`).
    pub container_dir: PathBuf,
    /// Ordered list of lower layer directories (top layer first).
    pub lower_dirs: Vec<PathBuf>,
    /// Read-write upper directory for container changes.
    pub upper_dir: PathBuf,
    /// Scratch work directory required by overlayfs.
    pub work_dir: PathBuf,
    /// Unified mount point exposing the combined container rootfs.
    pub merged_dir: PathBuf,
}

impl OverlayFs {
    /// Create new overlay directory layout for a given container.
    pub fn new(container_dir: &Path, lower_dirs: Vec<PathBuf>) -> Result<Self> {
        let upper_dir = container_dir.join("upper");
        let work_dir = container_dir.join("work");
        let merged_dir = container_dir.join("merged");

        std::fs::create_dir_all(&upper_dir)?;
        std::fs::create_dir_all(&work_dir)?;
        std::fs::create_dir_all(&merged_dir)?;

        Ok(Self {
            container_dir: container_dir.to_path_buf(),
            lower_dirs,
            upper_dir,
            work_dir,
            merged_dir,
        })
    }

    /// Mount the unified overlayfs on `merged_dir`.
    pub fn mount(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.mount_linux()
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.mount_fallback()
        }
    }

    #[cfg(target_os = "linux")]
    fn mount_linux(&self) -> Result<()> {
        use std::ffi::CString;

        // lowerdir string: colon-separated list of directories (lower1:lower2:lower3)
        let lower_paths: Vec<String> = self
            .lower_dirs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let lowerdir_opt = lower_paths.join(":");

        let options = format!(
            "lowerdir={},upperdir={},workdir={}",
            lowerdir_opt,
            self.upper_dir.to_string_lossy(),
            self.work_dir.to_string_lossy()
        );

        let source = CString::new("overlay").unwrap();
        let target = CString::new(self.merged_dir.to_string_lossy().as_bytes()).unwrap();
        let fs_type = CString::new("overlay").unwrap();
        let data = CString::new(options).unwrap();

        // SAFETY: Calling libc::mount with valid null-terminated CStrings and checked paths
        let ret = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                fs_type.as_ptr(),
                0,
                data.as_ptr() as *const libc::c_void,
            )
        };

        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(AlphenError::Runtime(format!(
                "Failed to mount overlayfs on {:?}: {err}",
                self.merged_dir
            )));
        }

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    fn mount_fallback(&self) -> Result<()> {
        // Fallback for non-Linux hosts (e.g. Windows ARM development):
        // Layers are already sequentially populated in merged_dir
        Ok(())
    }

    /// Unmount the overlayfs filesystem.
    pub fn unmount(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::ffi::CString;

            let target = CString::new(self.merged_dir.to_string_lossy().as_bytes()).unwrap();
            // SAFETY: Calling libc::umount2 with MNT_DETACH (lazy unmount)
            let ret = unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                log::warn!("umount2 failed for {:?}: {err}", self.merged_dir);
            }
        }
        Ok(())
    }
}

impl Drop for OverlayFs {
    fn drop(&mut self) {
        let _ = self.unmount();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlayfs_directory_creation() {
        let temp = std::env::temp_dir().join("alphen_test_overlay");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let lower1 = temp.join("layer1");
        std::fs::create_dir_all(&lower1).unwrap();

        let overlay = OverlayFs::new(&temp, vec![lower1]).unwrap();
        assert!(overlay.upper_dir.exists());
        assert!(overlay.work_dir.exists());
        assert!(overlay.merged_dir.exists());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
