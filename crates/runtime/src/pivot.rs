//! Root filesystem transition using `pivot_root` and `chroot`.

#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;
use std::path::Path;

/// Switch root filesystem to `new_root` using Linux `pivot_root(2)`.
///
/// Follows the standard Linux kernel transition sequence (man 2 pivot_root):
/// 1. Bind-mount `new_root` to itself to guarantee it is a mount point.
/// 2. Remount `/` as `MS_PRIVATE | MS_REC` so mount propagation is disabled.
/// 3. Create a temporary `.old_root` directory inside `new_root`.
/// 4. Invoke `SYS_pivot_root(new_root, .old_root)`.
/// 5. Change current directory to `/`.
/// 6. Lazy-unmount `/.old_root` with `umount2(MNT_DETACH)`.
/// 7. Remove `/.old_root`.
pub fn switch_root_pivot(new_root: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        switch_root_pivot_linux(new_root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = new_root;
        Ok(())
    }
}

/// Fallback root transition using traditional `chroot(2)`.
pub fn switch_root_chroot(new_root: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        switch_root_chroot_linux(new_root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = new_root;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn switch_root_chroot_linux(new_root: &Path) -> Result<()> {
    use std::ffi::CString;

    let c_new_root = CString::new(new_root.to_string_lossy().as_bytes()).unwrap();

    // SAFETY: Calling libc::chroot followed by chdir
    unsafe {
        if libc::chroot(c_new_root.as_ptr()) != 0 {
            return Err(AlphenError::Runtime(format!(
                "chroot to {:?} failed: {}",
                new_root,
                std::io::Error::last_os_error()
            )));
        }
        let root = CString::new("/").unwrap();
        if libc::chdir(root.as_ptr()) != 0 {
            return Err(AlphenError::Runtime(format!(
                "chdir to / failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn switch_root_pivot_linux(new_root: &Path) -> Result<()> {
    use std::ffi::CString;

    let c_new_root = CString::new(new_root.to_string_lossy().as_bytes()).unwrap();
    let c_slash = CString::new("/").unwrap();

    // SAFETY: Preparing mount points for pivot_root
    unsafe {
        // 1. Ensure new_root is a mount point by bind-mounting onto itself
        if libc::mount(
            c_new_root.as_ptr(),
            c_new_root.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        ) != 0
        {
            return Err(AlphenError::Runtime(format!(
                "Self-bind-mount on {:?} failed: {}",
                new_root,
                std::io::Error::last_os_error()
            )));
        }

        // 2. Make root private to prevent pivot propagation
        if libc::mount(
            std::ptr::null(),
            c_slash.as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        ) != 0
        {
            return Err(AlphenError::Runtime(format!(
                "Making root mount private failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    // 3. Create .old_root mount point directory
    let old_root_dir = new_root.join(".old_root");
    std::fs::create_dir_all(&old_root_dir)?;
    let c_old_root = CString::new(old_root_dir.to_string_lossy().as_bytes()).unwrap();

    // 4. Invoke SYS_pivot_root syscall
    // Linux syscall: long pivot_root(const char *new_root, const char *put_old);
    // SAFETY: SYS_pivot_root invoked with valid path pointers
    unsafe {
        let ret = libc::syscall(
            libc::SYS_pivot_root,
            c_new_root.as_ptr(),
            c_old_root.as_ptr(),
        );
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            let _ = std::fs::remove_dir(&old_root_dir);
            return Err(AlphenError::Runtime(format!(
                "pivot_root syscall failed: {err}"
            )));
        }

        // 5. Change current directory to new root
        if libc::chdir(c_slash.as_ptr()) != 0 {
            return Err(AlphenError::Runtime(format!(
                "chdir to / after pivot_root failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        // 6. Unmount old root
        let detached_old = CString::new("/.old_root").unwrap();
        if libc::umount2(detached_old.as_ptr(), libc::MNT_DETACH) != 0 {
            log::warn!(
                "Lazy unmount of /.old_root failed: {}",
                std::io::Error::last_os_error()
            );
        }

        // 7. Remove the mountpoint directory
        let _ = libc::rmdir(detached_old.as_ptr());
    }

    Ok(())
}
